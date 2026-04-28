use std::time::Instant;

use ratatui::text::Line;

use crate::tmux;
use crate::tui::app::{App, ComboItem, strip_ansi, workspace_branch};
use crate::tui::ansi::parse_ansi_line_with_search;

impl App {
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
                        .map(|(dir, svc)| {
                            let alias = self.config.repos.get(&dir)
                                .and_then(|d| d.alias.as_deref())
                                .unwrap_or(dir.as_str());
                            format!("{alias}~{svc}")
                        })
                        .filter(|tmux_name| self.is_running(tmux_name))
                }).collect()
            }
            Some(ComboItem::Instance { branch, is_main }) => {
                if *is_main {
                    let combo_name = self.find_parent_combo(self.cursor);
                    let all_ws = self.config.all_workspaces();
                    let entries = match all_ws.get(&combo_name) {
                        Some(e) => e,
                        None => return Vec::new(),
                    };
                    entries.iter().filter_map(|entry| {
                        self.config.find_service_entry_quiet(entry)
                            .map(|(dir, svc)| {
                                let alias = self.config.repos.get(&dir)
                                    .and_then(|d| d.alias.as_deref())
                                    .unwrap_or(dir.as_str());
                                format!("{alias}~{svc}")
                            })
                            .filter(|tmux_name| self.is_running(tmux_name))
                    }).collect()
                } else {
                    let branch_safe = branch.replace('/', "-");
                    let mut svcs: Vec<String> = self.worktrees.values()
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
                        .collect();
                    // Also include setup~ tmux windows for creating workspaces
                    let setup_prefix = format!("setup~");
                    let setup_suffix = format!("~{branch_safe}");
                    for win in &self.running_windows {
                        if win.starts_with(&setup_prefix) && win.ends_with(&setup_suffix) && !svcs.contains(win) {
                            svcs.push(win.clone());
                        }
                    }
                    svcs
                }
            }
            Some(ComboItem::InstanceDir { branch, dir, is_main, .. }) => {
                if *is_main {
                    let alias = self.config.repos.get(dir)
                        .and_then(|d| d.alias.as_deref())
                        .unwrap_or(dir.as_str());
                    self.config.repos.get(dir)
                        .map(|d| d.services.keys()
                            .map(|s| format!("{alias}~{s}"))
                            .filter(|tmux_name| self.is_running(tmux_name))
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
}
