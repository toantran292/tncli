use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use ratatui::text::Line;

use crate::config::{Config, Shortcut};
use crate::tmux;

use super::ansi::parse_ansi_line_with_search;

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
fn workspace_branch(wt: &crate::worktree::WorktreeInfo) -> Option<String> {
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
    parsed_lines: Vec<Line<'static>>,
    parsed_dirty: bool,
    parsed_query: String,
    parsed_current_match: Option<usize>,
    parsed_start: usize,
    parsed_end: usize,
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


    /// Build flattened Workspaces tree: combo definitions + active instances nested under them.
    pub fn rebuild_combo_tree(&mut self) {
        self.combo_items.clear();

        // Collect active workspace instances (grouped by branch, sorted by name)
        let mut instances: indexmap::IndexMap<String, Vec<(String, String)>> = indexmap::IndexMap::new();
        for (wt_key, wt) in &self.worktrees {
            if let Some(branch) = workspace_branch(wt) {
                instances.entry(branch).or_default().push((wt.parent_dir.clone(), wt_key.clone()));
            }
        }
        instances.sort_keys();
        let all_ws = self.config.all_workspaces();

        // Sort dirs within each instance by combo order (not alphabetical)
        for (_, dirs) in instances.iter_mut() {
            let combo_order: Vec<String> = all_ws.values().flat_map(|entries| {
                let mut seen = Vec::new();
                for entry in entries {
                    if let Some((dir, _)) = self.config.find_service_entry_quiet(entry) {
                        if !seen.contains(&dir) { seen.push(dir); }
                    }
                }
                seen
            }).collect();
            dirs.sort_by(|a, b| {
                let ia = combo_order.iter().position(|d| d == &a.0).unwrap_or(usize::MAX);
                let ib = combo_order.iter().position(|d| d == &b.0).unwrap_or(usize::MAX);
                ia.cmp(&ib)
            });
        }
        let mut matched_instances: std::collections::HashSet<String> = std::collections::HashSet::new();

        for name in &self.combos {
            self.combo_items.push(ComboItem::Combo(name.clone()));

            // Find combo's unique dir names
            let combo_dirs: Vec<String> = all_ws.get(name)
                .map(|entries| {
                    let mut dirs = Vec::new();
                    for entry in entries {
                        if let Some((dir, _)) = self.config.find_service_entry_quiet(entry) {
                            if !dirs.contains(&dir) { dirs.push(dir); }
                        }
                    }
                    dirs
                })
                .unwrap_or_default();

            // "main" instance (always first, virtual)
            let main_inst_key = format!("ws-inst-main-{name}");
            self.combo_items.push(ComboItem::Instance { branch: "main".to_string(), is_main: true });
            if !self.combo_collapsed.get(&main_inst_key).copied().unwrap_or(false) {
                for dir_name in &combo_dirs {
                    self.combo_items.push(ComboItem::InstanceDir {
                        branch: "main".to_string(),
                        dir: dir_name.clone(),
                        wt_key: String::new(),
                        is_main: true,
                    });
                    let dir_key = format!("ws-dir-main-{name}-{dir_name}");
                    if !self.combo_collapsed.get(&dir_key).copied().unwrap_or(false) {
                        if let Some(dir_cfg) = self.config.repos.get(dir_name) {
                            for svc_name in dir_cfg.services.keys() {
                                self.combo_items.push(ComboItem::InstanceService {
                                    branch: "main".to_string(),
                                    dir: dir_name.clone(),
                                    wt_key: String::new(),
                                    svc: svc_name.clone(),
                                    tmux_name: svc_name.clone(),
                                    is_main: true,
                                });
                            }
                        }
                    }
                }
            }

            // Show creating workspaces under this combo
            for branch in &self.creating_workspaces.clone() {
                if !matched_instances.contains(branch) && !instances.contains_key(branch) {
                    self.combo_items.push(ComboItem::Instance { branch: branch.clone(), is_main: false });
                    matched_instances.insert(branch.clone());
                }
            }

            // Find instances whose dirs match this combo
            for (branch, dirs) in &instances {
                if matched_instances.contains(branch) { continue; }
                let inst_dirs: Vec<&str> = dirs.iter().map(|(d, _)| d.as_str()).collect();
                let matches = !combo_dirs.is_empty()
                    && combo_dirs.iter().all(|d| inst_dirs.contains(&d.as_str()));
                if !matches { continue; }

                matched_instances.insert(branch.clone());
                self.combo_items.push(ComboItem::Instance { branch: branch.clone(), is_main: false });

                let inst_key = format!("ws-inst-{branch}");
                if !self.combo_collapsed.get(&inst_key).copied().unwrap_or(false) {
                    for (dir_name, wt_key) in dirs {
                        self.combo_items.push(ComboItem::InstanceDir {
                            branch: branch.clone(),
                            dir: dir_name.clone(),
                            wt_key: wt_key.clone(),
                            is_main: false,
                        });

                        let dir_key = format!("ws-dir-{branch}-{dir_name}");
                        if !self.combo_collapsed.get(&dir_key).copied().unwrap_or(false) {
                            if let Some(dir_cfg) = self.config.repos.get(dir_name) {
                                for svc_name in dir_cfg.services.keys() {
                                    let tmux_name = self.wt_tmux_name(dir_name, svc_name, branch);
                                    self.combo_items.push(ComboItem::InstanceService {
                                        branch: branch.clone(),
                                        dir: dir_name.clone(),
                                        wt_key: wt_key.clone(),
                                        svc: svc_name.clone(),
                                        tmux_name,
                                        is_main: false,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // Orphan instances (no matching combo) — show at end
        for (branch, dirs) in &instances {
            if matched_instances.contains(branch) { continue; }
            self.combo_items.push(ComboItem::Instance { branch: branch.clone(), is_main: false });
            let inst_key = format!("ws-inst-{branch}");
            if !self.combo_collapsed.get(&inst_key).copied().unwrap_or(false) {
                for (dir_name, wt_key) in dirs {
                    self.combo_items.push(ComboItem::InstanceDir {
                        branch: branch.clone(),
                        dir: dir_name.clone(),
                        wt_key: wt_key.clone(),
                        is_main: false,
                    });
                    let dir_key = format!("ws-dir-{branch}-{dir_name}");
                    if !self.combo_collapsed.get(&dir_key).copied().unwrap_or(false) {
                        if let Some(dir_cfg) = self.config.repos.get(dir_name) {
                            for svc_name in dir_cfg.services.keys() {
                                let tmux_name = self.wt_tmux_name(dir_name, svc_name, branch);
                                self.combo_items.push(ComboItem::InstanceService {
                                    branch: branch.clone(),
                                    dir: dir_name.clone(),
                                    wt_key: wt_key.clone(),
                                    svc: svc_name.clone(),
                                    tmux_name,
                                    is_main: false,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Clamp cursor after rebuild
        let max = self.combo_items.len();
        if max > 0 && self.cursor >= max {
            self.cursor = max - 1;
        }
    }

    /// Toggle collapse of a workspace instance/dir at cursor.
    pub fn toggle_collapse(&mut self) {
        match self.combo_items.get(self.cursor).cloned() {
            Some(ComboItem::Instance { branch, is_main }) => {
                // For main instance, use combo-specific key; find parent combo
                let key = if is_main {
                    // Find the combo name above this item
                    let combo_name = self.find_parent_combo(self.cursor);
                    format!("ws-inst-main-{combo_name}")
                } else {
                    format!("ws-inst-{branch}")
                };
                let collapsed = self.combo_collapsed.get(&key).copied().unwrap_or(false);
                self.combo_collapsed.insert(key, !collapsed);
                self.rebuild_combo_tree();
            }
            Some(ComboItem::InstanceDir { branch, dir, is_main, .. }) => {
                let key = if is_main {
                    let combo_name = self.find_parent_combo(self.cursor);
                    format!("ws-dir-main-{combo_name}-{dir}")
                } else {
                    format!("ws-dir-{branch}-{dir}")
                };
                let collapsed = self.combo_collapsed.get(&key).copied().unwrap_or(false);
                self.combo_collapsed.insert(key, !collapsed);
                self.rebuild_combo_tree();
            }
            _ => {}
        }
        self.save_collapse_state();
    }

    /// Find the parent Combo name for an item at a given index.
    fn find_parent_combo(&self, idx: usize) -> String {
        for i in (0..=idx).rev() {
            if let Some(ComboItem::Combo(name)) = self.combo_items.get(i) {
                return name.clone();
            }
        }
        String::new()
    }

    /// Save collapse state to disk.
    fn save_collapse_state(&self) {
        save_collapse_state(&self.session, &self.dir_names, &self.wt_collapsed, &self.combo_collapsed);
    }

    /// Scan for existing git worktrees and load them.
    pub fn scan_worktrees(&mut self) {
        self.worktrees.clear();
        for dir_name in &self.dir_names {
            // Scan all dirs (workspace can create worktrees for dirs without worktree: true)
            let dir_path = match self.dir_path(dir_name) {
                Some(p) => p,
                None => continue,
            };
            let wts = match crate::worktree::list_worktrees(std::path::Path::new(&dir_path)) {
                Ok(w) => w,
                Err(_) => continue,
            };
            // Skip the main worktree (first entry = the repo itself)
            for (wt_path, branch) in wts.iter().skip(1) {
                let wt_path = std::path::PathBuf::from(wt_path);
                let wt_key = format!("{dir_name}--{}", branch.replace('/', "-"));
                let allocs = crate::worktree::load_ip_allocations();
                // Try per-worktree key first, then workspace key (ws-{branch})
                let ip = allocs.get(&wt_key)
                    .or_else(|| allocs.get(&format!("ws-{}", branch.replace('/', "-"))))
                    .cloned()
                    .unwrap_or_default();
                self.worktrees.insert(wt_key, crate::worktree::WorktreeInfo {
                    branch: branch.clone(),
                    parent_dir: dir_name.clone(),
                    bind_ip: ip,
                    path: wt_path,
                });
            }
        }
        self.rebuild_combo_tree();
    }

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
        match crate::worktree::create_worktree(std::path::Path::new(&dir_path), branch, &copy_files) {
            Ok(wt_path) => {
                let wt_key = format!("{dir_name}--{}", branch.replace('/', "-"));
                let ip = crate::worktree::allocate_ip(&wt_key);
                // Generate .env.tncli + docker-compose.override.yml
                let _ = crate::worktree::write_env_file(&wt_path, &ip);
                let repo_dir = std::path::Path::new(&dir_path);
                let dir_cfg = self.config.repos.get(dir_name);
                let wt_cfg = dir_cfg.and_then(|d| d.wt());
                let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
                let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
                crate::worktree::generate_compose_override(repo_dir, &wt_path, &ip, &compose_files, &worktree_env, branch, None, None, &[]);
                // Ensure docker-compose.override.yml is globally gitignored
                crate::worktree::ensure_global_gitignore();
                self.worktrees.insert(wt_key.clone(), crate::worktree::WorktreeInfo {
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
            crate::worktree::release_ip(wt_key);
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
            let _ = crate::worktree::remove_worktree(
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
        if !compose_files.is_empty() {
            crate::worktree::setup_main_as_worktree(
                p, &compose_files, &wt_cfg.env, &branch,
                if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                &shared_hosts,
            );
        }
        // Write env file
        let branch_safe = crate::worktree::branch_safe(&branch);
        let resolved = crate::worktree::resolve_env_templates(&wt_cfg.env, "127.0.0.1", &branch_safe, &branch);
        let env_file = wt_cfg.env_file.as_deref().unwrap_or(".env.local");
        crate::worktree::apply_env_overrides(p, &resolved, env_file);
        let _ = crate::worktree::write_env_file(p, "127.0.0.1");
        crate::worktree::ensure_global_gitignore();
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
                if !compose_files.is_empty() {
                    crate::worktree::setup_main_as_worktree(
                        p, &compose_files, &wt_cfg.env, &branch,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts,
                    );
                }
                // Write env file for main
                let branch_safe = crate::worktree::branch_safe(&branch);
                let resolved = crate::worktree::resolve_env_templates(&wt_cfg.env, "127.0.0.1", &branch_safe, &branch);
                let env_file = wt_cfg.env_file.as_deref().unwrap_or(".env.local");
                crate::worktree::apply_env_overrides(p, &resolved, env_file);
                let _ = crate::worktree::write_env_file(p, "127.0.0.1");
                count += 1;
            }
        }
        crate::worktree::ensure_global_gitignore();
        format!("{count} dirs bound to 127.0.0.1. Restart services to apply.")
    }

    /// Create workspace: mark as "creating", run heavy work in background.
    /// UI shows "creating..." immediately, `scan_worktrees` picks up results on tick.
    pub fn create_workspace(&mut self, ws_name: &str, branch_name: &str) -> (String, Option<String>) {
        let workspaces = self.config.all_workspaces();
        let entries = match workspaces.get(ws_name) {
            Some(e) => e.clone(),
            None => return ("workspace not found".into(), None),
        };

        let mut unique_dirs: Vec<String> = Vec::new();
        for entry in &entries {
            if let Ok((dir, _)) = self.config.find_service_entry(entry) {
                if !unique_dirs.contains(&dir) {
                    unique_dirs.push(dir);
                }
            }
        }
        if unique_dirs.is_empty() {
            return ("no dirs found in workspace".into(), None);
        }

        // Check /etc/hosts for shared service hostnames
        if !self.config.shared_services.is_empty() {
            let hostnames: Vec<&str> = self.config.shared_services.values()
                .filter_map(|s| s.host.as_deref())
                .collect();
            let missing = crate::worktree::check_etc_hosts(&hostnames);
            if !missing.is_empty() {
                return (format!("Missing hosts in /etc/hosts: {}. Run: tncli setup", missing.join(", ")), None);
            }
        }

        // Allocate IP + slots (fast, state only)
        let ip = crate::worktree::allocate_ip(&format!("ws-{branch_name}"));
        if !self.config.shared_services.is_empty() {
            let ws_key = format!("ws-{branch_name}");
            for dir_name in &unique_dirs {
                if let Some(dir) = self.config.repos.get(dir_name) {
                    if let Some(wt_cfg) = dir.wt() {
                        for sref in &wt_cfg.shared_services {
                            if let Some(svc_def) = self.config.shared_services.get(&sref.name) {
                                if let Some(capacity) = svc_def.capacity {
                                    let base_port = svc_def.ports.first()
                                        .and_then(|p| p.split(':').next())
                                        .and_then(|p| p.parse::<u16>().ok())
                                        .unwrap_or(6379);
                                    crate::worktree::allocate_slot(&sref.name, &ws_key, capacity, base_port);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Mark as creating → UI shows "creating..." immediately
        self.creating_workspaces.insert(branch_name.to_string());
        self.rebuild_combo_tree();

        // Collect all data needed for background thread (can't borrow self)
        let config = self.config.clone();
        let config_path = self.config_path.clone();
        let session = self.session.clone();
        let branch = branch_name.to_string();
        let bind_ip = ip.clone();
        let dirs = unique_dirs.clone();
        let dir_branches: Vec<(String, String)> = unique_dirs.iter()
            .map(|d| (d.clone(), self.dir_branch(d).unwrap_or_else(|| "main".to_string())))
            .collect();
        let dir_paths: Vec<(String, String)> = unique_dirs.iter()
            .filter_map(|d| self.dir_path(d).map(|p| (d.clone(), p)))
            .collect();
        let shared_overrides: Vec<(String, indexmap::IndexMap<String, crate::config::ServiceOverride>, Vec<String>)> =
            unique_dirs.iter()
                .map(|d| {
                    let (ov, hosts) = self.resolve_shared_overrides(d);
                    (d.clone(), ov, hosts)
                })
                .collect();

        // Background: all heavy I/O
        std::thread::spawn(move || {
            let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
            let ws_folder = crate::worktree::ensure_workspace_folder(config_dir, &branch);
            let network_name = format!("tncli-ws-{branch}");
            let branch_safe = crate::worktree::branch_safe(&branch);

            // Start shared services
            if !config.shared_services.is_empty() {
                let mut needed: Vec<String> = Vec::new();
                for dir_name in &dirs {
                    if let Some(dir) = config.repos.get(dir_name) {
                        if let Some(wt_cfg) = dir.wt() {
                            for sref in &wt_cfg.shared_services {
                                if !needed.contains(&sref.name) { needed.push(sref.name.clone()); }
                            }
                        }
                    }
                }
                crate::worktree::generate_shared_compose(config_dir, &session, &config.shared_services);
                let refs: Vec<&str> = needed.iter().map(|s| s.as_str()).collect();
                crate::worktree::start_shared_services(config_dir, &session, &refs);

                // Create databases
                for dir_name in &dirs {
                    if let Some(dir) = config.repos.get(dir_name) {
                        if let Some(wt_cfg) = dir.wt() {
                            for sref in &wt_cfg.shared_services {
                                if let Some(db_tpl) = &sref.db_name {
                                    let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                                        .replace("{{branch}}", &branch);
                                    let svc_def = config.shared_services.get(&sref.name);
                                    let host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost");
                                    let port = svc_def.and_then(|d| d.ports.first())
                                        .and_then(|p| p.split(':').next())
                                        .and_then(|p| p.parse().ok())
                                        .unwrap_or(5432);
                                    let user = svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres");
                                    let pw = svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres");
                                    crate::worktree::create_shared_db(host, port, &db_name, user, pw);
                                }
                            }
                        }
                    }
                }
            }

            // Setup main as worktree (127.0.0.1, with env + shared hosts + DB)
            for (dir_name, dir_path) in &dir_paths {
                let p = std::path::Path::new(dir_path);
                let dir_cfg = config.repos.get(dir_name);
                let wt_cfg = dir_cfg.and_then(|d| d.wt());
                if let Some(wt) = wt_cfg {
                    if !wt.compose_files.is_empty() || p.join("docker-compose.yml").is_file() {
                        let compose_files = if wt.compose_files.is_empty() {
                            vec!["docker-compose.yml".to_string()]
                        } else {
                            wt.compose_files.clone()
                        };
                        let (svc_overrides, shared_hosts) = shared_overrides.iter()
                            .find(|(d, _, _)| d == dir_name)
                            .map(|(_, ov, h)| (ov.clone(), h.clone()))
                            .unwrap_or_default();
                        let main_branch = dir_branches.iter()
                            .find(|(d, _)| d == dir_name)
                            .map(|(_, b)| b.as_str())
                            .unwrap_or("main");
                        crate::worktree::setup_main_as_worktree(
                            p, &compose_files, &wt.env, main_branch,
                            if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                            &shared_hosts,
                        );
                    }

                    // Write env file for main dir
                    let main_branch = dir_branches.iter()
                        .find(|(d, _)| d == dir_name)
                        .map(|(_, b)| b.as_str())
                        .unwrap_or("main");
                    let branch_safe = crate::worktree::branch_safe(main_branch);
                    let resolved = crate::worktree::resolve_env_templates(&wt.env, "127.0.0.1", &branch_safe, main_branch);
                    let env_file = wt.env_file.as_deref().unwrap_or(".env.local");
                    crate::worktree::apply_env_overrides(p, &resolved, env_file);
                    let _ = crate::worktree::write_env_file(p, "127.0.0.1");

                    // Create DB for main dir
                    for sref in &wt.shared_services {
                        if let Some(db_tpl) = &sref.db_name {
                            let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                                .replace("{{branch}}", main_branch);
                            let svc_def = config.shared_services.get(&sref.name);
                            let host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost");
                            let port = svc_def.and_then(|d| d.ports.first())
                                .and_then(|p| p.split(':').next())
                                .and_then(|p| p.parse().ok())
                                .unwrap_or(5432);
                            let user = svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres");
                            let pw = svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres");
                            crate::worktree::create_shared_db(host, port, &db_name, user, pw);
                        }
                    }
                }
            }

            // Create worktrees
            let mut wt_dirs: Vec<(String, std::path::PathBuf)> = Vec::new();
            for (dir_name, base_branch) in &dir_branches {
                let dir_path = match dir_paths.iter().find(|(d, _)| d == dir_name) {
                    Some((_, p)) => p.clone(),
                    None => continue,
                };
                let wt_cfg = config.repos.get(dir_name).and_then(|d| d.wt());
                let copy_files = wt_cfg.map(|wt| wt.copy.clone()).unwrap_or_default();

                if let Ok(wt_path) = crate::worktree::create_worktree_from_base(
                    std::path::Path::new(&dir_path), &branch, base_branch, &copy_files, Some(&ws_folder),
                ) {
                    // Generate compose override
                    let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
                    let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
                    let (svc_overrides, shared_hosts) = shared_overrides.iter()
                        .find(|(d, _, _)| d == dir_name)
                        .map(|(_, ov, h)| (ov.clone(), h.clone()))
                        .unwrap_or_default();
                    crate::worktree::generate_compose_override(
                        std::path::Path::new(&dir_path), &wt_path, &bind_ip,
                        &compose_files, &worktree_env, &branch, None,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts,
                    );
                    let _ = crate::worktree::write_env_file(&wt_path, &bind_ip);

                    // Write .env.local
                    let resolved = crate::worktree::resolve_env_templates(&worktree_env, &bind_ip, &branch_safe, &branch);
                    let env_file = wt_cfg.and_then(|wt| wt.env_file.as_deref()).unwrap_or(".env.local");
                    crate::worktree::apply_env_overrides(&wt_path, &resolved, env_file);

                    wt_dirs.push((dir_name.clone(), wt_path));
                }
            }

            // Run setup commands (login shell to load nvm/rvm)
            for (dir_name, wt_path) in &wt_dirs {
                let setup = config.repos.get(dir_name)
                    .and_then(|d| d.wt())
                    .map(|wt| wt.setup.clone()).unwrap_or_default();
                if !setup.is_empty() {
                    let combined = setup.join(" && ");
                    let _ = std::process::Command::new("zsh")
                        .args(["-lc", &combined])
                        .current_dir(wt_path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
            }

            // Create network + regenerate overrides with network
            crate::worktree::create_docker_network(&network_name);
            for (dir_name, wt_path) in &wt_dirs {
                let wt_cfg = config.repos.get(dir_name).and_then(|d| d.wt());
                let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
                let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
                let dir_path = dir_paths.iter().find(|(d, _)| d == dir_name)
                    .map(|(_, p)| p.clone()).unwrap_or_default();
                let (svc_overrides, shared_hosts) = shared_overrides.iter()
                    .find(|(d, _, _)| d == dir_name)
                    .map(|(_, ov, h)| (ov.clone(), h.clone()))
                    .unwrap_or_default();
                crate::worktree::generate_compose_override(
                    std::path::Path::new(&dir_path), wt_path, &bind_ip,
                    &compose_files, &worktree_env, &branch, Some(&network_name),
                    if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                    &shared_hosts,
                );
            }

            crate::worktree::ensure_global_gitignore();
        });

        (format!("creating workspace {branch_name} (BIND_IP={ip})..."), Some(ip))
    }

    /// Delete workspace: mark as deleting, run cleanup in background.
    /// Worktrees stay in state with "deleting" status until cleanup finishes.
    pub fn delete_workspace_by_name(&mut self, branch_name: &str) -> (String, Option<String>) {
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();

        // Mark workspace as deleting (UI shows "deleting..." state)
        self.deleting_workspaces.insert(branch_name.to_string());
        self.rebuild_combo_tree();

        // Collect worktree info for background cleanup
        let wt_keys: Vec<String> = self.worktrees.keys()
            .filter(|k| k.ends_with(&format!("--{}", branch_name.replace('/', "-"))))
            .cloned()
            .collect();

        let mut cleanup_items: Vec<(String, PathBuf, String, Vec<String>)> = Vec::new();
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

                cleanup_items.push((dir_path, wt.path.clone(), wt.branch.clone(), pre_delete));
            }
        }

        // Release shared service slots + IP
        let ws_key = format!("ws-{branch_name}");
        for (svc_name, _) in &self.config.shared_services {
            crate::worktree::release_slot(svc_name, &ws_key);
        }
        crate::worktree::release_ip(&ws_key);

        // Collect databases to drop
        let branch_safe = branch_name.replace('/', "_").replace('-', "_");
        let mut dbs_to_drop: Vec<(String, u16, String, String, String)> = Vec::new(); // (host, port, db, user, pw)
        for wt_key in &wt_keys {
            if let Some(wt) = self.worktrees.get(wt_key) {
                if let Some(dir) = self.config.repos.get(&wt.parent_dir) {
                    for sref in dir.wt().into_iter().flat_map(|wt| &wt.shared_services) {
                        if let Some(db_tpl) = &sref.db_name {
                            let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                                .replace("{{branch}}", branch_name);
                            let svc_def = self.config.shared_services.get(&sref.name);
                            let host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost").to_string();
                            let port = svc_def.and_then(|d| d.ports.first())
                                .and_then(|p| p.split(':').next())
                                .and_then(|p| p.parse().ok())
                                .unwrap_or(5432);
                            let user = svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres").to_string();
                            let pw = svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres").to_string();
                            dbs_to_drop.push((host, port, db_name, user, pw));
                        }
                    }
                }
            }
        }

        // Background: run all cleanup, then remove from state
        let network = format!("tncli-ws-{branch_name}");
        let branch_owned = branch_name.to_string();
        let config_dir_owned = config_dir.clone();
        std::thread::spawn(move || {
            // Pre-delete commands + git worktree remove for each dir
            for (dir_path, wt_path, wt_branch, pre_delete) in &cleanup_items {
                if !pre_delete.is_empty() && wt_path.exists() {
                    let combined = pre_delete.join(" && ");
                    let _ = std::process::Command::new("zsh")
                        .args(["-c", &combined])
                        .current_dir(wt_path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
                let _ = crate::worktree::remove_worktree(
                    std::path::Path::new(dir_path), wt_path, wt_branch,
                );
            }
            // Drop databases from shared postgres
            for (host, port, db_name, user, pw) in &dbs_to_drop {
                crate::worktree::drop_shared_db(host, *port, db_name, user, pw);
            }
            crate::worktree::remove_docker_network(&network);
            crate::worktree::delete_workspace_folder(&config_dir_owned, &branch_owned);
        });

        let ip_to_teardown: Option<String> = None;

        let msg = format!("workspace {} deleted ({} worktrees)", branch_name, wt_keys.len());
        (msg, ip_to_teardown)
    }

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
            let ws_name = self.ws_name.clone();
            self.ws_creating = false;
            let (msg, _) = self.create_workspace(&ws_name, &new_branch);
            self.set_message(&msg);
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
        match crate::worktree::create_worktree_from_base(
            std::path::Path::new(&dir_path), new_branch, base_branch, &copy_files, None
        ) {
            Ok(wt_path) => {
                let wt_key = format!("{dir_name}--{}", new_branch.replace('/', "-"));
                let ip = crate::worktree::allocate_ip(&wt_key);
                let _ = crate::worktree::write_env_file(&wt_path, &ip);
                let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
                let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
                let repo_dir = std::path::Path::new(&dir_path);
                crate::worktree::generate_compose_override(repo_dir, &wt_path, &ip, &compose_files, &worktree_env, new_branch, None, None, &[]);
                crate::worktree::ensure_global_gitignore();
                self.worktrees.insert(wt_key.clone(), crate::worktree::WorktreeInfo {
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
    fn wt_tmux_name(&self, dir_name: &str, svc_name: &str, branch: &str) -> String {
        let alias = self.config.repos.get(dir_name)
            .and_then(|d| d.alias.as_deref())
            .unwrap_or(dir_name);
        let branch_safe = branch.replace('/', "-");
        format!("{alias}~{svc_name}~{branch_safe}")
    }

    /// Resolve shared services for a dir: merge service_overrides with shared disabled profiles,
    /// and collect shared hostnames for extra_hosts.
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
    pub fn combo_running_services(&self) -> Vec<String> {
        match self.combo_items.get(self.cursor) {
            Some(ComboItem::Combo(combo_name)) => {
                let workspaces = self.config.all_workspaces();
                let entries = match workspaces.get(combo_name.as_str()) {
                    Some(e) => e,
                    None => return Vec::new(),
                };
                entries.iter().filter_map(|entry| {
                    self.config.find_service_entry_quiet(entry)
                        .map(|(_, svc)| svc)
                        .filter(|svc| self.is_running(svc))
                }).collect()
            }
            Some(ComboItem::Instance { branch, is_main }) => {
                if *is_main {
                    // For main instance: collect bare svc names from combo dirs
                    let combo_name = self.find_parent_combo(self.cursor);
                    let all_ws = self.config.all_workspaces();
                    let entries = match all_ws.get(&combo_name) {
                        Some(e) => e,
                        None => return Vec::new(),
                    };
                    entries.iter().filter_map(|entry| {
                        self.config.find_service_entry_quiet(entry)
                            .map(|(_, svc)| svc)
                            .filter(|svc| self.is_running(svc))
                    }).collect()
                } else {
                    let branch_safe = branch.replace('/', "-");
                    self.worktrees.values()
                        .filter(|wt| workspace_branch(wt).as_deref() == Some(branch.as_str()))
                        .flat_map(|wt| {
                            let alias = self.config.repos.get(&wt.parent_dir)
                                .and_then(|d| d.alias.as_deref())
                                .unwrap_or(&wt.parent_dir);
                            self.config.repos.get(&wt.parent_dir)
                                .map(|d| d.services.keys()
                                    .map(|s| format!("{alias}~{s}~{branch_safe}"))
                                    .filter(|tmux_name| self.is_running(tmux_name))
                                    .collect::<Vec<_>>())
                                .unwrap_or_default()
                        })
                        .collect()
                }
            }
            Some(ComboItem::InstanceDir { branch, dir, is_main, .. }) => {
                if *is_main {
                    self.config.repos.get(dir)
                        .map(|d| d.services.keys()
                            .filter(|s| self.is_running(s))
                            .cloned()
                            .collect())
                        .unwrap_or_default()
                } else {
                    let branch_safe = branch.replace('/', "-");
                    let alias = self.config.repos.get(dir).and_then(|d| d.alias.as_deref()).unwrap_or(dir);
                    self.config.repos.get(dir)
                        .map(|d| d.services.keys()
                            .map(|s| format!("{alias}~{s}~{branch_safe}"))
                            .filter(|tmux_name| self.is_running(tmux_name))
                            .collect())
                        .unwrap_or_default()
                }
            }
            Some(ComboItem::InstanceService { tmux_name, .. }) => {
                if self.is_running(tmux_name) { vec![tmux_name.clone()] } else { Vec::new() }
            }
            None => Vec::new(),
        }
    }

    /// Get running services for current selection.
    pub fn current_running_services(&self) -> Vec<String> {
        self.combo_running_services()
    }

    /// Get service name for log display with cycling support.
    pub fn log_service_name(&self) -> Option<String> {
        let running = self.current_running_services();
        if running.is_empty() {
            return None;
        }
        let idx = self.combo_log_idx % running.len();
        Some(running[idx].clone())
    }

    /// Get total + index for log title (e.g. [1/3]).
    pub fn log_cycle_info(&self) -> Option<(usize, usize)> {
        let running = self.current_running_services();
        if running.len() <= 1 {
            return None;
        }
        let idx = self.combo_log_idx % running.len();
        Some((idx + 1, running.len()))
    }

    pub fn cycle_combo_log(&mut self, direction: i32) {
        let running = self.current_running_services();
        if running.len() <= 1 {
            return;
        }
        let len = running.len() as i32;
        self.combo_log_idx = ((self.combo_log_idx as i32 + direction).rem_euclid(len)) as usize;
        self.log_scroll = 0;
        self.invalidate_log();
        self.last_log_size = (0, 0);
    }

    pub fn ensure_log_cache(&mut self, viewport_h: usize) -> bool {
        let svc = match self.log_service_name() {
            Some(s) => s,
            None => {
                self.log_cache.clear();
                self.log_cache_svc = None;
                self.stripped_line_count = 0;
                self.parsed_dirty = true;
                return false;
            }
        };
        if self.log_dirty || self.log_cache_svc.as_deref() != Some(&svc) {
            let capture_lines = if self.log_scroll == 0 && self.search_query.is_empty() {
                viewport_h + 50
            } else {
                3600
            };
            self.log_cache = tmux::capture_pane(&self.session, &svc, capture_lines);
            self.log_cache_svc = Some(svc);
            self.log_dirty = false;
            self.parsed_dirty = true;
            let mut count = self.log_cache.len();
            while count > 0 && self.log_cache[count - 1].trim().is_empty() {
                count -= 1;
            }
            self.stripped_line_count = count;
        }
        self.stripped_line_count > 0
    }

    pub fn max_scroll(&self, viewport_h: usize) -> usize {
        self.stripped_line_count.saturating_sub(viewport_h)
    }

    pub fn clamp_scroll_to(&mut self, viewport_h: usize) {
        let max = self.max_scroll(viewport_h);
        if self.log_scroll > max {
            self.log_scroll = max;
        }
    }

    pub fn get_visible_lines(&mut self, viewport_h: usize) -> &[Line<'static>] {
        self.clamp_scroll_to(viewport_h);
        let total = self.stripped_line_count;
        if total == 0 {
            self.parsed_lines.clear();
            return &self.parsed_lines;
        }
        let start = total.saturating_sub(viewport_h).saturating_sub(self.log_scroll);
        let end = (start + viewport_h).min(total).min(self.log_cache.len());
        let flat_match = if !self.search_query.is_empty() && !self.search_matches.is_empty() {
            self.search_matches.get(self.search_current).map(|m| m.0)
        } else {
            None
        };
        let needs_rerender = self.parsed_dirty
            || self.parsed_start != start
            || self.parsed_end != end
            || self.parsed_query != self.search_query
            || self.parsed_current_match != flat_match;
        if needs_rerender {
            self.parsed_lines.clear();
            for idx in start..end {
                let is_current = flat_match == Some(idx);
                self.parsed_lines.push(
                    parse_ansi_line_with_search(&self.log_cache[idx], &self.search_query, is_current)
                );
            }
            self.parsed_dirty = false;
            self.parsed_start = start;
            self.parsed_end = end;
            self.parsed_query = self.search_query.clone();
            self.parsed_current_match = flat_match;
        }
        &self.parsed_lines
    }

    pub fn invalidate_parsed(&mut self) {
        self.parsed_dirty = true;
    }

    pub fn sync_tmux_size(&mut self, w: u16, h: u16) {
        if (w, h) != self.last_log_size && w > 0 && h > 0 {
            self.last_log_size = (w, h);
            tmux::resize_all_windows(&self.session, w, h);
        }
    }

    pub fn set_message(&mut self, msg: &str) {
        self.message = msg.to_string();
        self.message_time = Some(Instant::now());
    }

    pub fn get_message(&self) -> &str {
        if let Some(t) = self.message_time {
            if t.elapsed().as_secs() < 4 {
                return &self.message;
            }
        }
        ""
    }

    pub fn update_search_matches(&mut self) {
        self.search_matches.clear();
        if self.search_query.is_empty() { return; }
        let query_lower = self.search_query.to_lowercase();
        for (line_idx, line) in self.log_cache.iter().enumerate().take(self.stripped_line_count) {
            let stripped = strip_ansi(line).to_lowercase();
            let mut start = 0;
            while let Some(pos) = stripped[start..].find(&query_lower) {
                self.search_matches.push((line_idx, start + pos));
                start += pos + query_lower.len();
            }
        }
        if self.search_current >= self.search_matches.len() {
            self.search_current = 0;
        }
        self.invalidate_parsed();
    }

    pub fn jump_to_match(&mut self, direction: i32, viewport_h: usize) {
        if self.search_matches.is_empty() { return; }
        let len = self.search_matches.len() as i32;
        self.search_current = ((self.search_current as i32 + direction).rem_euclid(len)) as usize;
        let (match_line, _) = self.search_matches[self.search_current];
        let total = self.stripped_line_count;
        if total > viewport_h {
            self.log_scroll = total.saturating_sub(match_line + viewport_h);
            self.clamp_scroll_to(viewport_h);
        }
        self.invalidate_parsed();
    }

    pub fn scroll_up(&mut self, n: usize) {
        let was_following = self.log_scroll == 0;
        self.log_scroll = self.log_scroll.saturating_add(n);
        if self.log_scroll > self.stripped_line_count {
            self.log_scroll = self.stripped_line_count;
        }
        if was_following && self.log_scroll > 0 {
            self.invalidate_log();
        }
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.log_scroll = self.log_scroll.saturating_sub(n);
    }

    pub fn scroll_to_top(&mut self) {
        self.log_scroll = self.stripped_line_count;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.log_scroll = 0;
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

    fn run_tncli_cmd(&self, args: &[&str]) -> bool {
        let exe = std::env::current_exe().unwrap_or_default();
        std::process::Command::new(exe)
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// Get the CLI target string for current selection.
    /// Services use "dir/svc" format to avoid ambiguity with dir aliases.
    fn current_target(&self) -> Option<String> {
        match self.current_combo_item()? {
            ComboItem::Combo(name) => Some(name.clone()),
            ComboItem::InstanceService { tmux_name, .. } => Some(tmux_name.clone()),
            _ => None,
        }
    }

    /// Start a worktree service directly via tmux (not through CLI).
    fn start_wt_service(&mut self, parent_dir: &str, svc: &str, wt_key: &str, tmux_name: &str) {
        let wt = match self.worktrees.get(wt_key) {
            Some(w) => w.clone(),
            None => { self.set_message("worktree not found"); return; }
        };
        let dir = match self.config.repos.get(parent_dir) {
            Some(d) => d,
            None => { self.set_message("dir not found"); return; }
        };
        let service = match dir.services.get(svc) {
            Some(s) => s,
            None => { self.set_message("service not found"); return; }
        };
        let cmd = match &service.cmd {
            Some(c) => c.clone(),
            None => { self.set_message("no cmd defined"); return; }
        };

        if tmux::window_exists(&self.session, tmux_name) {
            self.set_message(&format!("{tmux_name} already running"));
            return;
        }

        let wt_dir = wt.path.to_string_lossy().to_string();
        let pre_start = service.pre_start.as_deref().or(dir.pre_start.as_deref());
        let env = service.env.as_deref().or(dir.env.as_deref());

        let mut full_cmd = format!("cd '{wt_dir}'");
        if let Some(pre) = pre_start {
            full_cmd.push_str(&format!(" && {pre}"));
        }
        // Export BIND_IP + worktree_env for worktree services
        if !wt.bind_ip.is_empty() {
            full_cmd.push_str(&format!(" && export BIND_IP={}", wt.bind_ip));
            // Export worktree_env vars (resolved with bind_ip/branch)
            // Keep *.local hostnames — Docker resolves via extra_hosts, host via /etc/hosts
            if let Some(wt_cfg) = self.config.repos.get(parent_dir).and_then(|d| d.wt()) {
                let branch_safe = crate::worktree::branch_safe(&wt.branch);
                for (k, v) in &wt_cfg.env {
                    let val = v.replace("{{bind_ip}}", &wt.bind_ip)
                        .replace("{{branch_safe}}", &branch_safe)
                        .replace("{{branch}}", &wt.branch);
                    full_cmd.push_str(&format!(" && export {}='{}'", k, val));
                }
            }
            full_cmd.push_str(&format!(" && {cmd}"));
        } else {
            full_cmd.push_str(&format!(" && {cmd}"));
        }
        if let Some(e) = env {
            full_cmd = format!("{e} {full_cmd}");
        }

        tmux::create_session_if_needed(&self.session);
        tmux::new_window(&self.session, tmux_name, &full_cmd);
        self.refresh_status();
        self.set_message(&format!("started: {tmux_name}"));
    }

    pub fn do_start(&mut self) {
        if let Some(item) = self.current_combo_item().cloned() {
            match item {
                ComboItem::InstanceService { dir, svc, wt_key, tmux_name, is_main, .. } => {
                    if is_main {
                        self.start_main_service(&dir, &svc);
                    } else {
                        self.start_wt_service(&dir, &svc, &wt_key, &tmux_name);
                    }
                    return;
                }
                ComboItem::Instance { branch, is_main } => {
                    if is_main {
                        self.start_main_instance();
                    } else {
                        self.start_workspace_instance(&branch);
                    }
                    return;
                }
                ComboItem::InstanceDir { branch, dir, wt_key, is_main } => {
                    if is_main {
                        self.start_main_dir(&dir);
                    } else {
                        self.start_wt_dir(&dir, &branch, &wt_key);
                    }
                    return;
                }
                _ => {}
            }
        }
        let target = match self.current_target() { Some(t) => t, None => return };
        let ok = self.run_tncli_cmd(&["start", &target]);
        self.refresh_status();
        let msg = if ok { format!("started: {target}") } else { format!("error starting {target}") };
        self.set_message(&msg);
    }

    /// Start a "main" service (bare tmux name, runs from repo dir).
    fn start_main_service(&mut self, dir_name: &str, svc_name: &str) {
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
        if let Ok(resolved) = self.config.resolve_service(config_dir, dir_name, svc_name) {
            if tmux::window_exists(&self.session, svc_name) {
                self.set_message(&format!("{svc_name} already running"));
                return;
            }
            let mut full_cmd = format!("cd '{}'", resolved.work_dir.display());
            if let Some(pre) = &resolved.pre_start { full_cmd.push_str(&format!(" && {pre}")); }
            // Export BIND_IP + worktree env for main services (main uses 127.0.0.1)
            if let Some(wt_cfg) = self.config.repos.get(dir_name).and_then(|d| d.wt()) {
                full_cmd.push_str(" && export BIND_IP=127.0.0.1");
                let branch = self.dir_branch(dir_name).unwrap_or_else(|| "main".to_string());
                let branch_safe = crate::worktree::branch_safe(&branch);
                for (k, v) in &wt_cfg.env {
                    let val = v.replace("{{bind_ip}}", "127.0.0.1")
                        .replace("{{branch_safe}}", &branch_safe)
                        .replace("{{branch}}", &branch);
                    full_cmd.push_str(&format!(" && export {}='{}'", k, val));
                }
            }
            full_cmd.push_str(&format!(" && {}", resolved.cmd));
            if let Some(env) = &resolved.env { full_cmd = format!("{env} {full_cmd}"); }
            tmux::create_session_if_needed(&self.session);
            tmux::new_window(&self.session, svc_name, &full_cmd);
            self.refresh_status();
            self.set_message(&format!("started: {svc_name}"));
        }
    }

    /// Start all main services for the combo under cursor.
    fn start_main_instance(&mut self) {
        let combo_name = self.find_parent_combo(self.cursor);
        let all_ws = self.config.all_workspaces();
        let entries = match all_ws.get(&combo_name) {
            Some(e) => e.clone(),
            None => return,
        };
        let mut started = 0;
        for entry in &entries {
            if let Some((dir, svc)) = self.config.find_service_entry_quiet(entry) {
                if !self.is_running(&svc) {
                    self.start_main_service(&dir, &svc);
                    started += 1;
                }
            }
        }
        self.set_message(&format!("started {started} main services"));
    }

    /// Start all main services in a specific dir.
    fn start_main_dir(&mut self, dir_name: &str) {
        let mut started = 0;
        if let Some(dir) = self.config.repos.get(dir_name).cloned() {
            for svc_name in dir.services.keys() {
                if !self.is_running(svc_name) {
                    self.start_main_service(dir_name, svc_name);
                    started += 1;
                }
            }
        }
        self.set_message(&format!("started {started} services for {dir_name}"));
    }

    /// Start all services in a workspace instance.
    fn start_workspace_instance(&mut self, branch: &str) {
        let branch_safe = branch.replace('/', "-");
        let wt_info: Vec<(String, String)> = self.worktrees.iter()
            .filter(|(_, wt)| workspace_branch(wt).as_deref() == Some(branch))
            .map(|(wt_key, wt)| (wt.parent_dir.clone(), wt_key.clone()))
            .collect();
        let mut started = 0;
        for (dir_name, wt_key) in &wt_info {
            if let Some(dir) = self.config.repos.get(dir_name).cloned() {
                let alias = dir.alias.as_deref().unwrap_or(dir_name.as_str());
                for svc_name in dir.services.keys() {
                    let tmux_name = format!("{alias}~{svc_name}~{branch_safe}");
                    if !self.is_running(&tmux_name) {
                        self.start_wt_service(dir_name, svc_name, wt_key, &tmux_name);
                        started += 1;
                    }
                }
            }
        }
        self.set_message(&format!("started {started} services for workspace {branch}"));
    }

    /// Start all services in a workspace dir.
    fn start_wt_dir(&mut self, dir_name: &str, branch: &str, wt_key: &str) {
        let branch_safe = branch.replace('/', "-");
        let mut started = 0;
        if let Some(dir) = self.config.repos.get(dir_name).cloned() {
            let alias = dir.alias.as_deref().unwrap_or(dir_name);
            for svc_name in dir.services.keys() {
                let tmux_name = format!("{alias}~{svc_name}~{branch_safe}");
                if !self.is_running(&tmux_name) {
                    self.start_wt_service(dir_name, svc_name, wt_key, &tmux_name);
                    started += 1;
                }
            }
        }
        self.set_message(&format!("started {started} services for {dir_name}~{branch}"));
    }

    pub fn do_stop(&mut self) {
        if let Some(item) = self.current_combo_item().cloned() {
            match item {
                ComboItem::InstanceService { tmux_name, .. } => {
                    if !self.is_running(&tmux_name) {
                        self.set_message("nothing to stop");
                        return;
                    }
                    self.stopping_services.insert(tmux_name.clone());
                    self.set_message(&format!("stopping: {tmux_name}..."));
                    let session = self.session.clone();
                    std::thread::spawn(move || {
                        crate::tmux::graceful_stop(&session, &tmux_name);
                    });
                    return;
                }
                ComboItem::Instance { branch, is_main } => {
                    if is_main {
                        self.stop_main_instance();
                    } else {
                        self.stop_workspace_instance(&branch);
                    }
                    return;
                }
                ComboItem::InstanceDir { branch, dir, is_main, .. } => {
                    if is_main {
                        self.stop_main_dir(&dir);
                    } else {
                        self.stop_wt_dir(&dir, &branch);
                    }
                    return;
                }
                _ => {}
            }
        }
        let target = match self.current_target() { Some(t) => t, None => return };
        // Check if any service in target is actually running
        let running_svcs: Vec<String> = if let Ok(pairs) = self.config.resolve_services(&target) {
            pairs.iter().filter(|(_, svc)| self.is_running(svc)).map(|(_, svc)| svc.clone()).collect()
        } else {
            Vec::new()
        };
        if running_svcs.is_empty() {
            self.set_message("nothing to stop");
            return;
        }
        // Mark as stopping
        for svc in &running_svcs {
            self.stopping_services.insert(svc.clone());
        }
        self.set_message(&format!("stopping: {target}..."));
        let exe = std::env::current_exe().unwrap_or_default();
        let target_clone = target.clone();
        std::thread::spawn(move || {
            let _ = std::process::Command::new(exe)
                .args(["stop", &target_clone])
                .output();
        });
    }

    /// Stop all main services for the combo under cursor.
    fn stop_main_instance(&mut self) {
        let combo_name = self.find_parent_combo(self.cursor);
        let all_ws = self.config.all_workspaces();
        let entries = match all_ws.get(&combo_name) {
            Some(e) => e.clone(),
            None => return,
        };
        let svcs: Vec<String> = entries.iter()
            .filter_map(|entry| self.config.find_service_entry_quiet(entry).map(|(_, svc)| svc))
            .filter(|svc| self.is_running(svc))
            .collect();
        if svcs.is_empty() {
            self.set_message("nothing to stop");
            return;
        }
        for s in &svcs { self.stopping_services.insert(s.clone()); }
        self.set_message(&format!("stopping {} main services...", svcs.len()));
        let session = self.session.clone();
        std::thread::spawn(move || {
            for svc in &svcs { crate::tmux::graceful_stop(&session, svc); }
        });
    }

    /// Stop all main services in a specific dir.
    fn stop_main_dir(&mut self, dir_name: &str) {
        let svcs: Vec<String> = self.config.repos.get(dir_name)
            .map(|d| d.services.keys()
                .filter(|s| self.is_running(s))
                .cloned()
                .collect())
            .unwrap_or_default();
        if svcs.is_empty() {
            self.set_message("nothing to stop");
            return;
        }
        for s in &svcs { self.stopping_services.insert(s.clone()); }
        self.set_message(&format!("stopping {} services for {dir_name}...", svcs.len()));
        let session = self.session.clone();
        std::thread::spawn(move || {
            for svc in &svcs { crate::tmux::graceful_stop(&session, svc); }
        });
    }

    pub fn do_stop_all(&mut self) {
        // Mark all running as stopping
        for svc in self.running_windows.iter() {
            self.stopping_services.insert(svc.clone());
        }
        self.set_message("stopping all services...");
        let exe = std::env::current_exe().unwrap_or_default();
        std::thread::spawn(move || {
            let _ = std::process::Command::new(exe).args(["stop"]).output();
        });
    }

    /// Stop all services in a workspace instance.
    fn stop_workspace_instance(&mut self, branch: &str) {
        let branch_safe = branch.replace('/', "-");
        let svcs: Vec<String> = self.worktrees.values()
            .filter(|wt| workspace_branch(wt).as_deref() == Some(branch))
            .flat_map(|wt| {
                let alias = self.config.repos.get(&wt.parent_dir)
                    .and_then(|d| d.alias.as_deref())
                    .unwrap_or(&wt.parent_dir);
                self.config.repos.get(&wt.parent_dir)
                    .map(|d| d.services.keys()
                        .map(|s| format!("{alias}~{s}~{branch_safe}"))
                        .filter(|tmux_name| self.is_running(tmux_name))
                        .collect::<Vec<_>>())
                    .unwrap_or_default()
            })
            .collect();
        if svcs.is_empty() {
            self.set_message("nothing to stop");
            return;
        }
        for s in &svcs { self.stopping_services.insert(s.clone()); }
        self.set_message(&format!("stopping {} services for workspace {branch}...", svcs.len()));
        let session = self.session.clone();
        std::thread::spawn(move || {
            for svc in &svcs { crate::tmux::graceful_stop(&session, svc); }
        });
    }

    /// Stop all services in a workspace dir.
    fn stop_wt_dir(&mut self, dir_name: &str, branch: &str) {
        let branch_safe = branch.replace('/', "-");
        let alias = self.config.repos.get(dir_name).and_then(|d| d.alias.as_deref()).unwrap_or(dir_name);
        let svcs: Vec<String> = self.config.repos.get(dir_name)
            .map(|d| d.services.keys()
                .map(|s| format!("{alias}~{s}~{branch_safe}"))
                .filter(|tmux_name| self.is_running(tmux_name))
                .collect())
            .unwrap_or_default();
        if svcs.is_empty() {
            self.set_message("nothing to stop");
            return;
        }
        for s in &svcs { self.stopping_services.insert(s.clone()); }
        self.set_message(&format!("stopping {} services for {dir_name}~{branch}...", svcs.len()));
        let session = self.session.clone();
        std::thread::spawn(move || {
            for svc in &svcs { crate::tmux::graceful_stop(&session, svc); }
        });
    }

    pub fn do_restart(&mut self) {
        let target = match self.current_target() { Some(t) => t, None => return };
        let ok = self.run_tncli_cmd(&["restart", &target]);
        self.refresh_status();
        let msg = if ok { format!("restarted: {target}") } else { format!("error restarting {target}") };
        self.set_message(&msg);
    }

    pub fn do_toggle(&mut self) {
        match self.current_combo_item().cloned() {
            Some(ComboItem::Combo(name)) => {
                let entries = self.config.all_workspaces().get(&name).cloned().unwrap_or_default();
                let any_running = entries.iter().any(|entry| {
                    self.config.find_service_entry_quiet(entry)
                        .map(|(_, svc)| self.is_running(&svc))
                        .unwrap_or(false)
                });
                if any_running { self.do_stop(); } else { self.do_start(); }
            }
            Some(ComboItem::Instance { .. }) | Some(ComboItem::InstanceDir { .. }) => {
                self.toggle_collapse();
            }
            Some(ComboItem::InstanceService { ref tmux_name, .. }) => {
                if self.is_running(tmux_name) { self.do_stop(); } else { self.do_start(); }
            }
            None => {}
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

fn save_collapse_state(
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
