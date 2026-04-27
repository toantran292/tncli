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
pub enum Section {
    Services,
    Combos,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Left,
    Right,
}

/// An item in the flattened tree view.
#[derive(Debug, Clone, PartialEq)]
pub enum TreeItem {
    Dir(String),
    Service { dir: String, svc: String },
}

pub struct App {
    pub config_path: PathBuf,
    pub config: Config,
    pub session: String,
    // tree
    pub tree_items: Vec<TreeItem>,
    pub dir_collapsed: Vec<bool>,
    pub dir_names: Vec<String>,
    // combos
    pub combos: Vec<String>,
    pub cursor: usize,
    pub section: Section,
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
}

impl App {
    pub fn new(config_path: PathBuf) -> anyhow::Result<Self> {
        let config = Config::load(&config_path)?;
        let session = config.session.clone();
        let dir_names: Vec<String> = config.dirs.keys().cloned().collect();
        let dir_collapsed = vec![false; dir_names.len()];
        let combos: Vec<String> = config.combinations.keys().cloned().collect();

        let mut app = Self {
            config_path,
            config,
            session,
            tree_items: Vec::new(),
            dir_collapsed,
            dir_names,
            combos,
            cursor: 0,
            section: Section::Services,
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
        };
        app.rebuild_tree();
        Ok(app)
    }

    /// Build flattened tree from dirs + collapse state.
    pub fn rebuild_tree(&mut self) {
        self.tree_items.clear();
        for (i, dir_name) in self.dir_names.iter().enumerate() {
            self.tree_items.push(TreeItem::Dir(dir_name.clone()));
            if !self.dir_collapsed.get(i).copied().unwrap_or(false) {
                if let Some(dir) = self.config.dirs.get(dir_name) {
                    for svc_name in dir.services.keys() {
                        self.tree_items.push(TreeItem::Service {
                            dir: dir_name.clone(),
                            svc: svc_name.clone(),
                        });
                    }
                }
            }
        }
    }

    /// Toggle collapse of a dir at cursor.
    pub fn toggle_collapse(&mut self) {
        if let Some(TreeItem::Dir(dir_name)) = self.tree_items.get(self.cursor) {
            if let Some(idx) = self.dir_names.iter().position(|d| d == dir_name) {
                if let Some(v) = self.dir_collapsed.get_mut(idx) {
                    *v = !*v;
                }
                self.rebuild_tree();
            }
        }
    }

    pub fn reload_config(&mut self) -> String {
        match Config::load(&self.config_path) {
            Ok(config) => {
                let old_dirs = self.dir_names.len();
                let old_combos = self.combos.len();

                self.session = config.session.clone();
                self.dir_names = config.dirs.keys().cloned().collect();
                self.dir_collapsed = vec![false; self.dir_names.len()];
                self.combos = config.combinations.keys().cloned().collect();
                self.config = config;
                self.rebuild_tree();
                self.clamp_cursor();

                let svc_count: usize = self.config.dirs.values().map(|d| d.services.len()).sum();
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
    }

    pub fn is_stopping(&self, svc: &str) -> bool {
        self.stopping_services.contains(svc)
    }

    pub fn invalidate_log(&mut self) {
        self.log_dirty = true;
        self.parsed_dirty = true;
    }

    /// Current item under cursor in the services tree.
    pub fn current_tree_item(&self) -> Option<&TreeItem> {
        self.tree_items.get(self.cursor)
    }

    /// Get the service name for the current selection (tree or combo).
    pub fn selected_service_name(&self) -> Option<String> {
        match self.section {
            Section::Services => match self.current_tree_item()? {
                TreeItem::Service { svc, .. } => Some(svc.clone()),
                TreeItem::Dir(_) => None,
            },
            Section::Combos => self.log_service_name(),
        }
    }

    /// Get dir name for current selection.
    pub fn selected_dir_name(&self) -> Option<String> {
        match self.current_tree_item()? {
            TreeItem::Dir(d) => Some(d.clone()),
            TreeItem::Service { dir, .. } => Some(dir.clone()),
        }
    }

    pub fn is_running(&self, svc: &str) -> bool {
        self.running_windows.contains(svc)
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
        let item = match self.current_tree_item() {
            Some(i) => i.clone(),
            None => { self.set_message("no item selected"); return; }
        };
        match item {
            TreeItem::Dir(ref dir_name) => {
                let dir = match self.config.dirs.get(dir_name) {
                    Some(d) => d,
                    None => return,
                };
                if dir.shortcuts.is_empty() {
                    self.set_message(&format!("no shortcuts for dir '{dir_name}'"));
                    return;
                }
                self.shortcuts_items = dir.shortcuts.clone();
                self.shortcuts_title = dir_name.clone();
                self.shortcuts_cursor = 0;
                self.shortcuts_open = true;
            }
            TreeItem::Service { ref dir, ref svc } => {
                let dir_obj = match self.config.dirs.get(dir) {
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
        }
    }

    /// Get the selected shortcut's cmd, desc, and working dir.
    pub fn selected_shortcut(&self) -> Option<(String, String, String)> {
        let shortcut = self.shortcuts_items.get(self.shortcuts_cursor)?;
        let dir_name = self.selected_dir_name()?;
        let dir_path = self.dir_path(&dir_name)?;
        Some((shortcut.cmd.clone(), shortcut.desc.clone(), dir_path))
    }

    pub fn shortcuts_count(&self) -> usize {
        self.shortcuts_items.len()
    }

    /// Combo running services for log cycling.
    pub fn combo_running_services(&self) -> Vec<String> {
        if self.section != Section::Combos {
            return Vec::new();
        }
        let combo_name = match self.combos.get(self.cursor) {
            Some(c) => c,
            None => return Vec::new(),
        };
        let entries = match self.config.combinations.get(combo_name) {
            Some(e) => e,
            None => return Vec::new(),
        };
        // Resolve combo entries to service names, filter running
        entries.iter().filter_map(|entry| {
            self.config.find_service_entry_quiet(entry)
                .map(|(_, svc)| svc)
                .filter(|svc| self.is_running(svc))
        }).collect()
    }

    /// Get running services for current selection (dir, combo, or single service).
    pub fn current_running_services(&self) -> Vec<String> {
        match self.section {
            Section::Services => {
                match self.current_tree_item() {
                    Some(TreeItem::Dir(dir_name)) => {
                        self.config.dirs.get(dir_name)
                            .map(|d| d.services.keys()
                                .filter(|s| self.is_running(s))
                                .cloned()
                                .collect())
                            .unwrap_or_default()
                    }
                    Some(TreeItem::Service { svc, .. }) => {
                        if self.is_running(svc) { vec![svc.clone()] } else { Vec::new() }
                    }
                    None => Vec::new(),
                }
            }
            Section::Combos => self.combo_running_services(),
        }
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
        match self.section {
            Section::Services => self.tree_items.len(),
            Section::Combos => self.combos.len(),
        }
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
        match self.section {
            Section::Services => {
                match self.current_tree_item()? {
                    TreeItem::Service { dir, svc } => Some(format!("{dir}/{svc}")),
                    TreeItem::Dir(d) => Some(d.clone()), // start/stop whole dir
                }
            }
            Section::Combos => self.combos.get(self.cursor).cloned(),
        }
    }

    pub fn do_start(&mut self) {
        let target = match self.current_target() { Some(t) => t, None => return };
        let ok = self.run_tncli_cmd(&["start", &target]);
        self.refresh_status();
        let msg = if ok { format!("started: {target}") } else { format!("error starting {target}") };
        self.set_message(&msg);
    }

    pub fn do_stop(&mut self) {
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

    pub fn do_restart(&mut self) {
        let target = match self.current_target() { Some(t) => t, None => return };
        let ok = self.run_tncli_cmd(&["restart", &target]);
        self.refresh_status();
        let msg = if ok { format!("restarted: {target}") } else { format!("error restarting {target}") };
        self.set_message(&msg);
    }

    pub fn do_toggle(&mut self) {
        match self.section {
            Section::Services => {
                match self.current_tree_item() {
                    Some(TreeItem::Dir(_)) => self.toggle_collapse(),
                    Some(TreeItem::Service { svc, .. }) => {
                        if self.is_running(svc) { self.do_stop(); } else { self.do_start(); }
                    }
                    None => {}
                }
            }
            Section::Combos => {
                let target = match self.combos.get(self.cursor) { Some(t) => t.clone(), None => return };
                // Check if any service in combo is running
                let entries = self.config.combinations.get(&target).cloned().unwrap_or_default();
                let any_running = entries.iter().any(|entry| {
                    self.config.find_service_entry_quiet(entry)
                        .map(|(_, svc)| self.is_running(&svc))
                        .unwrap_or(false)
                });
                if any_running { self.do_stop(); } else { self.do_start(); }
            }
        }
    }
}
