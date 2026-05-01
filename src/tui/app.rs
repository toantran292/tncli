use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use crate::config::{Config, Shortcut};
use crate::tmux;

/// An item in the flattened Workspaces tree view.
#[derive(Debug, Clone, PartialEq)]
pub enum ComboItem {
    /// Workspace combo definition (e.g. "crm" -> api + client + comm + ai).
    Combo(String),
    /// Active workspace instance header (branch name from workspace--{branch} folder).
    Instance { branch: String, is_main: bool },
    /// Dir inside a workspace instance.
    InstanceDir { branch: String, dir: String, wt_key: String, is_main: bool },
    /// Service inside a workspace instance dir.
    InstanceService { branch: String, dir: String, wt_key: String, svc: String, tmux_name: String, is_main: bool },
}

/// Item in workspace repo selection checklist.
#[derive(Debug, Clone)]
pub struct WsSelectItem {
    pub dir_name: String,
    pub alias: String,
    pub selected: bool,
    pub branch: String,
    pub conflict: bool, // branch already used by another worktree
}

/// Check if a worktree is inside a workspace folder.
#[allow(dead_code)]
fn is_workspace_worktree(wt: &crate::services::WorktreeInfo) -> bool {
    wt.path.parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n.to_string_lossy().starts_with("workspace--"))
}

/// Extract workspace branch name from worktree path (workspace--{branch}/dir_name).
pub(crate) fn workspace_branch(wt: &crate::services::WorktreeInfo) -> Option<String> {
    wt.path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_string_lossy().strip_prefix("workspace--").map(|s| s.to_string()))
}

/// Check if worktree belongs to a specific workspace branch (for use from ui.rs).
pub fn workspace_branch_eq(wt: &crate::services::WorktreeInfo, branch: &str) -> bool {
    workspace_branch(wt).as_deref() == Some(branch)
}

pub struct App {
    pub config_path: PathBuf,
    pub config: Config,
    pub session: String,
    /// Allocated loopback IP for the main workspace (e.g. "127.0.0.2").
    pub main_bind_ip: String,
    // tree
    pub dir_names: Vec<String>,
    // worktrees
    pub worktrees: std::collections::HashMap<String, crate::services::WorktreeInfo>,
    pub deleting_workspaces: HashSet<String>, // branch names being deleted
    pub creating_workspaces: HashSet<String>, // branch names being created
    pub wt_collapsed: std::collections::HashMap<String, bool>,
    // combos
    pub combos: Vec<String>,
    pub combo_items: Vec<ComboItem>,
    pub combo_collapsed: std::collections::HashMap<String, bool>,
    pub cursor: usize,
    pub combo_log_idx: usize,
    pub running_windows: HashSet<String>,
    pub stopping_services: HashSet<String>,
    pub starting_services: HashSet<String>,
    pub message: String,
    pub message_time: Option<Instant>,
    // shortcuts popup
    pub shortcuts_open: bool,
    pub shortcuts_cursor: usize,
    pub shortcuts_items: Vec<Shortcut>,
    pub shortcuts_title: String,
    // worktree menu
    pub wt_menu_open: bool,
    pub wt_menu_cursor: usize,
    pub wt_menu_dir: String,
    // branch menu (checkout/create)
    pub branch_menu_open: bool,
    pub branch_menu_cursor: usize,
    pub branch_menu_dir: String,
    // branch picker
    pub wt_branch_open: bool,
    pub wt_branch_cursor: usize,
    pub wt_branches: Vec<String>,
    pub wt_branch_filtered: Vec<String>,
    pub wt_branch_search: String,
    pub wt_branch_searching: bool,
    // branch name input (for single worktree or workspace)
    pub wt_name_input_open: bool,
    pub wt_name_input: String,
    pub wt_name_base_branch: String,
    // workspace creation
    pub ws_creating: bool,
    pub ws_name: String,  // workspace name (from combos section)
    pub ws_source_branch: Option<String>, // source worktree branch for context-aware create
    // workspace repo selection checklist
    pub ws_select_open: bool,
    pub ws_select_cursor: usize,
    pub ws_select_branch: String,
    pub ws_select_items: Vec<WsSelectItem>,
    // workspace edit menu (add/remove repo)
    pub ws_edit_open: bool,
    pub ws_edit_cursor: usize,
    pub ws_edit_branch: String,
    // add repo picker
    pub ws_add_open: bool,
    pub ws_add_cursor: usize,
    pub ws_add_items: Vec<WsSelectItem>,
    // remove repo picker
    pub ws_remove_open: bool,
    pub ws_remove_cursor: usize,
    pub ws_remove_items: Vec<(String, String)>, // (dir_name, wt_key)
    // cheat-sheet popup
    pub cheatsheet_open: bool,
    // shared services info popup
    pub shared_info_open: bool,
    // confirm dialog
    pub confirm_open: bool,
    pub confirm_msg: String,
    pub confirm_action: ConfirmAction,
    // pipeline progress (multiple concurrent pipelines)
    pub active_pipelines: Vec<PipelineDisplay>,
    // event sender for pipeline threads
    pub event_tx: Option<std::sync::mpsc::Sender<super::event::AppEvent>>,
    // background scan timing
    pub last_scan: Instant,
    pub scan_pending: bool,
    // split-pane mode (native tmux right pane)
    pub tui_window_id: Option<String>,
    pub tui_session: Option<String>,
    pub tui_pane_id: Option<String>,   // our pane ID (e.g. "%5")
    pub right_pane_id: Option<String>, // right pane ID (e.g. "%10")
    pub joined_service: Option<String>,
    pub swap_pending: bool,
    // tmux popup
    pub pending_popup: Option<PendingPopup>,
    pub popup_stack: Vec<PendingPopup>,
}

#[derive(Debug, Clone)]
pub enum PendingPopup {
    BranchPicker { dir: String, checkout_mode: bool },
    Shortcut,
    GitMenu { dir: String, path: String },
    GitPullAll { branch: String, is_main: bool },
    WsEdit { branch: String },
    WsAdd { branch: String },
    WsRemove,
    WsRepoSelect { ws_name: String, ws_branch: String },
    NameInput { context: String },
    Confirm { action: ConfirmAction },
    Spotlight,
}

const POPUP_RESULT_FILE: &str = "/tmp/tncli-popup-result";

/// Pipeline progress display state.
pub struct PipelineDisplay {
    pub operation: String,
    pub branch: String,
    pub current_stage: usize,
    pub total_stages: usize,
    pub stage_name: String,
    pub failed: Option<(usize, String)>,
}

#[derive(Debug, Clone, Default)]
pub enum ConfirmAction {
    #[default]
    None,
    DeleteWorkspace { branch: String },
    StopAll,
}

impl App {
    pub fn new(config_path: PathBuf) -> anyhow::Result<Self> {
        let config = Config::load(&config_path)?;
        let session = config.session.clone();
        let dir_names: Vec<String> = config.repos.keys().cloned().collect();
        let combos: Vec<String> = config.all_workspaces().keys().cloned().collect();

        // Load saved collapse state
        let (_dir_collapsed, wt_collapsed, combo_collapsed) =
            load_collapse_state(&session, &dir_names);

        // Auto-create main workspace folder + migrate repos
        let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
        crate::services::ensure_main_workspace(config_dir, &config);
        crate::services::ensure_node_bind_host();
        crate::services::migrate_legacy_ips();

        // Auto-start shared services in background (if configured)
        if !config.shared_services.is_empty() {
            let config_dir_owned = config_dir.to_path_buf();
            let session_owned = config.session.clone();
            let shared = config.shared_services.clone();
            std::thread::spawn(move || {
                crate::services::generate_shared_compose(&config_dir_owned, &session_owned, &shared);
                let all: Vec<&str> = shared.keys().map(|s| s.as_str()).collect();
                crate::services::start_shared_services(&config_dir_owned, &session_owned, &all);
            });
        }

        // Allocate a loopback IP for the main workspace
        let default_branch = config.default_branch.as_deref().unwrap_or("main");
        let main_bind_ip = crate::services::main_ip(&config.session, default_branch);

        // Register proxy routes for main workspace
        let branch_safe = crate::services::branch_safe(default_branch);
        let mut proxy_services: Vec<(&str, u16, &str)> = Vec::new();
        for (_, dir) in &config.repos {
            if let (Some(alias), Some(port)) = (dir.alias.as_deref(), dir.proxy_port) {
                proxy_services.push((alias, port, main_bind_ip.as_str()));
            }
            for (svc_name, svc) in &dir.services {
                if let Some(port) = svc.proxy_port {
                    proxy_services.push((svc_name.as_str(), port, main_bind_ip.as_str()));
                }
            }
        }
        if !proxy_services.is_empty() {
            crate::services::proxy::register_routes(&config.session, &branch_safe, &proxy_services);
        }

        let mut app = Self {
            config_path,
            config,
            session,
            main_bind_ip,
            dir_names,
            worktrees: std::collections::HashMap::new(),
            deleting_workspaces: HashSet::new(),
            creating_workspaces: HashSet::new(),
            wt_collapsed,
            combos,
            combo_items: Vec::new(),
            combo_collapsed,
            cursor: 0,
            combo_log_idx: 0,
            running_windows: HashSet::new(),
            stopping_services: HashSet::new(),
            starting_services: HashSet::new(),
            message: String::new(),
            message_time: None,
            shortcuts_open: false,
            shortcuts_cursor: 0,
            shortcuts_items: Vec::new(),
            shortcuts_title: String::new(),
            wt_menu_open: false,
            wt_menu_cursor: 0,
            wt_menu_dir: String::new(),
            branch_menu_open: false,
            branch_menu_cursor: 0,
            branch_menu_dir: String::new(),
            wt_branch_open: false,
            wt_branch_cursor: 0,
            wt_branches: Vec::new(),
            wt_branch_filtered: Vec::new(),
            wt_branch_search: String::new(),
            wt_branch_searching: false,
            wt_name_input_open: false,
            wt_name_input: String::new(),
            wt_name_base_branch: String::new(),
            ws_creating: false,
            ws_name: String::new(),
            ws_source_branch: None,
            ws_select_open: false,
            ws_select_cursor: 0,
            ws_select_branch: String::new(),
            ws_select_items: Vec::new(),
            ws_edit_open: false,
            ws_edit_cursor: 0,
            ws_edit_branch: String::new(),
            ws_add_open: false,
            ws_add_cursor: 0,
            ws_add_items: Vec::new(),
            ws_remove_open: false,
            ws_remove_cursor: 0,
            ws_remove_items: Vec::new(),
            cheatsheet_open: false,
            shared_info_open: false,
            confirm_open: false,
            confirm_msg: String::new(),
            confirm_action: ConfirmAction::None,
            active_pipelines: Vec::new(),
            event_tx: None,
            last_scan: Instant::now(),
            scan_pending: false,
            tui_window_id: None,
            tui_session: None,
            tui_pane_id: None,
            right_pane_id: None,
            joined_service: None,
            swap_pending: false,
            pending_popup: None,
            popup_stack: Vec::new(),
        };
        app.scan_worktrees(); // also calls rebuild_combo_tree
        Ok(app)
    }


    /// Execute the confirmed action. Returns optional IP to teardown.
    pub fn execute_confirm(&mut self) {
        self.confirm_open = false;
        let action = std::mem::take(&mut self.confirm_action);
        match action {
            ConfirmAction::DeleteWorkspace { branch } => {
                if let Some(tx) = self.event_tx.clone() {
                    let (msg, _) = self.start_delete_pipeline(&branch, tx);
                    self.set_message(&msg);
                } else {
                    self.set_message("internal error: no event sender");
                }
            }
            ConfirmAction::StopAll => {
                self.do_stop_all();
            }
            ConfirmAction::None => {}
        }
    }

    /// Create a new branch and checkout.
    pub fn create_branch_and_checkout(&mut self, dir_name: &str, branch: &str) -> String {
        let dir_path = self.selected_work_dir(dir_name)
            .or_else(|| self.dir_path(dir_name))
            .unwrap_or_default();
        let output = std::process::Command::new("git")
            .args(["-C", &dir_path, "checkout", "-b", branch])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                self.scan_worktrees();
                format!("created and checked out {branch} in {dir_name}")
            }
            Ok(o) => format!("create branch failed: {}", String::from_utf8_lossy(&o.stderr).trim()),
            Err(e) => format!("git error: {e}"),
        }
    }

    /// Git checkout a branch in a dir (or worktree).
    pub fn git_checkout(&mut self, dir_name: &str, branch: &str) -> String {
        let dir_path = self.selected_work_dir(dir_name)
            .or_else(|| self.dir_path(dir_name))
            .unwrap_or_default();
        let output = std::process::Command::new("git")
            .args(["-C", &dir_path, "checkout", branch])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                self.scan_worktrees();
                format!("checked out {branch} in {dir_name}")
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                format!("checkout failed: {}", stderr.trim())
            }
            Err(e) => format!("git error: {e}"),
        }
    }

    /// Get actual git branch for a worktree path (reads HEAD, not worktree metadata).
    pub fn wt_git_branch(&self, path: &std::path::Path) -> Option<String> {
        let output = std::process::Command::new("git")
            .args(["-C", &path.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()?;
        if !output.status.success() { return None; }
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() { None } else { Some(branch) }
    }

    pub fn reload_config(&mut self) -> String {
        match Config::load(&self.config_path) {
            Ok(config) => {
                let old_dirs = self.dir_names.len();
                let old_combos = self.combos.len();

                self.session = config.session.clone();
                self.dir_names = config.repos.keys().cloned().collect();
                self.combos = config.all_workspaces().keys().cloned().collect();
                self.config = config;
                self.rebuild_combo_tree();
                self.clamp_cursor();

                // Re-generate shared compose + detect changes
                let shared_changed = self.regenerate_shared_compose();
                // Re-generate env files for all dirs
                self.regenerate_all_env_files();

                let svc_count: usize = self.config.repos.values().map(|d| d.services.len()).sum();
                let mut msg = format!(
                    "config reloaded -- {} dirs, {} services, {} combos (was {}/{})",
                    self.dir_names.len(), svc_count, self.combos.len(), old_dirs, old_combos
                );
                if shared_changed {
                    msg.push_str(" | shared services changed — restart to apply");
                }
                msg
            }
            Err(e) => format!("reload failed: {e}"),
        }
    }

    /// Re-generate env files (.env.tncli, env_file, compose override) for all dirs + worktrees.
    fn regenerate_all_env_files(&self) {

        for dir_name in &self.dir_names {
            let wt_cfg = match self.config.repos.get(dir_name).and_then(|d| d.wt()) {
                Some(wt) => wt.clone(),
                None => continue,
            };

            // Main workspace
            if let Some(dir_path) = self.dir_path(dir_name) {
                let p = std::path::Path::new(&dir_path);
                let branch = self.dir_branch(dir_name).unwrap_or_else(|| "main".to_string());
                let ws_key = format!("ws-{}", branch.replace('/', "-"));
                wt_cfg.apply_all_env_files(p, &self.config, &self.main_bind_ip, &branch, &ws_key);
                let _ = crate::services::write_env_file(p, &self.main_bind_ip);

                // Compose override for main
                let (svc_overrides, shared_hosts) = crate::pipeline::context::resolve_shared_overrides(&self.config, dir_name);
                let compose_files = if wt_cfg.compose_files.is_empty() && p.join("docker-compose.yml").is_file() {
                    vec!["docker-compose.yml".to_string()]
                } else {
                    wt_cfg.compose_files.clone()
                };
                if !compose_files.is_empty() {
                    crate::services::generate_compose_override(
                        p, p, &self.main_bind_ip, &compose_files, &wt_cfg.env, &branch, None,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts, &ws_key, &self.config, &wt_cfg.databases,
                    );
                }
            }

            // Worktrees
            for (_, wt) in &self.worktrees {
                if wt.parent_dir != *dir_name { continue; }
                let ws_branch = workspace_branch(wt).unwrap_or_else(|| wt.branch.clone());
                let ws_key = format!("ws-{}", ws_branch.replace('/', "-"));
                wt_cfg.apply_all_env_files(&wt.path, &self.config, &wt.bind_ip, &ws_branch, &ws_key);
                let _ = crate::services::write_env_file(&wt.path, &wt.bind_ip);

                // Compose override for worktree
                let (svc_overrides, shared_hosts) = crate::pipeline::context::resolve_shared_overrides(&self.config, dir_name);
                let compose_files = if wt_cfg.compose_files.is_empty() && wt.path.join("docker-compose.yml").is_file() {
                    vec!["docker-compose.yml".to_string()]
                } else {
                    wt_cfg.compose_files.clone()
                };
                if !compose_files.is_empty() {
                    let dir_path = self.dir_path(dir_name).unwrap_or_default();
                    crate::services::generate_compose_override(
                        std::path::Path::new(&dir_path), &wt.path, &wt.bind_ip,
                        &compose_files, &wt_cfg.env, &ws_branch, None,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts, &ws_key, &self.config, &wt_cfg.databases,
                    );
                }
            }
        }
    }

    /// Re-generate docker-compose.shared.yml. Returns true if content changed.
    fn regenerate_shared_compose(&self) -> bool {
        if self.config.shared_services.is_empty() {
            return false;
        }
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
        let shared_path = config_dir.join("docker-compose.shared.yml");
        let old_content = std::fs::read_to_string(&shared_path).unwrap_or_default();
        crate::services::generate_shared_compose(config_dir, &self.session, &self.config.shared_services);
        let new_content = std::fs::read_to_string(&shared_path).unwrap_or_default();
        old_content != new_content
    }

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
            let _ = std::process::Command::new("tmux")
                .args(["kill-window", "-t", &format!("={session}:{w}")])
                .output();
            self.running_windows.remove(w);
        }
        // Periodic background worktree scan (every 5 seconds)
        if !self.scan_pending && self.last_scan.elapsed() >= std::time::Duration::from_secs(5) {
            self.trigger_background_scan();
        }
    }

    /// Spawn background thread to scan worktrees without blocking UI.
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

    /// Apply worktree scan result from background thread.
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
                    let ws_branch = super::app::workspace_branch(wt)
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

    /// Handle pipeline progress event from background thread.
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

    /// Current item under cursor in the services tree.
    /// Current item under cursor in the workspaces tree.
    pub fn current_combo_item(&self) -> Option<&ComboItem> {
        self.combo_items.get(self.cursor)
    }

    /// Get dir name for current selection.
    pub fn selected_dir_name(&self) -> Option<String> {
        match self.current_combo_item()? {
            ComboItem::InstanceDir { dir, .. } => Some(dir.clone()),
            ComboItem::InstanceService { dir, .. } => Some(dir.clone()),
            _ => None,
        }
    }

    pub fn is_running(&self, svc: &str) -> bool {
        self.running_windows.contains(svc)
    }

    /// Build unique tmux window name for worktree service: {alias}~{svc}~{branch_safe}
    pub(crate) fn wt_tmux_name(&self, dir_name: &str, svc_name: &str, branch: &str) -> String {
        let alias = self.config.repos.get(dir_name)
            .and_then(|d| d.alias.as_deref())
            .unwrap_or(dir_name);
        let branch_safe = branch.replace('/', "-");
        format!("{alias}~{svc_name}~{branch_safe}")
    }

    /// Get working directory for a dir_name, resolved through main workspace folder.
    pub fn dir_path(&self, dir_name: &str) -> Option<String> {
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
        let p = std::path::Path::new(dir_name);
        if p.is_absolute() {
            return Some(dir_name.to_string());
        }
        // Try workspace folder first
        let branch = self.config.global_default_branch();
        let ws_path = config_dir.join(format!("workspace--{branch}")).join(dir_name);
        if ws_path.exists() {
            return Some(ws_path.to_string_lossy().into_owned());
        }
        // Fallback: direct path (pre-migration)
        Some(config_dir.join(dir_name).to_string_lossy().into_owned())
    }

    /// Get main workspace folder path.
    pub fn main_workspace_dir(&self) -> std::path::PathBuf {
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
        let branch = self.config.global_default_branch();
        config_dir.join(format!("workspace--{branch}"))
    }

    /// Open editor (zed or code) for the current selection.
    pub fn open_editor(&mut self) {
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));

        let path: Option<String> = match self.current_combo_item().cloned() {
            Some(ComboItem::Combo(_)) => {
                self.set_message("select a workspace instance");
                return;
            }
            Some(ComboItem::Instance { branch, is_main }) => {
                if is_main {
                    Some(self.main_workspace_dir().to_string_lossy().into_owned())
                } else {
                    Some(config_dir.join(format!("workspace--{branch}")).to_string_lossy().into_owned())
                }
            }
            Some(ComboItem::InstanceDir { wt_key, dir, is_main, .. }) => {
                if is_main {
                    self.dir_path(&dir)
                } else {
                    self.worktrees.get(&wt_key).map(|wt| wt.path.to_string_lossy().into_owned())
                }
            }
            Some(ComboItem::InstanceService { wt_key, dir, is_main, .. }) => {
                if is_main {
                    self.dir_path(&dir)
                } else {
                    self.worktrees.get(&wt_key).map(|wt| wt.path.to_string_lossy().into_owned())
                }
            }
            None => None,
        };

        let Some(path) = path else {
            self.set_message("no selection");
            return;
        };

        if std::process::Command::new("zed").arg(&path).spawn().is_ok() {
            self.set_message("opened in zed");
        } else if std::process::Command::new("code").arg(&path).spawn().is_ok() {
            self.set_message("opened in code");
        } else {
            self.set_message("no editor found");
        }
    }

    /// Get current git branch for a dir. Returns None if not a git repo.
    pub fn dir_branch(&self, dir_name: &str) -> Option<String> {
        let dir_path = self.dir_path(dir_name)?;
        let output = std::process::Command::new("git")
            .args(["-C", &dir_path, "rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() { None } else { Some(branch) }
    }



    /// Get the selected shortcut's cmd, desc, and working dir.
    pub fn selected_shortcut(&self) -> Option<(String, String, String)> {
        let shortcut = self.shortcuts_items.get(self.shortcuts_cursor)?;
        let dir_name = self.selected_dir_name()?;

        // Use worktree path if in workspace/worktree context
        let dir_path = self.selected_work_dir(&dir_name)
            .unwrap_or_else(|| self.dir_path(&dir_name).unwrap_or_default());

        // Build env exports from worktree config so shortcuts use correct DB/Redis URLs
        let mut cmd = String::new();
        if let Some(wt_cfg) = self.config.repos.get(&dir_name).and_then(|d| d.wt()) {
            let (bind_ip, branch) = match self.current_combo_item() {
                Some(ComboItem::InstanceDir { wt_key, is_main, branch, .. }) |
                Some(ComboItem::InstanceService { wt_key, is_main, branch, .. }) => {
                    if *is_main {
                        (self.main_bind_ip.clone(), branch.clone())
                    } else {
                        let ip = self.worktrees.get(wt_key)
                            .map(|wt| wt.bind_ip.clone())
                            .unwrap_or_else(|| self.main_bind_ip.clone());
                        (ip, branch.clone())
                    }
                }
                _ => (self.main_bind_ip.clone(), "main".to_string()),
            };
            let branch_safe = crate::services::branch_safe(&branch);
            let ws_key = format!("ws-{}", branch.replace('/', "-"));
            let db_names: Vec<String> = wt_cfg.databases.iter()
                .map(|tpl| {
                    let name = tpl.replace("{{branch_safe}}", &branch_safe).replace("{{branch}}", &branch);
                    format!("{}_{name}", self.config.session)
                })
                .collect();
            // NODE_OPTIONS for dns resolution
            let home = std::env::var("HOME").unwrap_or_default();
            let patch = format!("{home}/.tncli/node-bind-host.js");
            if std::path::Path::new(&patch).exists() {
                cmd.push_str(&format!("export NODE_OPTIONS=\"--dns-result-order=ipv4first --require {patch} ${{NODE_OPTIONS:-}}\" && "));
            }
            cmd.push_str(&format!("export BIND_IP={bind_ip}"));
            // Global env → worktree env
            let mut merged_env = self.config.env.clone();
            for (k, v) in &wt_cfg.env {
                merged_env.insert(k.clone(), v.clone());
            }
            for (k, v) in &merged_env {
                let val = v.replace("{{bind_ip}}", &bind_ip)
                    .replace("{{branch_safe}}", &branch_safe)
                    .replace("{{branch}}", &branch);
                let val = crate::services::resolve_slot_templates(&val, &ws_key);
                let val = crate::services::resolve_config_templates(&val, &self.config, &branch_safe);
                let val = crate::services::resolve_db_templates(&val, &db_names);
                cmd.push_str(&format!(" && export {k}='{val}'"));
            }
            cmd.push_str(" && ");
        }
        cmd.push_str(&shortcut.cmd);

        Some((cmd, shortcut.desc.clone(), dir_path))
    }

    /// Get working directory for current selection — worktree path if applicable, else repo dir.
    pub fn selected_work_dir(&self, dir_name: &str) -> Option<String> {
        match self.current_combo_item()? {
            ComboItem::InstanceDir { wt_key, is_main, dir, .. } | ComboItem::InstanceService { wt_key, is_main, dir, .. } => {
                if *is_main {
                    self.dir_path(dir)
                } else {
                    self.worktrees.get(wt_key).map(|wt| wt.path.to_string_lossy().into_owned())
                }
            }
            _ => {
                let _ = dir_name;
                None
            }
        }
    }

    /// Combo running services for log cycling.
    pub fn current_list_len(&self) -> usize {
        self.combo_items.len()
    }

    pub fn clamp_cursor(&mut self) {
        let len = self.current_list_len();
        if self.cursor >= len && len > 0 {
            self.cursor = len - 1;
        }
    }



    /// Check if a branch is already used by a worktree for this dir.
    pub fn has_branch_conflict(&self, dir_name: &str, branch: &str) -> bool {
        self.worktrees.values().any(|wt| wt.parent_dir == dir_name && wt.branch == branch)
    }

    /// Create workspace: all repos checkout same branch. No checklist needed.
    pub fn build_ws_select(&mut self, ws_branch: &str) {
        let ws_name = self.ws_name.clone();

        // Check for conflicts
        let all_ws = self.config.all_workspaces();
        let entries = all_ws.get(&ws_name).cloned().unwrap_or_default();
        let mut unique_dirs = Vec::new();
        for entry in &entries {
            if let Some((dir, _)) = self.config.find_service_entry_quiet(entry) {
                if !unique_dirs.contains(&dir) { unique_dirs.push(dir); }
            }
        }

        let conflicts: Vec<String> = unique_dirs.iter()
            .filter(|d| self.has_branch_conflict(d, ws_branch))
            .cloned().collect();
        if !conflicts.is_empty() {
            self.set_message(&format!("branch conflict: {}", conflicts.join(", ")));
            return;
        }

        // Populate ws_select_items with per-repo base branch
        // If source_branch is set (creating from non-main), use each repo's current branch as base
        // If None (creating from main), use each repo's default branch
        self.ws_select_items = unique_dirs.iter().map(|dir_name| {
            let alias = self.config.repos.get(dir_name)
                .and_then(|d| d.alias.as_deref())
                .unwrap_or(dir_name)
                .to_string();
            let base = if let Some(ref src) = self.ws_source_branch {
                // Find repo's current branch in the source worktree
                self.worktrees.values()
                    .find(|wt| wt.parent_dir == *dir_name && workspace_branch(wt).as_deref() == Some(src))
                    .and_then(|wt| self.wt_git_branch(&wt.path))
                    .unwrap_or_else(|| ws_branch.to_string())
            } else {
                // Main: use repo's default branch
                self.config.default_branch_for(dir_name)
            };
            WsSelectItem {
                dir_name: dir_name.clone(),
                alias,
                selected: true,
                branch: base, // per-repo base branch
                conflict: false,
            }
        }).collect();

        // Show fzf multi-select popup for repo selection
        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let items: Vec<String> = self.ws_select_items.iter()
            .map(|i| format!("{}\t{} (from {})", i.dir_name, i.alias, i.branch))
            .collect();
        let input = items.join("\n");
        let cmd = format!(
            "printf '{}' | fzf --multi --prompt='Select repos (Tab toggle, Enter confirm)> ' --with-nth=2.. --delimiter='\t' | cut -f1 > {}",
            input.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("60%", "50%", &cmd);
        self.pending_popup = Some(PendingPopup::WsRepoSelect { ws_name, ws_branch: ws_branch.to_string() });
    }

    /// Add repo to workspace — fzf popup.
    pub fn build_ws_add_list(&mut self, branch: &str) {
        let existing_dirs: Vec<String> = self.worktrees.values()
            .filter(|wt| workspace_branch(wt).as_deref() == Some(branch))
            .map(|wt| wt.parent_dir.clone())
            .collect();

        let available: Vec<String> = self.dir_names.iter()
            .filter(|d| !existing_dirs.contains(d))
            .map(|d| {
                let alias = self.config.repos.get(d)
                    .and_then(|dir| dir.alias.as_deref())
                    .unwrap_or(d);
                format!("{}\t{}", d, alias)
            })
            .collect();

        if available.is_empty() {
            self.set_message("all repos already in workspace");
            return;
        }

        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let items = available.join("\n");
        let cmd = format!(
            "printf '{}' | fzf --prompt='Add repo> ' --with-nth=2 --delimiter='\t' | cut -f1 > {}",
            items.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("50%", "40%", &cmd);
        self.pending_popup = Some(PendingPopup::WsAdd { branch: branch.to_string() });
    }

    /// Remove repo from workspace — fzf popup.
    pub fn build_ws_remove_list(&mut self, branch: &str) {
        let repos: Vec<(String, String)> = self.worktrees.iter()
            .filter(|(_, wt)| workspace_branch(wt).as_deref() == Some(branch))
            .map(|(wt_key, wt)| {
                let alias = self.config.repos.get(&wt.parent_dir)
                    .and_then(|d| d.alias.as_deref())
                    .unwrap_or(&wt.parent_dir)
                    .to_string();
                (wt_key.clone(), alias)
            })
            .collect();

        if repos.is_empty() {
            self.set_message("no repos to remove");
            return;
        }

        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let items: Vec<String> = repos.iter().map(|(k, a)| format!("{}\t{}", k, a)).collect();
        let input = items.join("\n");
        let cmd = format!(
            "printf '{}' | fzf --prompt='Remove repo> ' --with-nth=2 --delimiter='\t' | cut -f1 > {}",
            input.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("50%", "40%", &cmd);
        self.pending_popup = Some(PendingPopup::WsRemove);
    }

    // ── Split-pane mode ──

    /// Service session name: tncli_{config_session} (e.g. "tncli_boom").
    /// Services live here, separate from the TUI session.
    pub fn svc_session(&self) -> String {
        format!("tncli_{}", self.session)
    }

    /// Ensure split pane exists. Recreate if lost.
    pub fn ensure_split(&mut self) {
        let wid = match &self.tui_window_id {
            Some(id) => id.clone(),
            None => return,
        };
        let panes = tmux::list_pane_ids(&wid);
        if panes.len() < 2 {
            let placeholder = "echo; echo '  Select a running service'; echo; echo '  j/k navigate · Tab focus · n/N cycle'; stty -echo; tail -f /dev/null";
            tmux::split_window_right(75, Some(placeholder));
            let all_panes = tmux::list_pane_ids(&wid);
            self.right_pane_id = all_panes.into_iter()
                .find(|p| self.tui_pane_id.as_ref() != Some(p));
            if let Some(ref rpid) = self.right_pane_id {
                tmux::set_pane_title(rpid, "service");
            }
            self.joined_service = None;
        }
    }

    /// Initialize the tmux split layout (right pane placeholder).
    pub fn setup_split(&mut self) {
        let wid = match &self.tui_window_id {
            Some(id) => id.clone(),
            None => return,
        };

        // Record our pane ID before split
        self.tui_pane_id = tmux::current_pane_id();

        let placeholder = "echo; echo '  Select a running service'; echo; echo '  j/k navigate · Tab focus · n/N cycle'; stty -echo; tail -f /dev/null";
        tmux::split_window_right(75, Some(placeholder));

        // Detect right pane ID (the new one, not ours)
        let all_panes = tmux::list_pane_ids(&wid);
        self.right_pane_id = all_panes.into_iter()
            .find(|p| self.tui_pane_id.as_ref() != Some(p));

        tmux::set_window_option(&wid, "pane-border-status", "top");
        tmux::set_window_option(&wid, "pane-border-format",
            " #{?pane_active,#[fg=colour39#,bold],#[fg=colour252]}#{pane_title}#[default] ");
        if let Some(ref pid) = self.tui_pane_id {
            tmux::set_pane_title(pid, &self.session);
        }
        if let Some(ref pid) = self.right_pane_id {
            tmux::set_pane_title(pid, "service");
        }
    }

    /// Clean up the tmux split (swap service back, kill right pane, restore borders).
    pub fn teardown_split(&mut self) {
        let wid = match &self.tui_window_id {
            Some(id) => id.clone(),
            None => return,
        };
        let svc_sess = self.svc_session();

        // Restore service back to its window
        if let Some(svc) = self.joined_service.take() {
            if let Some(ref rpid) = self.right_pane_id {
                if tmux::window_exists(&svc_sess, &svc) {
                    let _ = tmux::swap_pane(&svc_sess, &svc, rpid);
                } else {
                    tmux::ensure_session(&svc_sess);
                    tmux::break_pane_to(rpid, &svc_sess, &svc);
                }
            }
        }
        // Kill any remaining panes that aren't our TUI pane
        if let Some(ref tui_pid) = self.tui_pane_id {
            for p in tmux::list_pane_ids(&wid) {
                if p != *tui_pid {
                    tmux::kill_pane(&p);
                }
            }
        }
        tmux::unset_window_option(&wid, "pane-border-status");
        tmux::unset_window_option(&wid, "pane-border-format");
        self.right_pane_id = None;
    }

    /// Swap the right pane to show the currently selected service.
    /// Simple approach: swap with service window directly, swap back to restore.
    /// No _blank management, no rename, no kill. Service windows always keep their name.
    pub fn swap_display_service(&mut self) {
        let svc_sess = self.svc_session();

        let new_svc = self.log_service_name();

        if new_svc == self.joined_service {
            // Same service but cursor may have changed context — update title
            if let (Some(svc), Some(rpid)) = (&self.joined_service, &self.right_pane_id) {
                let title = self.build_pane_title(svc);
                tmux::set_pane_title(rpid, &title);
            }
            return;
        }

        // Step 1: Restore current service back to its window (swap back)
        if let Some(old) = self.joined_service.take() {
            if let Some(rpid) = &self.right_pane_id {
                if tmux::window_exists(&svc_sess, &old) {
                    let _ = tmux::swap_pane(&svc_sess, &old, rpid);
                    self.redetect_right_pane();
                }
            }
        }

        // Step 2: Show new service (swap in)
        if let Some(ref new) = new_svc {
            if let Some(rpid) = &self.right_pane_id {
                if tmux::window_exists(&svc_sess, new) && tmux::swap_pane(&svc_sess, new, rpid).is_ok() {
                    self.joined_service = Some(new.clone());
                    self.redetect_right_pane();
                    if let Some(rpid) = &self.right_pane_id {
                        let title = self.build_pane_title(new);
                        tmux::set_pane_title(rpid, &title);
                    }
                }
            }
        } else if let Some(rpid) = &self.right_pane_id {
            tmux::set_pane_title(rpid, "service");
        }
    }

    /// Re-detect right pane ID after swap (pane objects move).
    pub(crate) fn redetect_right_pane(&mut self) {
        if let Some(wid) = &self.tui_window_id {
            let all_panes = tmux::list_pane_ids(wid);
            self.right_pane_id = all_panes.into_iter()
                .find(|p| self.tui_pane_id.as_ref() != Some(p));
        }
    }

    /// Build pane title for a service (e.g. "(crm-380) api~start [1/3]").
    fn build_pane_title(&self, svc: &str) -> String {
        let cycle = self.log_cycle_info();
        let branch_tag = self.selected_dir_name()
            .and_then(|d| {
                self.selected_work_dir(&d)
                    .and_then(|p| self.wt_git_branch(std::path::Path::new(&p)))
                    .or_else(|| self.dir_branch(&d))
            })
            .map(|b| format!("({b}) "))
            .unwrap_or_default();

        if let Some((cur, total)) = cycle {
            format!("{branch_tag}{svc} [{cur}/{total}]")
        } else {
            format!("{branch_tag}{svc}")
        }
    }

    // ── tmux popup ──

    /// Generic fzf menu popup. Returns selected line via temp file.
    pub fn popup_menu(&mut self, title: &str, options: &[&str], popup: PendingPopup) {
        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let items = options.join("\n");
        let cmd = format!(
            "printf '{}' | fzf --prompt='{} > ' --no-info --reverse > {}",
            items.replace('\'', "'\\''"), title.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("50%", "40%", &cmd);
        self.pending_popup = Some(popup);
    }

    /// Text input popup. Returns input via temp file.
    pub fn popup_input(&mut self, prompt: &str, popup: PendingPopup) {
        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let cmd = format!(
            "printf '\\n  {}\\n\\n  > ' && read input && echo \"$input\" > {}",
            prompt.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("50%", "20%", &cmd);
        self.pending_popup = Some(popup);
    }

    /// Yes/No confirm popup.
    pub fn popup_confirm(&mut self, msg: &str, action: ConfirmAction) {
        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let cmd = format!(
            "printf '\\n  {}\\n\\n  ' && printf '[y/N] ' && read -k1 answer && echo \"$answer\" > {}",
            msg.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("60%", "20%", &cmd);
        self.pending_popup = Some(PendingPopup::Confirm { action });
    }

    /// Spotlight launcher — fzf command palette for ALL services (regardless of collapse).
    pub fn popup_spotlight(&mut self) {
        let _ = std::fs::remove_file(POPUP_RESULT_FILE);

        // Build list from config + worktrees (not combo_items which depends on collapse)
        // Format: tmux_name\ticon svc_name      alias/branch
        let mut items: Vec<String> = Vec::new();

        // Main workspace services
        for (dir_name, dir_cfg) in &self.config.repos {
            let alias = dir_cfg.alias.as_deref().unwrap_or(dir_name);
            for svc_name in dir_cfg.services.keys() {
                let tmux_name = format!("{alias}~{svc_name}");
                let icon = if self.is_running(&tmux_name) { "●" } else { "○" };
                items.push(format!("{}\t{} {:<15} {}/main", tmux_name, icon, svc_name, alias));
            }
        }

        // Worktree workspace services
        let mut seen_branches: std::collections::HashSet<String> = std::collections::HashSet::new();
        for wt in self.worktrees.values() {
            let ws_branch = workspace_branch(wt).unwrap_or_else(|| wt.branch.clone());
            let key = format!("{}~{}", wt.parent_dir, ws_branch);
            if !seen_branches.insert(key) { continue; }
            if let Some(dir_cfg) = self.config.repos.get(&wt.parent_dir) {
                let alias = dir_cfg.alias.as_deref().unwrap_or(&wt.parent_dir);
                for svc_name in dir_cfg.services.keys() {
                    let tmux_name = self.wt_tmux_name(&wt.parent_dir, svc_name, &ws_branch);
                    let icon = if self.is_running(&tmux_name) { "●" } else { "○" };
                    items.push(format!("{}\t{} {:<15} {}/{}", tmux_name, icon, svc_name, alias, ws_branch));
                }
            }
        }

        if items.is_empty() {
            self.set_message("no services found");
            return;
        }

        let input = items.join("\n");
        let cmd = format!(
            "printf '{}' | fzf --prompt='> ' --with-nth=2.. --delimiter='\t' --ansi | cut -f1 > {}",
            input.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("60%", "50%", &cmd);
        self.pending_popup = Some(PendingPopup::Spotlight);
    }

    /// Handle spotlight result: find service by tmux_name, uncollapse, cursor focus, start if needed, swap.
    fn handle_spotlight_result(&mut self, tmux_name: &str) {
        // Parse tmux_name to determine context: alias~svc or alias~svc~branch
        let parts: Vec<&str> = tmux_name.splitn(3, '~').collect();
        let (dir_name, _svc_name, ws_branch) = if parts.len() == 3 {
            // Non-main: alias~svc~branch_safe
            let alias = parts[0];
            let dir = self.config.repos.iter()
                .find(|(_, d)| d.alias.as_deref() == Some(alias))
                .map(|(k, _)| k.clone())
                .unwrap_or_else(|| alias.to_string());
            (dir, parts[1].to_string(), Some(parts[2].replace('-', "-")))
        } else if parts.len() == 2 {
            // Main: alias~svc
            let alias = parts[0];
            let dir = self.config.repos.iter()
                .find(|(_, d)| d.alias.as_deref() == Some(alias))
                .map(|(k, _)| k.clone())
                .unwrap_or_else(|| alias.to_string());
            (dir, parts[1].to_string(), None)
        } else { return; };

        let is_main = ws_branch.is_none();

        // Uncollapse: instance + dir
        if is_main {
            for combo_name in &self.combos {
                let key = format!("ws-inst-main-{combo_name}");
                self.combo_collapsed.insert(key, false);
                let dir_key = format!("ws-dir-main-{combo_name}-{dir_name}");
                self.combo_collapsed.insert(dir_key, false);
            }
        } else if let Some(ref branch) = ws_branch {
            // Find workspace branch from worktrees
            for wt in self.worktrees.values() {
                if wt.parent_dir == dir_name {
                    if let Some(wb) = workspace_branch(wt) {
                        let key = format!("ws-inst-{wb}");
                        self.combo_collapsed.insert(key, false);
                        let dir_key = format!("ws-dir-{wb}-{dir_name}");
                        self.combo_collapsed.insert(dir_key, false);
                        break;
                    }
                }
            }
            let _ = branch;
        }
        self.rebuild_combo_tree();

        // Find service in combo_items and set cursor
        for (idx, item) in self.combo_items.iter().enumerate() {
            let matches = match item {
                ComboItem::InstanceService { tmux_name: t, .. } => t == tmux_name,
                ComboItem::InstanceDir { dir, branch, is_main: m, .. } => {
                    // Single-service dir
                    let svc_count = self.config.repos.get(dir).map(|d| d.services.len()).unwrap_or(0);
                    if svc_count == 1 {
                        if let Some(dc) = self.config.repos.get(dir) {
                            if let Some(sn) = dc.services.keys().next() {
                                let tn = if *m {
                                    let alias = dc.alias.as_deref().unwrap_or(dir);
                                    format!("{alias}~{sn}")
                                } else {
                                    self.wt_tmux_name(dir, sn, branch)
                                };
                                tn == tmux_name
                            } else { false }
                        } else { false }
                    } else { false }
                }
                _ => false,
            };
            if matches {
                self.cursor = idx;
                break;
            }
        }

        // Start if not running
        if !self.is_running(tmux_name) {
            self.do_start();
        }

        self.swap_pending = true;
    }

    /// Git menu popup: context-sensitive options.
    pub fn popup_git_menu(&mut self) {
        match self.current_combo_item().cloned() {
            // Instance level (main or non-main) → pull all repos
            Some(ComboItem::Instance { branch, is_main }) => {
                let label = if is_main { "main" } else { &branch };
                self.popup_menu(&format!("Git ({label})"), &[
                    "pull all repos",
                ], PendingPopup::GitPullAll { branch: branch.clone(), is_main });
            }
            // Dir/Service level
            Some(ComboItem::InstanceDir { dir, wt_key, is_main, .. }) |
            Some(ComboItem::InstanceService { dir, wt_key, is_main, .. }) => {
                let path = if is_main {
                    self.dir_path(&dir)
                } else {
                    self.worktrees.get(&wt_key).map(|wt| wt.path.to_string_lossy().into_owned())
                        .or_else(|| self.dir_path(&dir))
                };
                let Some(path) = path else { self.set_message("dir not found"); return; };

                if is_main {
                    self.popup_menu("Git (main)", &[
                        "pull origin",
                        "diff view",
                    ], PendingPopup::GitMenu { dir, path });
                } else {
                    self.popup_menu("Git", &[
                        "checkout branch",
                        "pull origin",
                        "diff view",
                    ], PendingPopup::GitMenu { dir, path });
                }
            }
            _ => { self.set_message("select a dir first"); }
        }
    }

    /// Shared services info popup.
    pub fn popup_shared_info(&mut self) {
        if self.config.shared_services.is_empty() {
            self.set_message("no shared services configured");
            return;
        }
        let session = &self.session;
        let project = format!("{session}-shared");
        let mut lines = Vec::new();
        lines.push(format!("  Shared Services ({})", project));
        lines.push(String::new());
        for (name, svc) in &self.config.shared_services {
            let host = svc.host.as_deref().unwrap_or("-");
            let ports: String = svc.ports.iter()
                .map(|p| p.split(':').next().unwrap_or(p).to_string())
                .collect::<Vec<_>>().join(", ");
            let cap = svc.capacity.map(|c| format!(" (cap:{c})")).unwrap_or_default();
            lines.push(format!("  {name:<16} {host:<22} :{ports}{cap}"));
        }
        let content = lines.join("\n");
        let cmd = format!(
            "echo '{}' | less -R --prompt='Shared Services (q to close)'",
            content.replace('\'', "'\\''")
        );
        tmux::display_popup("60%", "50%", &cmd);
    }

    /// Launch a branch picker popup using fzf inside tmux display-popup.
    pub fn popup_branch_picker(&mut self, dir_name: &str, checkout_mode: bool) {
        let dir_path = if checkout_mode {
            self.selected_work_dir(dir_name).unwrap_or_else(|| self.dir_path(dir_name).unwrap_or_default())
        } else {
            self.dir_path(dir_name).unwrap_or_default()
        };

        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let cmd = format!(
            "git -C '{}' branch -a --sort=-committerdate | sed 's/^[* ]*//' | sed 's|remotes/origin/||' | sort -u | fzf --prompt='Branch> ' > {}",
            dir_path, POPUP_RESULT_FILE
        );
        tmux::display_popup("70%", "60%", &cmd);
        self.pending_popup = Some(PendingPopup::BranchPicker { dir: dir_name.to_string(), checkout_mode });
    }

    /// Launch shortcuts picker popup using fzf.
    pub fn popup_shortcuts(&mut self) {
        let item = match self.current_combo_item().cloned() {
            Some(i) => i,
            None => { self.set_message("no shortcuts for this item"); return; }
        };
        let (items, title) = match item {
            ComboItem::InstanceDir { ref dir, .. } => {
                let dir_obj = match self.config.repos.get(dir) {
                    Some(d) => d,
                    None => return,
                };
                if dir_obj.shortcuts.is_empty() {
                    self.set_message(&format!("no shortcuts for dir '{dir}'"));
                    return;
                }
                (dir_obj.shortcuts.clone(), dir.clone())
            }
            ComboItem::InstanceService { ref dir, ref svc, .. } => {
                let dir_obj = match self.config.repos.get(dir) {
                    Some(d) => d,
                    None => return,
                };
                let svc_obj = match dir_obj.services.get(svc) {
                    Some(s) => s,
                    None => return,
                };
                let mut merged = dir_obj.shortcuts.clone();
                merged.extend(svc_obj.shortcuts.clone());
                if merged.is_empty() {
                    self.set_message("no shortcuts");
                    return;
                }
                (merged, format!("{dir}/{svc}"))
            }
            _ => { self.set_message("no shortcuts for this item"); return; }
        };

        // Store shortcuts for result handling
        self.shortcuts_items = items.clone();
        self.shortcuts_title = title;

        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let lines: Vec<String> = items.iter().enumerate()
            .map(|(i, s)| format!("{}\t{} -> {}", i, s.desc, s.cmd))
            .collect();
        let input = lines.join("\n");
        let cmd = format!(
            "echo '{}' | fzf --prompt='Shortcut> ' --with-nth=2.. --delimiter='\t' | cut -f1 > {}",
            input.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("70%", "50%", &cmd);
        self.pending_popup = Some(PendingPopup::Shortcut);
    }

    /// Show cheatsheet in tmux popup.
    pub fn popup_cheatsheet(&mut self) {
        let content = r#"
  Left Panel
  j/k          Navigate up/down
  Enter        Toggle start/stop or collapse
  Space        Spotlight (find any service)
  s            Start service/instance
  x            Stop service/instance
  X            Stop all (confirm)
  r            Restart
  c            Shortcuts popup
  e            Open in editor
  g            Git: checkout/pull/diff (main: pull+diff only)
  w            Create workspace / worktree menu
  d            Delete workspace (confirm)
  t            Shell in popup
  I            Shared services info
  R            Reload config
  Tab/l        Focus service pane
  n/N          Cycle running services

  Global
  ?            This cheat-sheet
  q            Quit
"#;
        let cmd = format!(
            "echo '{}' | less -R --prompt='Keybindings (q to close)'",
            content.replace('\'', "'\\''")
        );
        tmux::display_popup("50%", "70%", &cmd);
    }

    /// Run a shortcut command directly in tmux popup.
    /// Output piped through less for scrolling. q to close.
    pub fn run_shortcut_in_popup(&mut self, cmd: &str, desc: &str, dir: &str) {
        // Pipe command output directly through less (handles scrolling natively)
        let log = "/tmp/tncli-shortcut-output.log";
        let script = format!(
            "#!/bin/zsh\nLOG='{}'\ncd '{}'\n({}) 2>&1 | tee \"$LOG\"\nless -R --mouse +G \"$LOG\"\nrm -f \"$LOG\"\n",
            log, dir, cmd
        );
        let script_path = "/tmp/tncli-shortcut-run.sh";
        let _ = std::fs::write(script_path, &script);
        let _ = std::process::Command::new("chmod").args(["+x", script_path]).output();
        tmux::display_popup("80%", "80%", script_path);
        self.set_message(&format!("running: {desc}"));
    }

    /// Re-launch a popup (used when returning from a sub-popup via ESC).
    fn relaunch_popup(&mut self, popup: PendingPopup) {
        match popup {
            PendingPopup::GitMenu { dir, path } => {
                // Can't know is_main here — show full menu, checkout will be filtered by git_menu handler
                self.popup_menu("Git", &["checkout branch", "pull origin", "diff view"],
                    PendingPopup::GitMenu { dir, path });
            }
            PendingPopup::WsEdit { branch } => {
                self.popup_menu("Workspace", &["Create new workspace", "Add repo", "Remove repo"],
                    PendingPopup::WsEdit { branch });
            }
            _ => {} // Other popups don't need relaunch
        }
    }

    /// Poll for popup result. Called on each tick.
    pub fn poll_popup_result(&mut self) {
        let popup = match self.pending_popup.take() {
            Some(p) => p,
            None => return,
        };

        let result = match std::fs::read_to_string(POPUP_RESULT_FILE) {
            Ok(s) => {
                let _ = std::fs::remove_file(POPUP_RESULT_FILE);
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            }
            Err(_) => {
                // File not ready yet — put popup back
                self.pending_popup = Some(popup);
                return;
            }
        };

        // ESC pressed (empty result) — return to parent popup if exists
        if result.is_none() {
            if let Some(parent) = self.popup_stack.pop() {
                self.relaunch_popup(parent);
            }
            return;
        }

        // Clear stack on successful result (don't return to parent)
        self.popup_stack.clear();

        match popup {
            PendingPopup::BranchPicker { dir, checkout_mode } => {
                if let Some(branch) = result {
                    if checkout_mode {
                        let msg = self.git_checkout(&dir, &branch);
                        self.set_message(&msg);
                    } else if self.ws_creating {
                        self.ws_creating = false;
                        self.build_ws_select(&branch);
                    } else {
                        let msg = self.create_worktree(&dir, &branch);
                        self.set_message(&msg);
                    }
                }
            }
            PendingPopup::Shortcut => {
                if let Some(idx_str) = result {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        self.shortcuts_cursor = idx;
                        if let Some((cmd, desc, dir)) = self.selected_shortcut() {
                            self.run_shortcut_in_popup(&cmd, &desc, &dir);
                        }
                    }
                }
            }
            PendingPopup::GitPullAll { branch, is_main } => {
                if result.as_deref() == Some("pull all repos") {
                    let mut script = String::from("#!/bin/zsh\n");
                    let dirs: Vec<(String, String)> = if is_main {
                        self.dir_names.iter().filter_map(|d| {
                            let path = self.dir_path(d)?;
                            let b = self.config.default_branch_for(d);
                            Some((d.clone(), format!("cd '{}' && git pull origin {}", path, b)))
                        }).collect()
                    } else {
                        self.worktrees.values()
                            .filter(|wt| workspace_branch(wt).as_deref() == Some(&branch))
                            .map(|wt| {
                                let path = wt.path.to_string_lossy();
                                // Pull current branch of each repo (may differ from workspace branch)
                                (wt.parent_dir.clone(), format!(
                                    "cd '{}' && git pull origin \"$(git rev-parse --abbrev-ref HEAD)\"", path
                                ))
                            }).collect()
                    };
                    // Run pulls in parallel, print each repo result as it finishes
                    for (i, (name, cmd)) in dirs.iter().enumerate() {
                        script.push_str(&format!(
                            "( {} > /tmp/tncli-pull-{i}.log 2>&1 && echo '\\033[32m✓ {name}\\033[0m' || echo '\\033[31m✗ {name}\\033[0m'; cat /tmp/tncli-pull-{i}.log; rm -f /tmp/tncli-pull-{i}.log; echo ) &\n",
                            cmd
                        ));
                    }
                    script.push_str("wait\necho '\\033[32m[Done]\\033[0m'\n");
                    let script_path = "/tmp/tncli-pull-all.sh";
                    let _ = std::fs::write(script_path, &script);
                    let _ = std::process::Command::new("chmod").args(["+x", script_path]).output();
                    let log = "/tmp/tncli-pull-all.log";
                    let run = format!("{} 2>&1 | tee '{}'; less -R --mouse +G '{}'; rm -f '{}' '{}'",
                        script_path, log, log, log, script_path);
                    tmux::display_popup("80%", "80%", &run);
                }
            }
            PendingPopup::GitMenu { dir, path } => {
                if let Some(choice) = result {
                    match choice.as_str() {
                        "checkout branch" => {
                            self.popup_stack.push(PendingPopup::GitMenu { dir: dir.clone(), path: path.clone() });
                            self.popup_branch_picker(&dir, true);
                        }
                        "pull origin" => {
                            let branch = self.dir_branch(&dir).unwrap_or_else(|| "main".to_string());
                            // Run pull in popup so user sees output
                            let cmd = format!("git -C '{}' pull origin {}", path, branch);
                            tmux::display_popup("70%", "50%",
                                &format!("({}) 2>&1 | less -R --mouse +G", cmd));
                        }
                        "diff view" => {
                            let cmd = format!(
                                "cd '{}' && git diff --color=always | less -R --mouse",
                                path
                            );
                            tmux::display_popup("90%", "90%", &cmd);
                        }
                        _ => {}
                    }
                }
            }
            PendingPopup::WsEdit { branch } => {
                if let Some(choice) = result {
                    match choice.as_str() {
                        "Create new workspace" => {
                            self.ws_creating = true;
                            self.popup_input("Workspace branch name:",
                                PendingPopup::NameInput { context: "workspace".to_string() });
                        }
                        "Add repo" => {
                            self.popup_stack.push(PendingPopup::WsEdit { branch: branch.clone() });
                            self.build_ws_add_list(&branch);
                        }
                        "Remove repo" => {
                            self.popup_stack.push(PendingPopup::WsEdit { branch: branch.clone() });
                            self.build_ws_remove_list(&branch);
                        }
                        _ => {}
                    }
                }
            }
            PendingPopup::WsAdd { branch } => {
                if let Some(dir_name) = result {
                    self.add_repo_to_workspace(&dir_name, &branch, &branch);
                }
            }
            PendingPopup::WsRemove => {
                if let Some(wt_key) = result {
                    let msg = self.delete_worktree(&wt_key);
                    self.set_message(&msg);
                }
            }
            PendingPopup::WsRepoSelect { ws_name, ws_branch } => {
                if let Some(selected_text) = result {
                    // Filter ws_select_items to only selected repos
                    let selected_dirs: Vec<String> = selected_text.lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect();
                    self.ws_select_items.retain(|i| selected_dirs.contains(&i.dir_name));
                    if self.ws_select_items.is_empty() {
                        self.set_message("no repos selected");
                        return;
                    }
                    self.ws_name = ws_name;
                    if let Some(tx) = self.event_tx.clone() {
                        let msg = self.start_create_pipeline(&self.ws_name.clone(), &ws_branch, tx);
                        self.set_message(&msg);
                    }
                }
            }
            PendingPopup::NameInput { context } => {
                if let Some(name) = result {
                    if name.is_empty() { return; }
                    if context.starts_with("branch:") {
                        let dir = context.strip_prefix("branch:").unwrap_or("");
                        let msg = self.create_branch_and_checkout(dir, &name);
                        self.set_message(&msg);
                    } else if context == "workspace" {
                        if let Some(_tx) = self.event_tx.clone() {
                            self.build_ws_select(&name);
                        }
                    }
                }
            }
            PendingPopup::Confirm { action } => {
                if let Some(answer) = result {
                    if answer.trim().eq_ignore_ascii_case("y") {
                        self.confirm_action = action;
                        self.execute_confirm();
                    } else {
                        self.set_message("cancelled");
                    }
                }
            }
            PendingPopup::Spotlight => {
                if let Some(tmux_name) = result {
                    let name = tmux_name.trim().to_string();
                    self.handle_spotlight_result(&name);
                }
            }
        }
    }
}

// ── Collapse state persistence ──

fn collapse_state_path(session: &str) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    std::path::PathBuf::from(home).join(format!(".tncli/collapse-{session}.json"))
}

fn load_collapse_state(
    session: &str,
    _dir_names: &[String],
) -> (Vec<bool>, std::collections::HashMap<String, bool>, std::collections::HashMap<String, bool>) {
    let path = collapse_state_path(session);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (Vec::new(), Default::default(), Default::default()),
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), Default::default(), Default::default()),
    };

    let mut wt_collapsed: std::collections::HashMap<String, bool> = Default::default();
    if let Some(wt) = json.get("wt").and_then(|v| v.as_object()) {
        for (k, v) in wt {
            if let Some(b) = v.as_bool() { wt_collapsed.insert(k.clone(), b); }
        }
    }

    let mut combo_collapsed: std::collections::HashMap<String, bool> = Default::default();
    if let Some(cb) = json.get("combo").and_then(|v| v.as_object()) {
        for (k, v) in cb {
            if let Some(b) = v.as_bool() { combo_collapsed.insert(k.clone(), b); }
        }
    }

    (Vec::new(), wt_collapsed, combo_collapsed)
}

pub(crate) fn save_collapse_state(
    session: &str,
    _dir_names: &[String],
    wt_collapsed: &std::collections::HashMap<String, bool>,
    combo_collapsed: &std::collections::HashMap<String, bool>,
) {
    let wt: serde_json::Map<String, serde_json::Value> = wt_collapsed.iter()
        .filter(|(_, v)| **v)
        .map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v)))
        .collect();
    let combo: serde_json::Map<String, serde_json::Value> = combo_collapsed.iter()
        .filter(|(_, v)| **v)
        .map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v)))
        .collect();

    let json = serde_json::json!({ "wt": wt, "combo": combo });
    let path = collapse_state_path(session);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap_or_default());
}
