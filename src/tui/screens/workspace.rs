use crate::tui::app::{App, ComboItem};

impl App {
    /// Create a worktree for a dir.
    pub fn create_worktree(&mut self, dir_name: &str, branch: &str) -> String {
        let dir_path = match self.dir_path(dir_name) {
            Some(p) => p,
            None => return "dir not found".to_string(),
        };
        let copy_files = self.config.repos.get(dir_name)
            .and_then(|d| d.wt())
            .map(|wt| wt.copy.clone())
            .unwrap_or_default();
        match crate::services::create_worktree(std::path::Path::new(&dir_path), branch, &copy_files) {
            Ok(wt_path) => {
                let wt_key = format!("{dir_name}--{}", branch.replace('/', "-"));
                let ip = crate::services::allocate_ip(&wt_key);
                // Generate .env.tncli + docker-compose.override.yml
                let _ = crate::services::write_env_file(&wt_path, &ip);
                let repo_dir = std::path::Path::new(&dir_path);
                let dir_cfg = self.config.repos.get(dir_name);
                let wt_cfg = dir_cfg.and_then(|d| d.wt());
                let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
                let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
                let ws_key = format!("ws-{}", branch.replace('/', "-"));
                crate::services::generate_compose_override(repo_dir, &wt_path, &ip, &compose_files, &worktree_env, branch, None, None, &[], &ws_key);
                // Ensure docker-compose.override.yml is globally gitignored
                crate::services::ensure_global_gitignore();
                self.worktrees.insert(wt_key.clone(), crate::services::WorktreeInfo {
                    branch: branch.to_string(),
                    parent_dir: dir_name.to_string(),
                    bind_ip: ip.clone(),
                    path: wt_path,
                });
                self.rebuild_combo_tree();
                format!("worktree created: {branch} (BIND_IP={ip}). Run: sudo ifconfig lo0 alias {ip}")
            }
            Err(e) => format!("worktree failed: {e}"),
        }
    }

    /// Delete a worktree.
    pub fn delete_worktree(&mut self, wt_key: &str) -> String {
        let wt = match self.worktrees.remove(wt_key) {
            Some(w) => w,
            None => return "worktree not found".to_string(),
        };
        let dir_path = match self.dir_path(&wt.parent_dir) {
            Some(p) => p,
            None => return "parent dir not found".to_string(),
        };

        // Stop tmux services immediately (fast)
        if let Some(dir) = self.config.repos.get(&wt.parent_dir) {
            for svc_name in dir.services.keys() {
                let tmux_name = self.wt_tmux_name(&wt.parent_dir, svc_name, &wt.branch);
                if self.is_running(&tmux_name) {
                    crate::tmux::graceful_stop(&self.session, &tmux_name);
                }
            }
        }

        // Release IP allocation
        if !wt.bind_ip.is_empty() {
            crate::services::release_ip(wt_key);
        }

        // Update UI immediately
        self.wt_collapsed.remove(wt_key);
        self.rebuild_combo_tree();
        let branch = wt.branch.clone();

        // Heavy cleanup in background (docker down, git worktree remove)
        let pre_delete = self.config.repos.get(&wt.parent_dir)
            .and_then(|d| d.wt())
            .map(|wt| wt.pre_delete.clone())
            .unwrap_or_default();
        let wt_path = wt.path.clone();
        let repo_dir = dir_path.clone();
        let wt_branch = wt.branch.clone();
        std::thread::spawn(move || {
            // Pre-delete commands (e.g. dip compose down)
            if !pre_delete.is_empty() && wt_path.exists() {
                let combined = pre_delete.join(" && ");
                let _ = std::process::Command::new("zsh")
                    .args(["-lc", &combined])
                    .current_dir(&wt_path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            // Remove git worktree + cleanup
            let _ = crate::services::remove_worktree(
                std::path::Path::new(&repo_dir), &wt_path, &wt_branch,
            );
        });

        format!("deleting worktree: {branch}...")
    }

    /// Setup main dir as worktree-like environment with 127.0.0.1 binding.
    pub fn setup_main_loopback(&mut self, dir_name: &str) -> String {
        let dir_path = match self.dir_path(dir_name) {
            Some(p) => p,
            None => return "dir not found".to_string(),
        };
        let wt_cfg = match self.config.repos.get(dir_name).and_then(|d| d.wt()) {
            Some(wt) => wt.clone(),
            None => return format!("worktree not configured for {dir_name}"),
        };
        let p = std::path::Path::new(&dir_path);
        let branch = self.dir_branch(dir_name).unwrap_or_else(|| "main".to_string());
        let (svc_overrides, shared_hosts) = self.resolve_shared_overrides(dir_name);
        let compose_files = if wt_cfg.compose_files.is_empty() && p.join("docker-compose.yml").is_file() {
            vec!["docker-compose.yml".to_string()]
        } else {
            wt_cfg.compose_files.clone()
        };
        let ws_key = format!("ws-{}", branch.replace('/', "-"));
        if !compose_files.is_empty() {
            crate::services::setup_main_as_worktree(
                p, &compose_files, &wt_cfg.env, &branch,
                if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                &shared_hosts, &ws_key,
            );
        }
        // Write env file
        let branch_safe = crate::services::branch_safe(&branch);
        let resolved = crate::services::resolve_env_templates(&wt_cfg.env, "127.0.0.1", &branch_safe, &branch, &ws_key);
        let env_file = wt_cfg.env_file.as_deref().unwrap_or(".env.local");
        crate::services::apply_env_overrides(p, &resolved, env_file);
        let _ = crate::services::write_env_file(p, "127.0.0.1");
        crate::services::ensure_global_gitignore();
        format!("main {dir_name} setup with 127.0.0.1. Restart services to apply.")
    }

    /// Setup ALL dirs with worktree=true to bind 127.0.0.1.
    #[allow(dead_code)]
    pub fn setup_all_main_loopback(&mut self) -> String {
        let mut count = 0;
        let dirs: Vec<(String, crate::config::WorktreeConfig)> = self.config.repos.iter()
            .filter_map(|(name, d)| d.wt().map(|wt| (name.clone(), wt.clone())))
            .collect();
        for (dir_name, wt_cfg) in &dirs {
            if let Some(dir_path) = self.dir_path(dir_name) {
                let p = std::path::Path::new(&dir_path);
                let branch = self.dir_branch(dir_name).unwrap_or_else(|| "main".to_string());
                let (svc_overrides, shared_hosts) = self.resolve_shared_overrides(dir_name);
                let compose_files = if wt_cfg.compose_files.is_empty() && p.join("docker-compose.yml").is_file() {
                    vec!["docker-compose.yml".to_string()]
                } else {
                    wt_cfg.compose_files.clone()
                };
                let ws_key = format!("ws-{}", branch.replace('/', "-"));
                if !compose_files.is_empty() {
                    crate::services::setup_main_as_worktree(
                        p, &compose_files, &wt_cfg.env, &branch,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts, &ws_key,
                    );
                }
                // Write env file for main
                let branch_safe = crate::services::branch_safe(&branch);
                let resolved = crate::services::resolve_env_templates(&wt_cfg.env, "127.0.0.1", &branch_safe, &branch, &ws_key);
                let env_file = wt_cfg.env_file.as_deref().unwrap_or(".env.local");
                crate::services::apply_env_overrides(p, &resolved, env_file);
                let _ = crate::services::write_env_file(p, "127.0.0.1");
                count += 1;
            }
        }
        crate::services::ensure_global_gitignore();
        format!("{count} dirs bound to 127.0.0.1. Restart services to apply.")
    }

    /// Start workspace creation via pipeline (TUI path).
    pub fn start_create_pipeline(
        &mut self,
        ws_name: &str,
        branch_name: &str,
        event_tx: std::sync::mpsc::Sender<crate::tui::event::AppEvent>,
    ) -> String {
        use crate::tui::app::PipelineDisplay;

        let branch = branch_name.to_string();

        self.creating_workspaces.insert(branch_name.to_string());
        self.active_pipelines.push(PipelineDisplay {
            operation: "Creating workspace".into(),
            branch: branch_name.to_string(),
            current_stage: 0,
            total_stages: 7,
            stage_name: "Preparing...".into(),
            failed: None,
        });
        self.rebuild_combo_tree();

        // Build context + run pipeline entirely in background (no UI blocking)
        let config = self.config.clone();
        let config_path = self.config_path.clone();
        let ws_name = ws_name.to_string();
        let branch_for_msg = branch.clone();
        let selected = self.ws_select_items.iter()
            .filter(|item| item.selected)
            .map(|item| (item.dir_name.clone(), item.branch.clone()))
            .collect::<Vec<_>>();
        let has_selection = !selected.is_empty() && self.ws_select_open;
        self.ws_select_open = false;
        std::thread::spawn(move || {
            use crate::pipeline;
            use std::collections::HashSet;

            let ctx = if has_selection {
                pipeline::context::CreateContext::from_config_with_selection(
                    &config, &config_path, &ws_name, &branch, selected,
                )
            } else {
                pipeline::context::CreateContext::from_config(
                    &config, &config_path, &ws_name, &branch, HashSet::new(),
                )
            };
            let ctx = match ctx {
                Ok(c) => c,
                Err(e) => {
                    let _ = event_tx.send(crate::tui::event::AppEvent::Pipeline(
                        crate::pipeline::PipelineEvent::PipelineFailed {
                            branch: branch.clone(),
                            stage: 0,
                            error: e.to_string(),
                        },
                    ));
                    return;
                }
            };

            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                while let Ok(evt) = rx.recv() {
                    if event_tx.send(crate::tui::event::AppEvent::Pipeline(evt)).is_err() {
                        break;
                    }
                }
            });
            pipeline::run_create_pipeline(ctx, tx);
        });

        format!("creating workspace {branch_for_msg}...")
    }

    /// Start workspace deletion via pipeline (TUI path).
    pub fn start_delete_pipeline(
        &mut self,
        branch_name: &str,
        event_tx: std::sync::mpsc::Sender<crate::tui::event::AppEvent>,
    ) -> (String, Option<String>) {
        use crate::pipeline;
        use crate::pipeline::context::{DeleteContext, CleanupItem, DbDropItem};
        use crate::tui::app::PipelineDisplay;
        use std::collections::HashSet;

        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
        let branch_safe = crate::services::branch_safe(branch_name);

        // Stop tmux services immediately (fast, before pipeline)
        let wt_keys: Vec<String> = self.worktrees.keys()
            .filter(|k| k.ends_with(&format!("--{}", branch_name.replace('/', "-"))))
            .cloned()
            .collect();

        let mut cleanup_items = Vec::new();
        let mut dbs_to_drop = Vec::new();

        for wt_key in &wt_keys {
            if let Some(wt) = self.worktrees.get(wt_key) {
                let dir_path = self.dir_path(&wt.parent_dir).unwrap_or_default();
                let pre_delete = self.config.repos.get(&wt.parent_dir)
                    .and_then(|d| d.wt())
                    .map(|wt| wt.pre_delete.clone())
                    .unwrap_or_default();

                // Stop tmux services immediately
                if let Some(dir) = self.config.repos.get(&wt.parent_dir) {
                    for svc_name in dir.services.keys() {
                        let tmux_name = self.wt_tmux_name(&wt.parent_dir, svc_name, &wt.branch);
                        if self.is_running(&tmux_name) {
                            crate::tmux::graceful_stop(&self.session, &tmux_name);
                        }
                    }
                }

                cleanup_items.push(CleanupItem {
                    dir_path,
                    wt_path: wt.path.clone(),
                    wt_branch: wt.branch.clone(),
                    pre_delete,
                });

                // Collect DBs to drop
                if let Some(dir) = self.config.repos.get(&wt.parent_dir) {
                    for sref in dir.wt().into_iter().flat_map(|wt| &wt.shared_services) {
                        if let Some(db_tpl) = &sref.db_name {
                            let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                                .replace("{{branch}}", branch_name);
                            let svc_def = self.config.shared_services.get(&sref.name);
                            dbs_to_drop.push(DbDropItem {
                                host: svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost").to_string(),
                                port: svc_def.and_then(|d| d.ports.first())
                                    .and_then(|p| p.split(':').next())
                                    .and_then(|p| p.parse().ok())
                                    .unwrap_or(5432),
                                db_name,
                                user: svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres").to_string(),
                                password: svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres").to_string(),
                            });
                        }
                    }
                }
            }
        }

        self.deleting_workspaces.insert(branch_name.to_string());
        self.active_pipelines.push(PipelineDisplay {
            operation: "Deleting workspace".into(),
            branch: branch_name.to_string(),
            current_stage: 0,
            total_stages: 5,
            stage_name: "Starting...".into(),
            failed: None,
        });
        self.rebuild_combo_tree();

        let ctx = DeleteContext {
            branch: branch_name.to_string(),
            config: self.config.clone(),
            config_dir,
            session: self.session.clone(),
            wt_keys: wt_keys.clone(),
            cleanup_items,
            dbs_to_drop,
            network: format!("tncli-ws-{branch_name}"),
            skip_stages: HashSet::new(),
        };

        std::thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                while let Ok(evt) = rx.recv() {
                    if event_tx.send(crate::tui::event::AppEvent::Pipeline(evt)).is_err() {
                        break;
                    }
                }
            });
            pipeline::run_delete_pipeline(ctx, tx);
        });

        let msg = format!("deleting workspace {}...", branch_name);
        (msg, None)
    }

    // Legacy create_workspace and delete_workspace_by_name removed.
    // Use start_create_pipeline() and start_delete_pipeline() instead.


    /// Open worktree menu for current dir.
    pub fn open_wt_menu(&mut self) {
        let dir_name = match self.current_combo_item() {
            Some(ComboItem::InstanceDir { dir, .. }) => dir.clone(),
            Some(ComboItem::InstanceService { dir, .. }) => dir.clone(),
            _ => return,
        };
        if !self.config.repos.get(&dir_name).is_some_and(|d| d.has_worktree()) {
            self.set_message(&format!("worktree not enabled for '{dir_name}' -- add worktree: block in tncli.yml"));
            return;
        }
        self.wt_menu_dir = dir_name;
        self.wt_menu_cursor = 0;
        self.wt_menu_open = true;
    }

    /// Open name input for creating worktree from current branch.
    pub fn create_wt_current_branch(&mut self) {
        let dir_name = self.wt_menu_dir.clone();
        let branch = match self.dir_branch(&dir_name) {
            Some(b) => b,
            None => { self.set_message("not a git repo"); return; }
        };
        self.wt_menu_open = false;
        self.wt_name_base_branch = branch;
        self.wt_name_input.clear();
        self.wt_name_input_open = true;
    }

    /// Confirm worktree/workspace creation. Returns BIND_IP if created successfully.
    pub fn confirm_wt_name(&mut self) {
        let new_branch = self.wt_name_input.trim().to_string();
        if new_branch.is_empty() {
            self.set_message("name cannot be empty");
            return;
        }
        self.wt_name_input_open = false;

        if self.ws_creating {
            self.ws_creating = false;
            // Open repo selection checklist instead of creating immediately
            self.build_ws_select(&new_branch);
        } else {
            let dir_name = self.wt_menu_dir.clone();
            let base = self.wt_name_base_branch.clone();
            let msg = self.create_worktree_new_branch(&dir_name, &new_branch, &base);
            self.set_message(&msg);
        }
    }

    /// Create worktree with a NEW branch from a base branch.
    pub fn create_worktree_new_branch(&mut self, dir_name: &str, new_branch: &str, base_branch: &str) -> String {
        let dir_path = match self.dir_path(dir_name) {
            Some(p) => p,
            None => return "dir not found".to_string(),
        };
        let wt_cfg = self.config.repos.get(dir_name).and_then(|d| d.wt());
        let copy_files = wt_cfg.map(|wt| wt.copy.clone()).unwrap_or_default();
        match crate::services::create_worktree_from_base(
            std::path::Path::new(&dir_path), new_branch, base_branch, &copy_files, None
        ) {
            Ok(wt_path) => {
                let wt_key = format!("{dir_name}--{}", new_branch.replace('/', "-"));
                let ip = crate::services::allocate_ip(&wt_key);
                let _ = crate::services::write_env_file(&wt_path, &ip);
                let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
                let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
                let repo_dir = std::path::Path::new(&dir_path);
                let ws_key = format!("ws-{}", new_branch.replace('/', "-"));
                crate::services::generate_compose_override(repo_dir, &wt_path, &ip, &compose_files, &worktree_env, new_branch, None, None, &[], &ws_key);
                crate::services::ensure_global_gitignore();
                self.worktrees.insert(wt_key.clone(), crate::services::WorktreeInfo {
                    branch: new_branch.to_string(),
                    parent_dir: dir_name.to_string(),
                    bind_ip: ip.clone(),
                    path: wt_path,
                });
                self.rebuild_combo_tree();
                format!("worktree created: {new_branch} (BIND_IP={ip}). Run migrations before starting services.")
            }
            Err(e) => format!("worktree failed: {e}"),
        }
    }

    /// Add a single repo to an existing workspace (background thread for setup).
    pub fn add_repo_to_workspace(&mut self, dir_name: &str, ws_branch: &str, repo_branch: &str) {
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
        let ws_folder = config_dir.join(format!("workspace--{ws_branch}"));

        let dir_path = match self.dir_path(dir_name) {
            Some(p) => p,
            None => { self.set_message("dir not found"); return; }
        };

        // Clone config data upfront to avoid borrow issues
        let wt_cfg_clone = self.config.repos.get(dir_name).and_then(|d| d.wt()).cloned();
        let copy_files = wt_cfg_clone.as_ref().map(|wt| wt.copy.clone()).unwrap_or_default();
        let setup_cmds = wt_cfg_clone.as_ref().map(|wt| wt.setup.clone()).unwrap_or_default();

        // Get current git branch as base
        let base_branch = self.dir_branch(dir_name).unwrap_or_else(|| "main".to_string());

        let wt_path = match crate::services::create_worktree_from_base(
            std::path::Path::new(&dir_path), repo_branch, &base_branch, &copy_files, Some(&ws_folder),
        ) {
            Ok(p) => p,
            Err(e) => { self.set_message(&format!("worktree failed: {e}")); return; }
        };

        // Reuse workspace IP if available
        let ws_ip_key = format!("ws-{}", ws_branch.replace('/', "-"));
        let allocs = crate::services::load_ip_allocations();
        let bind_ip = allocs.get(&ws_ip_key).cloned().unwrap_or_else(|| "127.0.0.1".to_string());

        let wt_key = format!("{dir_name}--{}", repo_branch.replace('/', "-"));
        let _ = crate::services::write_env_file(&wt_path, &bind_ip);

        // Configure
        if let Some(wt) = &wt_cfg_clone {
            let compose_files = wt.compose_files.clone();
            let (svc_overrides, shared_hosts) = crate::pipeline::context::resolve_shared_overrides(&self.config, dir_name);
            let ws_key = format!("ws-{}", ws_branch.replace('/', "-"));
            if !compose_files.is_empty() {
                crate::services::generate_compose_override(
                    std::path::Path::new(&dir_path), &wt_path, &bind_ip,
                    &compose_files, &wt.env, repo_branch, None,
                    if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                    &shared_hosts, &ws_key,
                );
            }
            let branch_safe = crate::services::branch_safe(repo_branch);
            let resolved = crate::services::resolve_env_templates(&wt.env, &bind_ip, &branch_safe, repo_branch, &ws_key);
            let env_file = wt.env_file.as_deref().unwrap_or(".env.local");
            crate::services::apply_env_overrides(&wt_path, &resolved, env_file);
        }

        self.worktrees.insert(wt_key, crate::services::WorktreeInfo {
            branch: repo_branch.to_string(),
            parent_dir: dir_name.to_string(),
            bind_ip: bind_ip.clone(),
            path: wt_path.clone(),
        });
        self.rebuild_combo_tree();
        if !setup_cmds.is_empty() {
            let wt_path_clone = wt_path.clone();
            let dir = dir_name.to_string();
            let tx = self.event_tx.clone();
            std::thread::spawn(move || {
                let combined = setup_cmds.join(" && ");
                let _ = std::process::Command::new("zsh")
                    .args(["-lc", &combined])
                    .current_dir(&wt_path_clone)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                if let Some(tx) = tx {
                    let _ = tx.send(crate::tui::event::AppEvent::Message(format!("setup complete: {dir}")));
                }
            });
            self.set_message(&format!("added {dir_name} to workspace, running setup..."));
        } else {
            self.set_message(&format!("added {dir_name} to workspace (BIND_IP={bind_ip})"));
        }
    }

    fn resolve_shared_overrides(&self, dir_name: &str) -> (indexmap::IndexMap<String, crate::config::ServiceOverride>, Vec<String>) {
        let dir = match self.config.repos.get(dir_name) {
            Some(d) => d,
            None => return (Default::default(), Vec::new()),
        };
        let wt_cfg = match dir.wt() {
            Some(wt) => wt,
            None => return (Default::default(), Vec::new()),
        };
        let mut overrides = wt_cfg.service_overrides.clone();
        let mut hosts: Vec<String> = Vec::new();

        for sref in &wt_cfg.shared_services {
            // Add profiles: disabled for shared services
            if !overrides.contains_key(&sref.name) {
                overrides.insert(sref.name.clone(), crate::config::ServiceOverride {
                    environment: indexmap::IndexMap::new(),
                    profiles: vec!["disabled".to_string()],
                    mem_limit: None,
                });
            }
            // Collect host from top-level shared_services definition
            if let Some(svc_def) = self.config.shared_services.get(&sref.name) {
                if let Some(host) = &svc_def.host {
                    if !hosts.contains(host) {
                        hosts.push(host.clone());
                    }
                }
            }
        }
        (overrides, hosts)
    }

}
