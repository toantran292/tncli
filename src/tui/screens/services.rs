use crate::tmux;

use crate::tui::app::{App, ComboItem, workspace_branch};

impl App {
    /// Start a service using worktree info. Works for both main (virtual worktree) and real worktrees.
    fn start_service_with_info(&mut self, parent_dir: &str, svc: &str, wt: &crate::services::WorktreeInfo, tmux_name: &str) {
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
        full_cmd.push_str(&format!(" && export BIND_IP={}", wt.bind_ip));
        if let Some(wt_cfg) = self.config.repos.get(parent_dir).and_then(|d| d.wt()) {
            let branch_safe = crate::services::branch_safe(&wt.branch);
            let ws_key = format!("ws-{}", wt.branch.replace('/', "-"));
            for (k, v) in &wt_cfg.env {
                let val = v.replace("{{bind_ip}}", &wt.bind_ip)
                    .replace("{{branch_safe}}", &branch_safe)
                    .replace("{{branch}}", &wt.branch);
                let val = crate::services::resolve_slot_templates(&val, &ws_key);
                full_cmd.push_str(&format!(" && export {}='{}'", k, val));
            }
        }
        full_cmd.push_str(&format!(" && {cmd}"));
        if let Some(e) = env {
            full_cmd = format!("{e} {full_cmd}");
        }

        // Mark as starting immediately for UI feedback
        self.starting_services.insert(tmux_name.to_string());

        // Ensure shared services + config applied in background, then start
        let session = self.session.clone();
        let config = self.config.clone();
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
        let dir_name = parent_dir.to_string();
        let tmux = tmux_name.to_string();
        let wt_clone = wt.clone();
        std::thread::spawn(move || {
            ensure_main_ready_sync(&config, &config_dir, &dir_name, &wt_clone);
            tmux::create_session_if_needed(&session);
            tmux::new_window(&session, &tmux, &full_cmd);
        });

        self.set_message(&format!("starting: {tmux_name}..."));
    }

    fn start_wt_service(&mut self, parent_dir: &str, svc: &str, wt_key: &str, tmux_name: &str) {
        let wt = match self.worktrees.get(wt_key) {
            Some(w) => w.clone(),
            None => { self.set_message("worktree not found"); return; }
        };
        self.start_service_with_info(parent_dir, svc, &wt, tmux_name);
    }

    fn start_main_service(&mut self, dir_name: &str, svc_name: &str) {
        let dir_path = match self.dir_path(dir_name) {
            Some(p) => p,
            None => { self.set_message("dir not found"); return; }
        };
        let alias = self.config.repos.get(dir_name)
            .and_then(|d| d.alias.as_deref())
            .unwrap_or(dir_name);
        let tmux_name = format!("{alias}~{svc_name}");
        let branch = self.dir_branch(dir_name).unwrap_or_else(|| "main".to_string());

        // Main = virtual worktree with allocated main IP
        let wt = crate::services::WorktreeInfo {
            branch,
            parent_dir: dir_name.to_string(),
            bind_ip: self.main_bind_ip.clone(),
            path: std::path::PathBuf::from(&dir_path),
        };
        self.start_service_with_info(dir_name, svc_name, &wt, &tmux_name);
    }

    pub fn do_start(&mut self) {
        if let Some(item) = self.current_combo_item().cloned() {
            match item {
                ComboItem::Combo(_) => {
                    self.set_message("select a workspace instance first");
                    return;
                }
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
            }
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
                let alias = self.config.repos.get(&dir)
                    .and_then(|d| d.alias.as_deref())
                    .unwrap_or(dir.as_str());
                let tmux_name = format!("{alias}~{svc}");
                if !self.is_running(&tmux_name) {
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
            let alias = dir.alias.as_deref().unwrap_or(dir_name);
            for svc_name in dir.services.keys() {
                let tmux_name = format!("{alias}~{svc_name}");
                if !self.is_running(&tmux_name) {
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
        for s in &svcs { self.stopping_services.insert(s.clone()); }
        self.set_message(&format!("stopping {} main services...", svcs.len()));
        let session = self.session.clone();
        std::thread::spawn(move || {
            for svc in &svcs { crate::tmux::graceful_stop(&session, svc); }
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
            Some(ComboItem::Combo(_)) => {
                self.toggle_collapse();
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

/// Ensure shared services running + compose override + env applied for a dir.
/// Called from background thread before starting service. Idempotent.
fn ensure_main_ready_sync(
    config: &crate::config::Config,
    config_dir: &std::path::Path,
    dir_name: &str,
    wt: &crate::services::WorktreeInfo,
) {
    let wt_cfg = match config.repos.get(dir_name).and_then(|d| d.wt()) {
        Some(wt) => wt.clone(),
        None => return,
    };

    // Start shared services if needed
    if !config.shared_services.is_empty() {
        let mut needed: Vec<String> = Vec::new();
        for sref in &wt_cfg.shared_services {
            if !needed.contains(&sref.name) { needed.push(sref.name.clone()); }
        }
        if !needed.is_empty() {
            crate::services::generate_shared_compose(config_dir, &config.session, &config.shared_services);
            let refs: Vec<&str> = needed.iter().map(|s| s.as_str()).collect();
            crate::services::start_shared_services(config_dir, &config.session, &refs);
        }
    }

    // Apply compose override + env files
    let p = &wt.path;
    let (svc_overrides, shared_hosts) = crate::pipeline::context::resolve_shared_overrides(config, dir_name);
    let compose_files = if wt_cfg.compose_files.is_empty() && p.join("docker-compose.yml").is_file() {
        vec!["docker-compose.yml".to_string()]
    } else {
        wt_cfg.compose_files.clone()
    };
    let ws_key = format!("ws-{}", wt.branch.replace('/', "-"));
    if !compose_files.is_empty() {
        crate::services::generate_compose_override(
            p, p, &wt.bind_ip, &compose_files, &wt_cfg.env, &wt.branch, None,
            if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
            &shared_hosts, &ws_key,
        );
    }

    wt_cfg.apply_all_env_files(p, &wt.bind_ip, &wt.branch, &ws_key);
    let _ = crate::services::write_env_file(p, &wt.bind_ip);

    let branch_safe = crate::services::branch_safe(&wt.branch);
    // Create DBs if needed (batch — single container for all DBs)
    let mut db_names = Vec::new();
    let mut db_host = "localhost";
    let mut db_port = 5432u16;
    let mut db_user = "postgres";
    let mut db_pw = "postgres";
    for sref in &wt_cfg.shared_services {
        if let Some(db_tpl) = &sref.db_name {
            let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                .replace("{{branch}}", &wt.branch);
            let svc_def = config.shared_services.get(&sref.name);
            db_host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost");
            db_port = svc_def.and_then(|d| d.ports.first())
                .and_then(|p| p.split(':').next())
                .and_then(|p| p.parse().ok())
                .unwrap_or(5432);
            db_user = svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres");
            db_pw = svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres");
            db_names.push(db_name);
        }
    }
    if !db_names.is_empty() {
        crate::services::create_shared_dbs_batch(db_host, db_port, &db_names, db_user, db_pw);
    }

    crate::services::ensure_global_gitignore();
}
