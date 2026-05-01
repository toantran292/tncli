use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use crate::config::{Config, Shortcut};

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
    pub main_bind_ip: String,
    pub dir_names: Vec<String>,
    pub worktrees: std::collections::HashMap<String, crate::services::WorktreeInfo>,
    pub deleting_workspaces: HashSet<String>,
    pub creating_workspaces: HashSet<String>,
    pub wt_collapsed: std::collections::HashMap<String, bool>,
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
    pub shortcuts_open: bool,
    pub shortcuts_cursor: usize,
    pub shortcuts_items: Vec<Shortcut>,
    pub shortcuts_title: String,
    pub wt_menu_open: bool,
    pub wt_menu_cursor: usize,
    pub wt_menu_dir: String,
    pub branch_menu_open: bool,
    pub branch_menu_cursor: usize,
    pub branch_menu_dir: String,
    pub wt_branch_open: bool,
    pub wt_branch_cursor: usize,
    pub wt_branches: Vec<String>,
    pub wt_branch_filtered: Vec<String>,
    pub wt_branch_search: String,
    pub wt_branch_searching: bool,
    pub wt_name_input_open: bool,
    pub wt_name_input: String,
    pub wt_name_base_branch: String,
    pub ws_creating: bool,
    pub ws_name: String,
    pub ws_source_branch: Option<String>,
    pub ws_select_open: bool,
    pub ws_select_cursor: usize,
    pub ws_select_branch: String,
    pub ws_select_items: Vec<WsSelectItem>,
    pub ws_edit_open: bool,
    pub ws_edit_cursor: usize,
    pub ws_edit_branch: String,
    pub ws_add_open: bool,
    pub ws_add_cursor: usize,
    pub ws_add_items: Vec<WsSelectItem>,
    pub ws_remove_open: bool,
    pub ws_remove_cursor: usize,
    pub ws_remove_items: Vec<(String, String)>,
    pub cheatsheet_open: bool,
    pub shared_info_open: bool,
    pub confirm_open: bool,
    pub confirm_msg: String,
    pub confirm_action: ConfirmAction,
    pub active_pipelines: Vec<PipelineDisplay>,
    pub event_tx: Option<std::sync::mpsc::Sender<super::event::AppEvent>>,
    pub last_scan: Instant,
    pub scan_pending: bool,
    pub tui_window_id: Option<String>,
    pub tui_session: Option<String>,
    pub tui_pane_id: Option<String>,
    pub right_pane_id: Option<String>,
    pub joined_service: Option<String>,
    pub swap_pending: bool,
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
    WsBranchPick { ws_name: String, ws_branch: String, items_data: String, idx: usize },
    NameInput { context: String },
    Confirm { action: ConfirmAction },
}

pub(crate) const POPUP_RESULT_FILE: &str = "/tmp/tncli-popup-result";

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
            super::app_collapse::load_collapse_state(&session, &dir_names);

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

    pub fn current_combo_item(&self) -> Option<&ComboItem> {
        self.combo_items.get(self.cursor)
    }

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

    pub(crate) fn wt_tmux_name(&self, dir_name: &str, svc_name: &str, branch: &str) -> String {
        let alias = self.config.repos.get(dir_name)
            .and_then(|d| d.alias.as_deref())
            .unwrap_or(dir_name);
        let branch_safe = branch.replace('/', "-");
        format!("{alias}~{svc_name}~{branch_safe}")
    }

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

    pub fn main_workspace_dir(&self) -> std::path::PathBuf {
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
        let branch = self.config.global_default_branch();
        config_dir.join(format!("workspace--{branch}"))
    }

    pub fn current_list_len(&self) -> usize {
        self.combo_items.len()
    }

    pub fn clamp_cursor(&mut self) {
        let len = self.current_list_len();
        if self.cursor >= len && len > 0 {
            self.cursor = len - 1;
        }
    }

    pub fn has_branch_conflict(&self, dir_name: &str, branch: &str) -> bool {
        self.worktrees.values().any(|wt| wt.parent_dir == dir_name && wt.branch == branch)
    }

    pub fn svc_session(&self) -> String {
        format!("tncli_{}", self.session)
    }
}
