use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use ratatui::text::Line;

use crate::config::Config;
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
                i += 1; // skip 'm'
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

pub struct App {
    pub config_path: PathBuf,
    pub config: Config,
    pub session: String,
    pub services: Vec<String>,
    pub combos: Vec<String>,
    pub cursor: usize,
    pub section: Section,
    pub focus: Focus,
    pub log_scroll: usize,
    pub combo_log_idx: usize,
    pub running_windows: HashSet<String>,
    pub message: String,
    pub message_time: Option<Instant>,
    // log cache
    pub log_cache: Vec<String>,
    pub log_cache_svc: Option<String>,
    pub log_dirty: bool,
    pub last_log_size: (u16, u16),
    // parsed line cache — avoids re-parsing ANSI every frame
    parsed_lines: Vec<Line<'static>>,
    parsed_dirty: bool,
    parsed_query: String,
    parsed_current_match: Option<usize>,
    parsed_start: usize,
    parsed_end: usize,
    // stripped line count (after removing trailing empties)
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
    pub shortcuts_svc: String,
}

impl App {
    pub fn new(config_path: PathBuf) -> anyhow::Result<Self> {
        let config = Config::load(&config_path)?;
        let session = config.session.clone();
        let services: Vec<String> = config.services.keys().cloned().collect();
        let combos: Vec<String> = config.combinations.keys().cloned().collect();

        Ok(Self {
            config_path,
            config,
            session,
            services,
            combos,
            cursor: 0,
            section: Section::Services,
            focus: Focus::Left,
            log_scroll: 0,
            combo_log_idx: 0,
            running_windows: HashSet::new(),
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
            shortcuts_svc: String::new(),
        })
    }

    /// Reload config and return a summary of changes.
    pub fn reload_config(&mut self) -> String {
        match Config::load(&self.config_path) {
            Ok(config) => {
                let old_svcs = self.services.len();
                let old_combos = self.combos.len();
                let new_svcs: Vec<String> = config.services.keys().cloned().collect();
                let new_combos: Vec<String> = config.combinations.keys().cloned().collect();

                self.session = config.session.clone();
                self.services = new_svcs;
                self.combos = new_combos;
                self.config = config;
                self.clamp_cursor();

                format!(
                    "config reloaded — {} services, {} combos (was {}/{})",
                    self.services.len(), self.combos.len(), old_svcs, old_combos
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
    }

    pub fn invalidate_log(&mut self) {
        self.log_dirty = true;
        self.parsed_dirty = true;
    }

    pub fn current_list(&self) -> &[String] {
        match self.section {
            Section::Services => &self.services,
            Section::Combos => &self.combos,
        }
    }

    pub fn current_item(&self) -> Option<&str> {
        self.current_list().get(self.cursor).map(|s| s.as_str())
    }

    pub fn is_running(&self, svc: &str) -> bool {
        self.running_windows.contains(svc)
    }

    /// Get working directory for a service, resolved relative to config dir.
    pub fn service_dir(&self, svc: &str) -> Option<String> {
        let service = self.config.services.get(svc)?;
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
        match &service.dir {
            Some(dir) => {
                let p = std::path::Path::new(dir);
                if p.is_absolute() {
                    Some(dir.clone())
                } else {
                    Some(config_dir.join(dir).to_string_lossy().into_owned())
                }
            }
            None => Some(config_dir.to_string_lossy().into_owned()),
        }
    }

    /// Get the selected service name (for left panel: direct, for right panel: log service).
    pub fn selected_service_name(&self) -> Option<String> {
        match self.section {
            Section::Services => self.current_item().map(|s| s.to_string()),
            Section::Combos => self.selected_service_for_logs().map(|s| s.to_string()),
        }
    }

    /// Open shortcuts popup for the selected service.
    pub fn open_shortcuts(&mut self) {
        let svc = match self.selected_service_name() {
            Some(s) => s,
            None => { self.set_message("no service selected"); return; }
        };
        let shortcuts = self.config.services.get(&svc)
            .map(|s| &s.shortcuts)
            .cloned()
            .unwrap_or_default();
        if shortcuts.is_empty() {
            self.set_message(&format!("no shortcuts defined for '{svc}' — add shortcuts: in tncli.yml"));
            return;
        }
        self.shortcuts_svc = svc;
        self.shortcuts_cursor = 0;
        self.shortcuts_open = true;
    }

    /// Run the selected shortcut command in the service's tmux pane.
    pub fn run_shortcut(&mut self) {
        let svc = self.shortcuts_svc.clone();
        let shortcuts = self.config.services.get(&svc)
            .map(|s| &s.shortcuts)
            .cloned()
            .unwrap_or_default();
        if let Some(shortcut) = shortcuts.get(self.shortcuts_cursor) {
            // Send command + Enter to the service's tmux pane
            crate::tmux::send_keys(&self.session, &svc, &[&shortcut.cmd, "Enter"]);
            self.set_message(&format!("ran: {}", shortcut.desc));
            self.shortcuts_open = false;
            self.log_scroll = 0;
            self.invalidate_log();
            self.invalidate_parsed();
        }
    }

    pub fn combo_running_services(&self) -> Vec<&str> {
        if self.section != Section::Combos {
            return Vec::new();
        }
        let item = match self.current_item() {
            Some(i) => i,
            None => return Vec::new(),
        };
        match self.config.combinations.get(item) {
            Some(svcs) => svcs.iter().filter(|s| self.is_running(s)).map(|s| s.as_str()).collect(),
            None => Vec::new(),
        }
    }

    pub fn selected_service_for_logs(&self) -> Option<&str> {
        let item = self.current_item()?;
        match self.section {
            Section::Services => {
                if self.is_running(item) { Some(item) } else { None }
            }
            Section::Combos => {
                let running = self.combo_running_services();
                if running.is_empty() {
                    None
                } else {
                    Some(running[self.combo_log_idx % running.len()])
                }
            }
        }
    }

    pub fn cycle_combo_log(&mut self, direction: i32) {
        let running = self.combo_running_services();
        if running.len() <= 1 {
            return;
        }
        let len = running.len() as i32;
        self.combo_log_idx = ((self.combo_log_idx as i32 + direction).rem_euclid(len)) as usize;
        self.log_scroll = 0;
        self.invalidate_log();
        self.last_log_size = (0, 0);
    }

    /// Capture log lines from tmux into cache.
    /// When following (scroll=0), only captures a small window for performance.
    /// When scrolling or searching, captures full buffer.
    pub fn ensure_log_cache(&mut self, viewport_h: usize) -> bool {
        let svc = match self.selected_service_for_logs() {
            Some(s) => s.to_string(),
            None => {
                self.log_cache.clear();
                self.log_cache_svc = None;
                self.stripped_line_count = 0;
                self.parsed_dirty = true;
                return false;
            }
        };
        if self.log_dirty || self.log_cache_svc.as_deref() != Some(&svc) {
            // Adaptive capture: small when following, large when scrolling
            let capture_lines = if self.log_scroll == 0 && self.search_query.is_empty() {
                viewport_h + 50 // just enough for viewport + small buffer
            } else {
                3600
            };
            self.log_cache = tmux::capture_pane(&self.session, &svc, capture_lines);
            self.log_cache_svc = Some(svc);
            self.log_dirty = false;
            self.parsed_dirty = true;
            // Strip trailing empty lines
            let mut count = self.log_cache.len();
            while count > 0 && self.log_cache[count - 1].trim().is_empty() {
                count -= 1;
            }
            self.stripped_line_count = count;
        }
        self.stripped_line_count > 0
    }

    /// Max scroll value for given viewport height.
    pub fn max_scroll(&self, viewport_h: usize) -> usize {
        self.stripped_line_count.saturating_sub(viewport_h)
    }

    /// Clamp log_scroll to valid range for given viewport.
    pub fn clamp_scroll_to(&mut self, viewport_h: usize) {
        let max = self.max_scroll(viewport_h);
        if self.log_scroll > max {
            self.log_scroll = max;
        }
    }

    /// Get visible lines as parsed ratatui Lines. Uses cache.
    pub fn get_visible_lines(&mut self, viewport_h: usize) -> &[Line<'static>] {
        self.clamp_scroll_to(viewport_h);

        let total = self.stripped_line_count;
        if total == 0 {
            self.parsed_lines.clear();
            return &self.parsed_lines;
        }

        // Compute visible window
        let start = total.saturating_sub(viewport_h).saturating_sub(self.log_scroll);
        let end = (start + viewport_h).min(total).min(self.log_cache.len());

        // Determine current search match
        let flat_match = if !self.search_query.is_empty() && !self.search_matches.is_empty() {
            self.search_matches.get(self.search_current).map(|m| m.0)
        } else {
            None
        };

        // Re-render only when something changed
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

    /// Force re-render on next get_visible_lines call.
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
        if self.search_query.is_empty() {
            return;
        }
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
        if self.search_matches.is_empty() {
            return;
        }
        let len = self.search_matches.len() as i32;
        self.search_current =
            ((self.search_current as i32 + direction).rem_euclid(len)) as usize;
        let (match_line, _) = self.search_matches[self.search_current];
        let total = self.stripped_line_count;
        if total > viewport_h {
            self.log_scroll = total.saturating_sub(match_line + viewport_h);
            self.clamp_scroll_to(viewport_h);
        }
        self.invalidate_parsed();
    }

    /// Scroll up by n lines. Clamped in get_visible_lines.
    pub fn scroll_up(&mut self, n: usize) {
        let was_following = self.log_scroll == 0;
        self.log_scroll = self.log_scroll.saturating_add(n);
        if self.log_scroll > self.stripped_line_count {
            self.log_scroll = self.stripped_line_count;
        }
        // When leaving follow mode, re-capture with full buffer
        if was_following && self.log_scroll > 0 {
            self.invalidate_log();
        }
    }

    /// Scroll down by n lines.
    pub fn scroll_down(&mut self, n: usize) {
        self.log_scroll = self.log_scroll.saturating_sub(n);
    }

    /// Scroll to top.
    pub fn scroll_to_top(&mut self) {
        self.log_scroll = self.stripped_line_count;
    }

    /// Scroll to bottom (follow).
    pub fn scroll_to_bottom(&mut self) {
        self.log_scroll = 0;
    }

    pub fn clamp_cursor(&mut self) {
        let len = self.current_list().len();
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

    pub fn do_start(&mut self) {
        let item = match self.current_item() {
            Some(i) => i.to_string(),
            None => return,
        };
        let ok = self.run_tncli_cmd(&["start", &item]);
        self.refresh_status();
        let msg = if ok { format!("started: {item}") } else { format!("error starting {item}") };
        self.set_message(&msg);
    }

    pub fn do_stop(&mut self) {
        let item = match self.current_item() {
            Some(i) => i.to_string(),
            None => return,
        };
        let ok = self.run_tncli_cmd(&["stop", &item]);
        self.refresh_status();
        let msg = if ok { format!("stopped: {item}") } else { format!("error stopping {item}") };
        self.set_message(&msg);
    }

    pub fn do_stop_all(&mut self) {
        let ok = self.run_tncli_cmd(&["stop"]);
        self.refresh_status();
        self.set_message(if ok { "stopped all services" } else { "error stopping all" });
    }

    pub fn do_restart(&mut self) {
        let item = match self.current_item() {
            Some(i) => i.to_string(),
            None => return,
        };
        let ok = self.run_tncli_cmd(&["restart", &item]);
        self.refresh_status();
        let msg = if ok { format!("restarted: {item}") } else { format!("error restarting {item}") };
        self.set_message(&msg);
    }

    pub fn do_toggle(&mut self) {
        let item = match self.current_item() {
            Some(i) => i.to_string(),
            None => return,
        };
        match self.section {
            Section::Services => {
                if self.is_running(&item) { self.do_stop(); } else { self.do_start(); }
            }
            Section::Combos => {
                let any_running = self.config.combinations.get(&item)
                    .map(|svcs| svcs.iter().any(|s| self.is_running(s)))
                    .unwrap_or(false);
                if any_running { self.do_stop(); } else { self.do_start(); }
            }
        }
    }
}
