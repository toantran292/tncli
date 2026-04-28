use crate::tmux;

use super::app::{App, ComboItem, workspace_branch};

impl App {
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
