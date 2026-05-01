use crate::tui::app::{App, ComboItem, workspace_branch};

impl App {
    pub fn do_stop(&mut self) {
        if let Some(item) = self.current_combo_item().cloned() {
            match item {
                ComboItem::Combo(_) => {
                    self.set_message("select a workspace instance first");
                    return;
                }
                ComboItem::InstanceService { tmux_name, .. } => {
                    if !self.is_running(&tmux_name) {
                        self.set_message("nothing to stop");
                        return;
                    }
                    self.stopping_services.insert(tmux_name.clone());
                    self.set_message(&format!("stopping: {tmux_name}..."));
                    self.unjoin_if_displayed(&tmux_name);
                    let svc_session = self.svc_session();
                    std::thread::spawn(move || {
                        crate::tmux::graceful_stop(&svc_session, &tmux_name);
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
                    if let Some(svc_name) = dir.strip_prefix("_global:") {
                        // Stop global service
                        let tmux_name = if is_main {
                            format!("_global~{svc_name}")
                        } else {
                            let bs = crate::services::branch_safe(&branch);
                            format!("_global~{svc_name}~{bs}")
                        };
                        if self.is_running(&tmux_name) {
                            self.unjoin_if_displayed(&tmux_name);
                            self.stopping_services.insert(tmux_name.clone());
                            let svc_session = self.svc_session();
                            std::thread::spawn(move || {
                                crate::tmux::graceful_stop(&svc_session, &tmux_name);
                            });
                            self.set_message(&format!("stopping: {svc_name}"));
                        }
                    } else if is_main {
                        self.stop_main_dir(&dir);
                    } else {
                        self.stop_wt_dir(&dir, &branch);
                    }
                    return;
                }
            }
        }
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
            .filter_map(|entry| {
                let (dir, svc) = self.config.find_service_entry_quiet(entry)?;
                let alias = self.config.repos.get(&dir)
                    .and_then(|d| d.alias.as_deref())
                    .unwrap_or(dir.as_str());
                let tmux_name = format!("{alias}~{svc}");
                if self.is_running(&tmux_name) { Some(tmux_name) } else { None }
            })
            .collect();
        if svcs.is_empty() {
            self.set_message("nothing to stop");
            return;
        }
        for s in &svcs {
            self.stopping_services.insert(s.clone());
            self.unjoin_if_displayed(s);
        }
        self.set_message(&format!("stopping {} main services...", svcs.len()));
        let svc_session = self.svc_session();
        std::thread::spawn(move || {
            for svc in &svcs { crate::tmux::graceful_stop(&svc_session, svc); }
        });
    }

    /// Stop all main services in a specific dir.
    fn stop_main_dir(&mut self, dir_name: &str) {
        let alias = self.config.repos.get(dir_name)
            .and_then(|d| d.alias.as_deref())
            .unwrap_or(dir_name);
        let svcs: Vec<String> = self.config.repos.get(dir_name)
            .map(|d| d.services.keys()
                .map(|s| format!("{alias}~{s}"))
                .filter(|tmux_name| self.is_running(tmux_name))
                .collect())
            .unwrap_or_default();
        if svcs.is_empty() {
            self.set_message("nothing to stop");
            return;
        }
        for s in &svcs {
            self.stopping_services.insert(s.clone());
            self.unjoin_if_displayed(s);
        }
        self.set_message(&format!("stopping {} services for {dir_name}...", svcs.len()));
        let svc_session = self.svc_session();
        std::thread::spawn(move || {
            for svc in &svcs { crate::tmux::graceful_stop(&svc_session, svc); }
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
        for s in &svcs {
            self.stopping_services.insert(s.clone());
            self.unjoin_if_displayed(s);
        }
        self.set_message(&format!("stopping {} services for workspace {branch}...", svcs.len()));
        let svc_session = self.svc_session();
        std::thread::spawn(move || {
            for svc in &svcs { crate::tmux::graceful_stop(&svc_session, svc); }
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
        for s in &svcs {
            self.stopping_services.insert(s.clone());
            self.unjoin_if_displayed(s);
        }
        self.set_message(&format!("stopping {} services for {dir_name}~{branch}...", svcs.len()));
        let svc_session = self.svc_session();
        std::thread::spawn(move || {
            for svc in &svcs { crate::tmux::graceful_stop(&svc_session, svc); }
        });
    }
}
