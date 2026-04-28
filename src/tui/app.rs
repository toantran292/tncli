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

/// Check if a worktree is inside a workspace folder.
#[allow(dead_code)]
fn is_workspace_worktree(wt: &crate::worktree::WorktreeInfo) -> bool {
    wt.path.parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n.to_string_lossy().starts_with("workspace--"))
}

/// Extract workspace branch name from worktree path (workspace--{branch}/dir_name).
pub(crate) fn workspace_branch(wt: &crate::worktree::WorktreeInfo) -> Option<String> {
    wt.path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_string_lossy().strip_prefix("workspace--").map(|s| s.to_string()))
}

/// Check if worktree belongs to a specific workspace branch (for use from ui.rs).
pub fn workspace_branch_eq(wt: &crate::worktree::WorktreeInfo, branch: &str) -> bool {
    workspace_branch(wt).as_deref() == Some(branch)
}

pub struct App {
    pub config_path: PathBuf,
    pub config: Config,
    pub session: String,
    // tree
    pub dir_names: Vec<String>,
    // worktrees
    pub worktrees: std::collections::HashMap<String, crate::worktree::WorktreeInfo>,
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
    // confirm dialog
    pub confirm_open: bool,
    pub confirm_msg: String,
    pub confirm_action: ConfirmAction,
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
            confirm_open: false,
            confirm_msg: String::new(),
            confirm_action: ConfirmAction::None,
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
        match crate::worktree::list_branches(std::path::Path::new(&dir_path)) {
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
                let (msg, _) = self.delete_workspace_by_name(&branch);
                self.set_message(&msg);
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
        let dir_name = match self.current_combo_item() {
            Some(ComboItem::InstanceDir { dir, .. }) => dir.clone(),
            Some(ComboItem::InstanceService { dir, .. }) => dir.clone(),
            _ => { self.set_message("select a dir first"); return; }
        };
        self.branch_menu_dir = dir_name;
        self.branch_menu_cursor = 0;
        self.branch_menu_open = true;
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
        match crate::worktree::list_branches(std::path::Path::new(&actual_path)) {
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

    /// Git fetch in a dir.
    pub fn git_fetch(&mut self, dir_name: &str) -> String {
        let dir_path = self.selected_work_dir(dir_name)
            .or_else(|| self.dir_path(dir_name))
            .unwrap_or_default();
        self.set_message(&format!("fetching {dir_name}..."));
        let output = std::process::Command::new("git")
            .args(["-C", &dir_path, "fetch", "--prune"])
            .output();
        match output {
            Ok(o) if o.status.success() => format!("fetched {dir_name}"),
            Ok(o) => format!("fetch failed: {}", String::from_utf8_lossy(&o.stderr).trim()),
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

                let svc_count: usize = self.config.repos.values().map(|d| d.services.len()).sum();
                format!(
                    "config reloaded -- {} dirs, {} services, {} combos (was {}/{})",
                    self.dir_names.len(), svc_count, self.combos.len(), old_dirs, old_combos
                )
            }
            Err(e) => format!("reload failed: {e}"),
        }
    }

    pub fn refresh_status(&mut self) {
        if tmux::session_exists(&self.session) {
            self.running_windows = tmux::list_windows(&self.session);
        } else {
            self.running_windows.clear();
        }
        // Clean up stopping services that are no longer running
        self.stopping_services.retain(|svc| self.running_windows.contains(svc));

        // Check if deleting workspaces have finished (folder gone)
        if !self.deleting_workspaces.is_empty() {
            let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
            let finished: Vec<String> = self.deleting_workspaces.iter()
                .filter(|branch| !crate::worktree::workspace_folder_path(config_dir, branch).exists())
                .cloned()
                .collect();
            if !finished.is_empty() {
                for branch in &finished {
                    self.deleting_workspaces.remove(branch);
                    // Remove worktrees from state
                    let suffix = format!("--{}", branch.replace('/', "-"));
                    let keys: Vec<String> = self.worktrees.keys()
                        .filter(|k| k.ends_with(&suffix))
                        .cloned()
                        .collect();
                    for k in keys {
                        self.worktrees.remove(&k);
                    }
                }
                self.rebuild_combo_tree();
                self.set_message(&format!("deleted: {}", finished.join(", ")));
            }
        }

        // Check if creating workspaces have finished (worktree dirs exist)
        if !self.creating_workspaces.is_empty() {
            let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
            let finished: Vec<String> = self.creating_workspaces.iter()
                .filter(|branch| {
                    let ws_folder = crate::worktree::workspace_folder_path(config_dir, branch);
                    // Check if at least one dir has docker-compose.override.yml (sign of completion)
                    ws_folder.exists() && std::fs::read_dir(&ws_folder)
                        .map(|entries| entries.flatten()
                            .any(|e| e.path().join("docker-compose.override.yml").exists()
                                || e.path().join(".env.tncli").exists()))
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            if !finished.is_empty() {
                for branch in &finished {
                    self.creating_workspaces.remove(branch);
                }
                self.scan_worktrees();
                self.set_message(&format!("workspace ready: {}", finished.join(", ")));
            }
        }
    }

    pub fn is_stopping(&self, svc: &str) -> bool {
        self.stopping_services.contains(svc)
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

    /// Get working directory for a dir_name, resolved relative to config dir.
    pub fn dir_path(&self, dir_name: &str) -> Option<String> {
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
        let p = std::path::Path::new(dir_name);
        if p.is_absolute() {
            Some(dir_name.to_string())
        } else {
            Some(config_dir.join(dir_name).to_string_lossy().into_owned())
        }
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
                    // Open config dir for main
                    Some(config_dir.to_string_lossy().into_owned())
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
