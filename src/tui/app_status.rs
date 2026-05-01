use std::time::Instant;

use crate::tmux;
use super::app::{App, PipelineDisplay, workspace_branch};

impl App {
    pub fn refresh_status(&mut self) {
        let svc_sess = self.svc_session();
        if tmux::session_exists(&svc_sess) {
            self.running_windows = tmux::list_windows(&svc_sess);
        } else {
            self.running_windows.clear();
        }
        // Joined service pane is in TUI window — always keep it in running_windows.
        // When service exits, user can scroll output (Ctrl-b [) and press Enter to dismiss.
        // ensure_split will recreate the right pane when the pane closes.
        if let Some(ref svc) = self.joined_service {
            self.running_windows.insert(svc.clone());
        }
        // Filter internal windows from running_windows (not real services)
        let internal: Vec<String> = self.running_windows.iter()
            .filter(|w| w.starts_with("cmd~") || *w == "_tncli_init" || *w == "_blank")
            .cloned().collect();
        for w in &internal {
            self.running_windows.remove(w);
        }
        // Clean up stopping services that are no longer running
        self.stopping_services.retain(|svc| self.running_windows.contains(svc));
        // Clean up starting services that are now running (or failed to start)
        self.starting_services.retain(|svc| !self.running_windows.contains(svc));
        // Detect pipeline state from tmux windows (pipeline~create~*, pipeline~delete~*, setup~*)
        let markers = crate::pipeline::list_active_pipelines();
        let mut changed = false;

        for (branch_safe, stage, total, stage_name) in &markers {
            let branch = branch_safe.replace('_', "-");

            // Pipeline tmux window still running?
            let has_pipeline_window = self.running_windows.iter().any(|w| {
                (w.starts_with("pipeline~create~") || w.starts_with("pipeline~delete~") || w.starts_with("setup~"))
                    && w.ends_with(&format!("~{branch_safe}"))
            });

            if has_pipeline_window {
                // Detect create vs delete from window name
                let is_delete = self.running_windows.iter().any(|w| w.starts_with("pipeline~delete~") && w.ends_with(&format!("~{branch_safe}")));
                if is_delete {
                    if !self.deleting_workspaces.contains(&branch) {
                        self.deleting_workspaces.insert(branch.clone());
                        changed = true;
                    }
                } else if !self.creating_workspaces.contains(&branch) {
                    self.creating_workspaces.insert(branch.clone());
                    changed = true;
                }
                // Update/create PipelineDisplay from marker
                let op = if is_delete { "Deleting workspace" } else { "Creating workspace" };
                if let Some(p) = self.active_pipelines.iter_mut().find(|p| p.branch == branch) {
                    p.current_stage = *stage;
                    p.total_stages = *total;
                    p.stage_name = stage_name.clone();
                } else {
                    self.active_pipelines.push(PipelineDisplay {
                        operation: op.into(),
                        branch: branch.clone(),
                        current_stage: *stage,
                        total_stages: *total,
                        stage_name: stage_name.clone(),
                        failed: None,
                    });
                    changed = true;
                }
            } else {
                // Pipeline window gone — completed or crashed, clean up
                crate::pipeline::mark_pipeline_done(branch_safe);
                if self.creating_workspaces.remove(&branch) { changed = true; }
                self.active_pipelines.retain(|p| p.branch != branch);
            }
        }

        // Also detect pipeline windows not in markers (e.g. started before markers existed)
        // Detect pipeline windows not in markers
        for win in &self.running_windows {
            if let Some(rest) = win.strip_prefix("pipeline~create~") {
                let branch = rest.replace('_', "-");
                if !self.creating_workspaces.contains(&branch) {
                    self.creating_workspaces.insert(branch);
                    changed = true;
                }
            } else if let Some(rest) = win.strip_prefix("pipeline~delete~") {
                let branch = rest.replace('_', "-");
                if !self.deleting_workspaces.contains(&branch) {
                    self.deleting_workspaces.insert(branch);
                    changed = true;
                }
            }
        }

        // Clean up — only keep if marker or tmux window still exists (NOT self-referencing)
        let is_alive = |b: &str| -> bool {
            let bs = crate::services::branch_safe(b);
            markers.iter().any(|(m, _, _, _)| *m == bs)
                || self.running_windows.iter().any(|w| (w.starts_with("pipeline~") || w.starts_with("setup~")) && w.ends_with(&format!("~{bs}")))
        };
        let before_pipelines = self.active_pipelines.len();
        self.active_pipelines.retain(|p| is_alive(&p.branch));
        let before_creating = self.creating_workspaces.len();
        let before_deleting = self.deleting_workspaces.len();
        self.creating_workspaces.retain(|b| is_alive(b));
        self.deleting_workspaces.retain(|b| is_alive(b));
        if changed || self.active_pipelines.len() != before_pipelines
            || self.creating_workspaces.len() != before_creating
            || self.deleting_workspaces.len() != before_deleting {
            self.rebuild_combo_tree();
        }
        // Clean up dead setup windows left from interrupted pipelines
        let session = self.svc_session();
        let dead_setups: Vec<String> = self.running_windows.iter()
            .filter(|w| w.starts_with("setup~"))
            .filter(|w| {
                // Check if no active pipeline owns this setup window
                let branch_part = w.rsplit('~').next().unwrap_or("");
                !self.creating_workspaces.iter().any(|b| crate::services::branch_safe(b) == branch_part)
            })
            .cloned()
            .collect();
        for w in &dead_setups {
            crate::tmux::kill_window(&session, w);
            self.running_windows.remove(w);
        }
        // Periodic background worktree scan (every 5 seconds)
        if !self.scan_pending && self.last_scan.elapsed() >= std::time::Duration::from_secs(5) {
            self.trigger_background_scan();
        }
    }

    pub fn trigger_background_scan(&mut self) {
        let Some(tx) = self.event_tx.clone() else { return };
        self.scan_pending = true;
        self.last_scan = Instant::now();

        let dir_names = self.dir_names.clone();
        let config_path = self.config_path.clone();
        let default_branch = self.config.global_default_branch().to_string();
        let session = self.session.clone();

        std::thread::spawn(move || {
            let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
            let mut worktrees = std::collections::HashMap::new();

            for dir_name in &dir_names {
                let dir_path = {
                    let p = std::path::Path::new(dir_name);
                    if p.is_absolute() {
                        dir_name.to_string()
                    } else {
                        // Try workspace folder first, fallback to config dir
                        let ws_path = config_dir.join(format!("workspace--{default_branch}")).join(dir_name);
                        if ws_path.exists() {
                            ws_path.to_string_lossy().into_owned()
                        } else {
                            config_dir.join(dir_name).to_string_lossy().into_owned()
                        }
                    }
                };
                // Prune stale worktree refs (cleans up manually deleted folders)
                let _ = std::process::Command::new("git")
                    .args(["-C", &dir_path, "worktree", "prune"])
                    .output();

                let wts = match crate::services::list_worktrees(std::path::Path::new(&dir_path)) {
                    Ok(w) => w,
                    Err(_) => continue,
                };
                let allocs = crate::services::load_ip_allocations();
                for (wt_path, branch) in wts.iter().skip(1) {
                    let wt_path = std::path::PathBuf::from(wt_path);
                    if !wt_path.exists() {
                        continue;
                    }
                    let wt_key = format!("{dir_name}--{}", branch.replace('/', "-"));
                    // Try existing allocation, or workspace-level key
                    let ws_key = wt_path.parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_string_lossy().strip_prefix("workspace--").map(|s| format!("ws-{}", s)))
                        .unwrap_or_else(|| format!("ws-{}", branch.replace('/', "-")));
                    let ip = allocs.get(&wt_key)
                        .or_else(|| allocs.get(&ws_key))
                        .cloned()
                        .unwrap_or_else(|| {
                            // Auto-allocate IP for workspace missing allocation
                            crate::services::allocate_ip(&session, &ws_key)
                        });
                    worktrees.insert(wt_key, crate::services::WorktreeInfo {
                        branch: branch.clone(),
                        parent_dir: dir_name.clone(),
                        bind_ip: ip,
                        path: wt_path,
                    });
                }
            }

            let _ = tx.send(super::event::AppEvent::WorktreeScanResult(worktrees));
        });
    }

    pub fn apply_scan_result(&mut self, worktrees: std::collections::HashMap<String, crate::services::WorktreeInfo>) {
        self.scan_pending = false;
        if self.worktrees != worktrees {
            // Register proxy routes for all workspaces with IPs
            // Collect all proxy entries (repo-level + per-service)
            let mut proxy_entries: Vec<(&str, u16)> = Vec::new();
            for (_, dir) in &self.config.repos {
                if let (Some(alias), Some(port)) = (dir.alias.as_deref(), dir.proxy_port) {
                    proxy_entries.push((alias, port));
                }
                for (svc_name, svc) in &dir.services {
                    if let Some(port) = svc.proxy_port {
                        proxy_entries.push((svc_name.as_str(), port));
                    }
                }
            }
            if !proxy_entries.is_empty() {
                let mut registered = std::collections::HashSet::new();
                for wt in worktrees.values() {
                    if wt.bind_ip.is_empty() { continue; }
                    let ws_branch = workspace_branch(wt)
                        .unwrap_or_else(|| wt.branch.clone());
                    if registered.insert(ws_branch.clone()) {
                        let bs = crate::services::branch_safe(&ws_branch);
                        let services: Vec<(&str, u16, &str)> = proxy_entries.iter()
                            .map(|&(name, port)| (name, port, wt.bind_ip.as_str()))
                            .collect();
                        crate::services::proxy::register_routes(&self.config.session, &bs, &services);
                    }
                }
            }
            self.worktrees = worktrees;
            self.rebuild_combo_tree();
        }
    }

    pub fn is_stopping(&self, svc: &str) -> bool {
        self.stopping_services.contains(svc)
    }

    pub fn is_starting(&self, svc: &str) -> bool {
        self.starting_services.contains(svc)
    }

    pub fn handle_pipeline_event(&mut self, evt: crate::pipeline::PipelineEvent) {
        use crate::pipeline::PipelineEvent;
        match evt {
            PipelineEvent::StageStarted { ref branch, index, ref name, total } => {
                if let Some(p) = self.active_pipelines.iter_mut().find(|p| p.branch == *branch) {
                    p.current_stage = index;
                    p.stage_name = name.clone();
                    p.total_stages = total;
                }
            }
            PipelineEvent::StageCompleted { .. } => {}
            PipelineEvent::StageSkipped { .. } => {}
            PipelineEvent::PipelineCompleted { ref branch } => {
                if let Some(idx) = self.active_pipelines.iter().position(|p| p.branch == *branch) {
                    let p = self.active_pipelines.remove(idx);
                    self.creating_workspaces.remove(&p.branch);

                    // For delete: immediately remove worktrees from state
                    if self.deleting_workspaces.remove(&p.branch) {
                        let suffix = format!("--{}", p.branch.replace('/', "-"));
                        let keys: Vec<String> = self.worktrees.keys()
                            .filter(|k| k.ends_with(&suffix))
                            .cloned()
                            .collect();
                        for k in keys {
                            self.worktrees.remove(&k);
                        }
                    }

                    self.rebuild_combo_tree();
                    // Trigger async scan to pick up any new worktrees (for create)
                    self.trigger_background_scan();
                    self.set_message(&format!("{} ready: {}", p.operation, p.branch));
                }
            }
            PipelineEvent::PipelineFailed { ref branch, stage, ref error } => {
                if let Some(p) = self.active_pipelines.iter_mut().find(|p| p.branch == *branch) {
                    p.failed = Some((stage, error.clone()));
                    self.set_message(&format!("Failed stage {}: {error}", stage + 1));
                }
            }
        }
    }
}
