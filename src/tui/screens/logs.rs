use std::time::Instant;

use crate::tui::app::{App, ComboItem, workspace_branch};

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
                    // Include setup~ and pipeline~ windows for creating/deleting workspaces
                    let branch_safe = crate::services::branch_safe(branch);
                    let suffix = format!("~{branch_safe}");
                    for win in &self.running_windows {
                        if (win.starts_with("setup~") || win.starts_with("pipeline~")) && win.ends_with(&suffix) && !svcs.contains(win) {
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

    pub fn current_running_services(&self) -> Vec<String> {
        self.combo_running_services()
    }

    pub fn log_service_name(&self) -> Option<String> {
        let running: Vec<String> = self.current_running_services()
            .into_iter()
            .filter(|s| !self.stopping_services.contains(s))
            .collect();
        if running.is_empty() {
            return None;
        }
        let idx = self.combo_log_idx % running.len();
        Some(running[idx].clone())
    }

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
        self.swap_pending = true;
    }

    pub fn set_message(&mut self, msg: &str) {
        self.message = msg.to_string();
        self.message_time = Some(Instant::now());
        // Also show in tmux status line (wider, more visible)
        crate::tmux::display_message(&format!("[tncli] {msg}"));
    }

    pub fn get_message(&self) -> &str {
        if let Some(t) = self.message_time {
            if t.elapsed().as_secs() < 4 {
                return &self.message;
            }
        }
        ""
    }
}
