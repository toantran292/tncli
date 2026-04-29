use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use ratatui::text::Line;

use crate::config::{Config, Shortcut};
use crate::tmux;


/// Strip ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\x1b' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Left,
    Right,
}

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
    pub focus: Focus,
    pub log_scroll: usize,
    pub combo_log_idx: usize,
    pub running_windows: HashSet<String>,
    pub stopping_services: HashSet<String>,
    pub starting_services: HashSet<String>,
    pub message: String,
    pub message_time: Option<Instant>,
    // log cache
    pub log_cache: Vec<String>,
    pub log_cache_svc: Option<String>,
    pub log_dirty: bool,
    pub last_log_size: (u16, u16),
    pub(crate) parsed_lines: Vec<Line<'static>>,
    pub(crate) parsed_dirty: bool,
    pub(crate) parsed_query: String,
    pub(crate) parsed_current_match: Option<usize>,
    pub(crate) parsed_start: usize,
    pub(crate) parsed_end: usize,
    pub stripped_line_count: usize,
    // modes
    pub copy_mode: bool,
    pub interactive_mode: bool,
    pub search_mode: bool,
    pub search_query: String,
    pub search_matches: Vec<(usize, usize)>,
    pub search_current: usize,
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
    pub wt_branch_dir: String,
    pub branch_checkout_mode: bool, // true = checkout, false = create worktree
    // branch name input (for single worktree or workspace)
    pub wt_name_input_open: bool,
    pub wt_name_input: String,
    pub wt_name_base_branch: String,
    // workspace creation
    pub ws_creating: bool,
    pub ws_name: String,  // workspace name (from combos section)
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
}

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
    DeleteWorktree { wt_key: String },
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

        let mut app = Self {
            config_path,
            config,
            session,
            dir_names,
            worktrees: std::collections::HashMap::new(),
            deleting_workspaces: HashSet::new(),
            creating_workspaces: HashSet::new(),
            wt_collapsed,
            combos,
            combo_items: Vec::new(),
            combo_collapsed,
            cursor: 0,
            focus: Focus::Left,
            log_scroll: 0,
            combo_log_idx: 0,
            running_windows: HashSet::new(),
            stopping_services: HashSet::new(),
            starting_services: HashSet::new(),
            message: String::new(),
            message_time: None,
            log_cache: Vec::new(),
            log_cache_svc: None,
            log_dirty: true,
            last_log_size: (0, 0),
            parsed_lines: Vec::new(),
            parsed_dirty: true,
            parsed_query: String::new(),
            parsed_current_match: None,
            parsed_start: 0,
            parsed_end: 0,
            stripped_line_count: 0,
            copy_mode: false,
            interactive_mode: false,
            search_mode: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current: 0,
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
            wt_branch_dir: String::new(),
            branch_checkout_mode: false,
            wt_name_input_open: false,
            wt_name_input: String::new(),
            wt_name_base_branch: String::new(),
            ws_creating: false,
            ws_name: String::new(),
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
        };
        app.scan_worktrees(); // also calls rebuild_combo_tree
        Ok(app)
    }


    /// Open branch picker for creating worktree.
    pub fn open_branch_picker(&mut self) {
        self.set_message("loading branches...");
        let dir_path = match self.dir_path(&self.wt_menu_dir) {
            Some(p) => p,
            None => return,
        };
        match crate::services::list_branches(std::path::Path::new(&dir_path)) {
            Ok(branches) => {
                if branches.is_empty() {
                    self.set_message("no branches found");
                    return;
                }
                self.wt_branches = branches.clone();
                self.wt_branch_filtered = branches;
                self.wt_branch_search.clear();
                self.wt_branch_searching = false;
                self.wt_branch_cursor = 0;
                self.wt_branch_dir = self.wt_menu_dir.clone();
                self.branch_checkout_mode = false;
                self.wt_branch_open = true;
                self.wt_menu_open = false;
            }
            Err(e) => self.set_message(&format!("git error: {e}")),
        }
    }

    /// Filter branches by search query.
    pub fn filter_branches(&mut self) {
        let query = self.wt_branch_search.to_lowercase();
        if query.is_empty() {
            self.wt_branch_filtered = self.wt_branches.clone();
        } else {
            self.wt_branch_filtered = self.wt_branches.iter()
                .filter(|b| b.to_lowercase().contains(&query))
                .cloned()
                .collect();
        }
        self.wt_branch_cursor = 0;
    }

    /// Open branch menu for current dir (checkout/create/fetch).
    /// Show confirm dialog for risky actions.
    pub fn ask_confirm(&mut self, msg: &str, action: ConfirmAction) {
        self.confirm_msg = msg.to_string();
        self.confirm_action = action;
        self.confirm_open = true;
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
            ConfirmAction::DeleteWorktree { wt_key } => {
                let msg = self.delete_worktree(&wt_key);
                self.set_message(&msg);
            }
            ConfirmAction::StopAll => {
                self.do_stop_all();
            }
            ConfirmAction::None => {}
        }
    }

    pub fn open_branch_menu(&mut self) {
        match self.current_combo_item().cloned() {
            Some(ComboItem::InstanceDir { dir, is_main, .. }) |
            Some(ComboItem::InstanceService { dir, is_main, .. }) => {
                if is_main {
                    // Main worktree: checkout default branch + pull (background)
                    let default_branch = self.config.default_branch_for(&dir);
                    let dir_path = self.selected_work_dir(&dir)
                        .or_else(|| self.dir_path(&dir))
                        .unwrap_or_default();
                    let tx = self.event_tx.clone();
                    let dir_name = dir.clone();
                    self.set_message(&format!("pulling {dir}..."));
                    std::thread::spawn(move || {
                        let msg = git_checkout_and_pull_sync(&dir_path, &dir_name, &default_branch);
                        if let Some(tx) = tx {
                            let _ = tx.send(crate::tui::event::AppEvent::Message(msg));
                        }
                    });
                } else {
                    self.branch_menu_dir = dir;
                    self.branch_menu_cursor = 0;
                    self.branch_menu_open = true;
                }
            }
            Some(ComboItem::Instance { branch, is_main }) => {
                if is_main {
                    // Pull default branch for all dirs
                    let default_branch = self.config.global_default_branch().to_string();
                    self.pull_workspace_dirs_branch(&default_branch, true);
                } else {
                    // Pull current branch for all dirs in this worktree
                    self.pull_workspace_dirs_branch(&branch, false);
                }
            }
            _ => { self.set_message("select a dir or workspace first"); }
        }
    }

    /// Pull a specific branch in a dir (fetch + merge).
    pub fn git_pull_branch(&mut self, dir_name: &str, branch: &str) -> String {
        let dir_path = self.selected_work_dir(dir_name)
            .or_else(|| self.dir_path(dir_name))
            .unwrap_or_default();
        let output = std::process::Command::new("git")
            .args(["-C", &dir_path, "pull", "origin", branch])
            .output();
        match output {
            Ok(o) if o.status.success() => format!("pulled origin/{branch} in {dir_name}"),
            Ok(o) => format!("pull failed: {}", String::from_utf8_lossy(&o.stderr).trim()),
            Err(e) => format!("git error: {e}"),
        }
    }

    /// Pull branch for all dirs in a workspace instance (background thread).
    /// For main: checkout per-repo default branch first, then pull.
    fn pull_workspace_dirs_branch(&mut self, branch: &str, is_main: bool) {
        let dirs: Vec<String> = if is_main {
            self.dir_names.clone()
        } else {
            self.worktrees.values()
                .filter(|wt| crate::tui::app::workspace_branch(wt).as_deref() == Some(branch))
                .map(|wt| wt.parent_dir.clone())
                .collect()
        };
        if dirs.is_empty() {
            self.set_message("no dirs to pull");
            return;
        }
        let count = dirs.len();
        if is_main {
            self.set_message(&format!("checkout + pulling default branches in {count} dirs..."));
        } else {
            self.set_message(&format!("pulling origin/{branch} in {count} dirs..."));
        }

        // Collect (dir_name, path, target_branch) — per-repo default for main
        let dir_info: Vec<(String, String, String)> = dirs.iter().filter_map(|d| {
            let path = if is_main {
                self.dir_path(d)
            } else {
                let wt = self.worktrees.values()
                    .find(|wt| wt.parent_dir == *d && workspace_branch(wt).as_deref() == Some(branch));
                wt.map(|w| w.path.to_string_lossy().into_owned())
            };
            let target = if is_main {
                self.config.default_branch_for(d)
            } else {
                branch.to_string()
            };
            path.map(|p| (d.clone(), p, target))
        }).collect();

        let do_checkout = is_main;
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let mut ok = 0;
            let mut fail = 0;
            for (_dir_name, dir_path, target_branch) in &dir_info {
                // Checkout default branch first (for main only)
                if do_checkout {
                    let co = std::process::Command::new("git")
                        .args(["-C", dir_path, "checkout", target_branch])
                        .output();
                    if co.is_ok_and(|o| !o.status.success()) {
                        fail += 1;
                        continue;
                    }
                }
                let result = std::process::Command::new("git")
                    .args(["-C", dir_path, "pull", "origin", target_branch])
                    .output();
                match result {
                    Ok(o) if o.status.success() => ok += 1,
                    _ => fail += 1,
                }
            }
            let msg = if fail == 0 {
                format!("pulled {ok} dirs")
            } else {
                format!("pulled: {ok} ok, {fail} failed")
            };
            if let Some(tx) = tx {
                let _ = tx.send(crate::tui::event::AppEvent::Message(msg));
            }
        });
    }

    /// Open branch picker for checkout (reuses existing branch picker).
    pub fn open_checkout_picker(&mut self) {
        let dir_name = self.branch_menu_dir.clone();
        self.branch_menu_open = false;
        let dir_path = match self.dir_path(&dir_name) {
            Some(p) => p,
            None => { self.set_message("dir not found"); return; }
        };
        let actual_path = self.selected_work_dir(&dir_name).unwrap_or(dir_path);
        self.set_message("loading branches...");
        match crate::services::list_branches(std::path::Path::new(&actual_path)) {
            Ok(branches) => {
                if branches.is_empty() {
                    self.set_message("no branches found");
                    return;
                }
                self.wt_branches = branches.clone();
                self.wt_branch_filtered = branches;
                self.wt_branch_search.clear();
                self.wt_branch_searching = false;
                self.wt_branch_cursor = 0;
                self.wt_branch_dir = dir_name;
                self.branch_checkout_mode = true;
                self.wt_branch_open = true;
            }
            Err(e) => self.set_message(&format!("git error: {e}")),
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
                let branch_safe = crate::services::branch_safe(&branch);
                let ws_key = format!("ws-{}", branch.replace('/', "-"));
                let resolved = crate::services::resolve_env_templates(&wt_cfg.env, "127.0.0.1", &branch_safe, &branch, &ws_key);
                let env_file = wt_cfg.env_file.as_deref().unwrap_or(".env.local");
                crate::services::apply_env_overrides(p, &resolved, env_file);
                let _ = crate::services::write_env_file(p, "127.0.0.1");

                // Compose override for main
                let (svc_overrides, shared_hosts) = crate::pipeline::context::resolve_shared_overrides(&self.config, dir_name);
                let compose_files = if wt_cfg.compose_files.is_empty() && p.join("docker-compose.yml").is_file() {
                    vec!["docker-compose.yml".to_string()]
                } else {
                    wt_cfg.compose_files.clone()
                };
                if !compose_files.is_empty() {
                    crate::services::generate_compose_override(
                        p, p, "127.0.0.1", &compose_files, &wt_cfg.env, &branch, None,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts, &ws_key,
                    );
                }
            }

            // Worktrees
            for (_, wt) in &self.worktrees {
                if wt.parent_dir != *dir_name { continue; }
                let branch_safe = crate::services::branch_safe(&wt.branch);
                let ws_key = format!("ws-{}", wt.branch.replace('/', "-"));
                let resolved = crate::services::resolve_env_templates(&wt_cfg.env, &wt.bind_ip, &branch_safe, &wt.branch, &ws_key);
                let env_file = wt_cfg.env_file.as_deref().unwrap_or(".env.local");
                crate::services::apply_env_overrides(&wt.path, &resolved, env_file);
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
                        &compose_files, &wt_cfg.env, &wt.branch, None,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts, &ws_key,
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
        if tmux::session_exists(&self.session) {
            self.running_windows = tmux::list_windows(&self.session);
        } else {
            self.running_windows.clear();
        }
        // Clean up stopping services that are no longer running
        self.stopping_services.retain(|svc| self.running_windows.contains(svc));
        // Clean up starting services that are now running (or failed to start)
        self.starting_services.retain(|svc| !self.running_windows.contains(svc));
        // Clean up dead setup windows left from interrupted pipelines
        let session = self.session.clone();
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
                    // Skip worktrees whose path no longer exists on disk
                    if !wt_path.exists() {
                        continue;
                    }
                    let wt_key = format!("{dir_name}--{}", branch.replace('/', "-"));
                    let ip = allocs.get(&wt_key)
                        .or_else(|| allocs.get(&format!("ws-{}", branch.replace('/', "-"))))
                        .cloned()
                        .unwrap_or_default();
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

    pub fn invalidate_log(&mut self) {
        self.log_dirty = true;
        self.parsed_dirty = true;
    }

    /// Current item under cursor in the services tree.
    /// Current item under cursor in the workspaces tree.
    pub fn current_combo_item(&self) -> Option<&ComboItem> {
        self.combo_items.get(self.cursor)
    }

    /// Get the service name for the current selection (tree or combo).
    pub fn selected_service_name(&self) -> Option<String> {
        self.log_service_name()
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

    /// Open shortcuts popup. Merges dir + service shortcuts.
    pub fn open_shortcuts(&mut self) {
        let item = match self.current_combo_item().cloned() {
            Some(i) => i,
            None => { self.set_message("no shortcuts for this item"); return; }
        };
        match item {
            ComboItem::InstanceDir { ref dir, .. } => {
                let dir_obj = match self.config.repos.get(dir) {
                    Some(d) => d,
                    None => return,
                };
                if dir_obj.shortcuts.is_empty() {
                    self.set_message(&format!("no shortcuts for dir '{dir}'"));
                    return;
                }
                self.shortcuts_items = dir_obj.shortcuts.clone();
                self.shortcuts_title = dir.clone();
                self.shortcuts_cursor = 0;
                self.shortcuts_open = true;
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
                    self.set_message(&format!("no shortcuts for '{svc}' -- add shortcuts: in tncli.yml"));
                    return;
                }
                self.shortcuts_items = merged;
                self.shortcuts_title = format!("{dir}/{svc}");
                self.shortcuts_cursor = 0;
                self.shortcuts_open = true;
            }
            _ => { self.set_message("no shortcuts for this item"); }
        }
    }

    /// Get the selected shortcut's cmd, desc, and working dir.
    pub fn selected_shortcut(&self) -> Option<(String, String, String)> {
        let shortcut = self.shortcuts_items.get(self.shortcuts_cursor)?;
        let dir_name = self.selected_dir_name()?;

        // Use worktree path if in workspace/worktree context
        let dir_path = self.selected_work_dir(&dir_name)
            .unwrap_or_else(|| self.dir_path(&dir_name).unwrap_or_default());

        Some((shortcut.cmd.clone(), shortcut.desc.clone(), dir_path))
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

    pub fn shortcuts_count(&self) -> usize {
        self.shortcuts_items.len()
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

    pub(crate) fn run_tncli_cmd(&self, args: &[&str]) -> bool {
        let exe = std::env::current_exe().unwrap_or_default();
        std::process::Command::new(exe)
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// Get the CLI target string for current selection.
    /// Services use "dir/svc" format to avoid ambiguity with dir aliases.
    pub(crate) fn current_target(&self) -> Option<String> {
        match self.current_combo_item()? {
            ComboItem::Combo(name) => Some(name.clone()),
            ComboItem::InstanceService { tmux_name, .. } => Some(tmux_name.clone()),
            _ => None,
        }
    }

    /// Check if a branch is already used by a worktree for this dir.
    pub fn has_branch_conflict(&self, dir_name: &str, branch: &str) -> bool {
        self.worktrees.values().any(|wt| wt.parent_dir == dir_name && wt.branch == branch)
    }

    /// Build workspace selection checklist from combo dirs.
    pub fn build_ws_select(&mut self, ws_branch: &str) {
        let combo_name = &self.ws_name;
        let all_ws = self.config.all_workspaces();
        let entries = all_ws.get(combo_name).cloned().unwrap_or_default();

        let mut unique_dirs = Vec::new();
        for entry in &entries {
            if let Some((dir, _)) = self.config.find_service_entry_quiet(entry) {
                if !unique_dirs.contains(&dir) {
                    unique_dirs.push(dir);
                }
            }
        }

        self.ws_select_items = unique_dirs.iter().map(|dir_name| {
            let alias = self.config.repos.get(dir_name)
                .and_then(|d| d.alias.as_deref())
                .unwrap_or(dir_name)
                .to_string();
            let conflict = self.has_branch_conflict(dir_name, ws_branch);
            WsSelectItem {
                dir_name: dir_name.clone(),
                alias,
                selected: true,
                branch: ws_branch.to_string(),
                conflict,
            }
        }).collect();

        self.ws_select_branch = ws_branch.to_string();
        self.ws_select_cursor = 0;
        self.ws_select_open = true;
    }

    /// Update conflict flags for all ws_select items.
    pub fn update_ws_select_conflicts(&mut self) {
        // Collect conflict checks first to avoid borrow conflict
        let conflicts: Vec<bool> = self.ws_select_items.iter()
            .map(|item| self.has_branch_conflict(&item.dir_name, &item.branch))
            .collect();
        for (item, conflict) in self.ws_select_items.iter_mut().zip(conflicts) {
            item.conflict = conflict;
        }
    }

    /// Build add-repo list (repos not in this workspace branch).
    pub fn build_ws_add_list(&mut self, branch: &str) {
        let existing_dirs: Vec<String> = self.worktrees.values()
            .filter(|wt| workspace_branch(wt).as_deref() == Some(branch))
            .map(|wt| wt.parent_dir.clone())
            .collect();

        self.ws_add_items = self.dir_names.iter()
            .filter(|d| !existing_dirs.contains(d))
            .map(|dir_name| {
                let alias = self.config.repos.get(dir_name)
                    .and_then(|d| d.alias.as_deref())
                    .unwrap_or(dir_name)
                    .to_string();
                let conflict = self.has_branch_conflict(dir_name, branch);
                WsSelectItem {
                    dir_name: dir_name.clone(),
                    alias,
                    selected: true,
                    branch: branch.to_string(),
                    conflict,
                }
            })
            .collect();

        if self.ws_add_items.is_empty() {
            self.set_message("all repos already in workspace");
            return;
        }

        self.ws_edit_branch = branch.to_string();
        self.ws_add_cursor = 0;
        self.ws_add_open = true;
    }

    /// Build remove-repo list (repos in this workspace branch).
    pub fn build_ws_remove_list(&mut self, branch: &str) {
        self.ws_remove_items = self.worktrees.iter()
            .filter(|(_, wt)| workspace_branch(wt).as_deref() == Some(branch))
            .map(|(wt_key, wt)| (wt.parent_dir.clone(), wt_key.clone()))
            .collect();

        if self.ws_remove_items.is_empty() {
            self.set_message("no repos to remove");
            return;
        }

        self.ws_edit_branch = branch.to_string();
        self.ws_remove_cursor = 0;
        self.ws_remove_open = true;
    }
}

// ── Git helpers (free functions for background threads) ──

fn git_checkout_and_pull_sync(dir_path: &str, dir_name: &str, branch: &str) -> String {
    let co = std::process::Command::new("git")
        .args(["-C", dir_path, "checkout", branch])
        .output();
    if let Ok(o) = &co {
        if !o.status.success() {
            let stderr = String::from_utf8_lossy(&o.stderr);
            return format!("checkout {branch} failed in {dir_name}: {}", stderr.trim());
        }
    }
    let output = std::process::Command::new("git")
        .args(["-C", dir_path, "pull", "origin", branch])
        .output();
    match output {
        Ok(o) if o.status.success() => format!("pulled {branch} in {dir_name}"),
        Ok(o) => format!("pull failed in {dir_name}: {}", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => format!("git error in {dir_name}: {e}"),
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
