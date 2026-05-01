use crate::tmux;

use crate::tui::app::{App, ComboItem, workspace_branch};

impl App {
    /// If service is currently displayed in right pane, swap blank back first.
    /// Returns the service name (for kill in background thread).
    pub(crate) fn unjoin_if_displayed(&mut self, tmux_name: &str) {
        if self.joined_service.as_deref() != Some(tmux_name) {
            return;
        }
        let svc_sess = self.svc_session();
        if let Some(ref rpid) = self.right_pane_id {
            // Swap service back to its window
            if tmux::window_exists(&svc_sess, tmux_name) {
                let _ = tmux::swap_pane(&svc_sess, tmux_name, rpid);
                self.joined_service = None;
                self.redetect_right_pane();
                if let Some(ref rpid) = self.right_pane_id {
                    tmux::set_pane_title(rpid, "service");
                }
            }
        }
    }
    /// Start a service using worktree info. Works for both main (virtual worktree) and real worktrees.
    fn start_service_with_info(&mut self, parent_dir: &str, svc: &str, wt: &crate::services::WorktreeInfo, tmux_name: &str) {
        let dir = match self.config.repos.get(parent_dir) {
            Some(d) => d,
            None => { self.set_message("dir not found"); return; }
        };

        // Check repo service first, then global service
        let (cmd, is_global_wt_level) = if let Some(service) = dir.services.get(svc) {
            match &service.cmd {
                Some(c) => (c.clone(), false),
                None => { self.set_message("no cmd defined"); return; }
            }
        } else if let Some(gs) = self.config.global_services.get(svc) {
            (gs.cmd.clone(), gs.worktree_level)
        } else {
            self.set_message("service not found");
            return;
        };

        if tmux::window_exists(&self.svc_session(), tmux_name) {
            self.set_message(&format!("{tmux_name} already running"));
            return;
        }

        // Global worktree_level: cd into workspace dir, not repo dir
        let wt_dir = if is_global_wt_level {
            wt.path.parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| wt.path.to_string_lossy().to_string())
        } else {
            wt.path.to_string_lossy().to_string()
        };
        let service = dir.services.get(svc);
        let pre_start = service.and_then(|s| s.pre_start.as_deref()).or(dir.pre_start.as_deref());
        let env = service.and_then(|s| s.env.as_deref()).or(dir.env.as_deref());

        let mut full_cmd = format!("cd '{wt_dir}'");
        if let Some(pre) = pre_start {
            full_cmd.push_str(&format!(" && {pre}"));
        }
        full_cmd.push_str(&format!(" && export BIND_IP={}", wt.bind_ip));
        // Inject node-bind-host.js: DNS via dnsmasq + BIND_IP patching
        let home = std::env::var("HOME").unwrap_or_default();
        let patch = format!("{home}/.tncli/node-bind-host.js");
        if std::path::Path::new(&patch).exists() {
            full_cmd.push_str(&format!(" && export NODE_OPTIONS=\"--dns-result-order=ipv4first --require {patch} ${{NODE_OPTIONS:-}}\""));
        }
        if let Some(wt_cfg) = self.config.repos.get(parent_dir).and_then(|d| d.wt()) {
            // Use workspace branch (from parent folder) not git branch
            let ws_branch = crate::tui::app::workspace_branch(wt)
                .unwrap_or_else(|| wt.branch.clone());
            let branch_safe = crate::services::branch_safe(&ws_branch);
            let ws_key = format!("ws-{}", ws_branch.replace('/', "-"));
            // Pre-resolve database names for {{db:N}}
            let db_names: Vec<String> = wt_cfg.databases.iter()
                .map(|tpl| {
                    let name = tpl.replace("{{branch_safe}}", &branch_safe).replace("{{branch}}", &ws_branch);
                    format!("{}_{name}", self.config.session)
                })
                .collect();
            // Global env → worktree env (worktree wins)
            let mut merged_env = self.config.env.clone();
            for (k, v) in &wt_cfg.env {
                merged_env.insert(k.clone(), v.clone());
            }
            for (k, v) in &merged_env {
                // Skip frontend env prefixes — let Vite/Next/CRA read from .env.local instead
                // (TUI uses git branch for branch_safe, but .env.local uses workspace branch — may differ)
                if k.starts_with("VITE_") || k.starts_with("NEXT_PUBLIC_") || k.starts_with("REACT_APP_") {
                    continue;
                }
                let val = v.replace("{{bind_ip}}", &wt.bind_ip)
                    .replace("{{branch_safe}}", &branch_safe)
                    .replace("{{branch}}", &ws_branch);
                let val = crate::services::resolve_slot_templates(&val, &ws_key);
                let val = crate::services::resolve_config_templates(&val, &self.config, &branch_safe);
                let val = crate::services::resolve_db_templates(&val, &db_names);
                full_cmd.push_str(&format!(" && export {}='{}'", k, val));
            }
            // Per-service env (overrides worktree env)
            for (k, v) in service.map(|s| &s.env_vars).unwrap_or(&indexmap::IndexMap::new()) {
                let val = v.replace("{{bind_ip}}", &wt.bind_ip)
                    .replace("{{branch_safe}}", &branch_safe)
                    .replace("{{branch}}", &ws_branch);
                let val = crate::services::resolve_slot_templates(&val, &ws_key);
                let val = crate::services::resolve_config_templates(&val, &self.config, &branch_safe);
                let val = crate::services::resolve_db_templates(&val, &db_names);
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
        let svc_session = self.svc_session();
        let config = self.config.clone();
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
        let dir_name = parent_dir.to_string();
        let tmux = tmux_name.to_string();
        let wt_clone = wt.clone();
        std::thread::spawn(move || {
            ensure_main_ready_sync(&config, &config_dir, &dir_name, &wt_clone);
            tmux::create_session_if_needed(&svc_session);
            tmux::new_window(&svc_session, &tmux, &full_cmd);
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

    /// Recreate databases for the selected workspace instance.
    pub fn do_recreate_db(&mut self) {
        let item = match self.current_combo_item().cloned() {
            Some(i) => i,
            None => return,
        };

        // Get workspace branch from any instance item
        let (branch, is_main) = match &item {
            ComboItem::Instance { branch, is_main } => (branch.clone(), *is_main),
            ComboItem::InstanceDir { branch, is_main, .. } => (branch.clone(), *is_main),
            ComboItem::InstanceService { branch, is_main, .. } => (branch.clone(), *is_main),
            _ => { self.set_message("select a workspace instance"); return; }
        };

        let ws_branch = if is_main {
            self.config.global_default_branch().to_string()
        } else {
            branch
        };
        let branch_safe = crate::services::branch_safe(&ws_branch);

        // Collect all databases from all repos
        let pg_svc = self.config.shared_services.values().find(|s| s.db_user.is_some());
        let host = self.config.shared_host("postgres");
        let host_str: &str = pg_svc.and_then(|s| s.host.as_deref()).unwrap_or(&host);
        let port: u16 = pg_svc.and_then(|s| s.ports.first())
            .and_then(|p| p.split(':').next()).and_then(|p| p.parse().ok()).unwrap_or(5432);
        let user = pg_svc.and_then(|s| s.db_user.as_deref()).unwrap_or("postgres");
        let pw = pg_svc.and_then(|s| s.db_password.as_deref()).unwrap_or("postgres");

        let mut db_names = Vec::new();
        for (_, dir) in &self.config.repos {
            if let Some(wt) = dir.wt() {
                for sref in &wt.shared_services {
                    if let Some(db_tpl) = &sref.db_name {
                        let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                            .replace("{{branch}}", &ws_branch);
                        db_names.push(db_name);
                    }
                }
                for db_tpl in &wt.databases {
                    let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                        .replace("{{branch}}", &ws_branch);
                    db_names.push(format!("{}_{db_name}", self.config.session));
                }
            }
        }

        if db_names.is_empty() {
            self.set_message("no databases configured");
            return;
        }

        let count = db_names.len();
        let host_owned = host_str.to_string();
        let user_owned = user.to_string();
        let pw_owned = pw.to_string();
        std::thread::spawn(move || {
            crate::services::drop_shared_dbs_batch(&host_owned, port, &db_names, &user_owned, &pw_owned);
            crate::services::create_shared_dbs_batch(&host_owned, port, &db_names, &user_owned, &pw_owned);
        });

        self.set_message(&format!("recreating {count} databases for {ws_branch}..."));
    }

    /// Open the selected service's proxy URL in browser.
    pub fn do_open_url(&mut self) {
        let item = match self.current_combo_item().cloned() {
            Some(i) => i,
            None => return,
        };
        let (dir_name, svc_name, branch, is_main) = match &item {
            ComboItem::InstanceService { dir, svc, branch, is_main, .. } => {
                (dir.clone(), Some(svc.clone()), branch.clone(), *is_main)
            }
            ComboItem::InstanceDir { dir, branch, is_main, .. } => {
                (dir.clone(), None, branch.clone(), *is_main)
            }
            _ => { self.set_message("select a service to open"); return; }
        };

        let ws_branch = if is_main {
            self.config.global_default_branch().to_string()
        } else {
            branch
        };
        // Find proxy_port: per-service first, then repo-level
        let dir = self.config.repos.get(&dir_name);
        let port = svc_name.as_ref()
            .and_then(|svc| dir?.services.get(svc)?.proxy_port)
            .or_else(|| dir?.proxy_port);

        let Some(port) = port else {
            self.set_message("no proxy_port configured");
            return;
        };

        // Use bind_ip for browser (secure context — crypto.subtle works)
        // Hostname is for server-to-server only (proxy rewrites Host header)
        let bind_ip = if is_main {
            &self.main_bind_ip
        } else {
            let ws_key = format!("ws-{}", ws_branch.replace('/', "-"));
            let allocs = crate::services::load_ip_allocations();
            let ip = allocs.get(&ws_key).cloned().unwrap_or_else(|| self.main_bind_ip.clone());
            // Leak into 'static to avoid borrow issues — fine for one-shot URL
            return {
                let url = format!("http://{}:{port}", ip);
                let _ = std::process::Command::new("open").arg(&url).spawn();
                self.set_message(&format!("opening {url}"));
            };
        };

        let url = format!("http://{bind_ip}:{port}");
        let _ = std::process::Command::new("open").arg(&url).spawn();
        self.set_message(&format!("opening {url}"));
    }

    /// Start a worktree-level global service at the workspace dir.
    fn start_global_service(&mut self, svc_name: &str, branch: &str, is_main: bool) {
        let gs = match self.config.global_services.get(svc_name) {
            Some(g) => g.clone(),
            None => { self.set_message(&format!("global service '{svc_name}' not found")); return; }
        };

        let tmux_name = if is_main {
            format!("_global~{svc_name}")
        } else {
            let bs = crate::services::branch_safe(branch);
            format!("_global~{svc_name}~{bs}")
        };

        if tmux::window_exists(&self.svc_session(), &tmux_name) {
            self.set_message(&format!("{svc_name} already running"));
            return;
        }

        // Determine workspace dir
        let ws_dir = if is_main {
            self.main_workspace_dir().to_string_lossy().to_string()
        } else {
            let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
            config_dir.join(format!("workspace--{branch}")).to_string_lossy().to_string()
        };

        let full_cmd = format!("cd '{}' && {}", ws_dir, gs.cmd);
        let svc_session = self.svc_session();
        let tmux_clone = tmux_name.clone();
        std::thread::spawn(move || {
            tmux::create_session_if_needed(&svc_session);
            tmux::new_window(&svc_session, &tmux_clone, &full_cmd);
        });
        self.starting_services.insert(tmux_name.clone());
        self.set_message(&format!("starting: {svc_name}"));
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
                    if let Some(svc_name) = dir.strip_prefix("_global:") {
                        // Worktree-level global service
                        self.start_global_service(svc_name, &branch, is_main);
                    } else if is_main {
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

    pub fn do_restart(&mut self) {
        match self.current_combo_item().cloned() {
            Some(ComboItem::InstanceService { tmux_name, .. }) => {
                self.restart_service(&tmux_name);
                self.do_start();
                self.swap_pending = true;
                self.set_message(&format!("restarting: {tmux_name}"));
            }
            Some(ComboItem::InstanceDir { .. }) => {
                // Restart all running services in this dir
                let running = self.current_running_services();
                let svc_sess = self.svc_session();
                for svc in &running {
                    self.unjoin_if_displayed(svc);
                    tmux::graceful_stop(&svc_sess, svc);
                    self.stopping_services.remove(svc);
                    self.running_windows.remove(svc);
                }
                if self.joined_service.as_ref().is_some_and(|j| running.contains(j)) {
                    self.joined_service = None;
                }
                self.do_start();
                self.swap_pending = true;
                self.set_message(&format!("restarting {} services...", running.len()));
            }
            Some(ComboItem::Instance { .. }) => {
                let running = self.current_running_services();
                let svc_sess = self.svc_session();
                for svc in &running {
                    self.unjoin_if_displayed(svc);
                    tmux::graceful_stop(&svc_sess, svc);
                    self.stopping_services.remove(svc);
                    self.running_windows.remove(svc);
                }
                if self.joined_service.as_ref().is_some_and(|j| running.contains(j)) {
                    self.joined_service = None;
                }
                self.do_start();
                self.swap_pending = true;
                self.set_message(&format!("restarting {} services...", running.len()));
            }
            _ => {}
        }
    }

    fn restart_service(&mut self, tmux_name: &str) {
        self.unjoin_if_displayed(tmux_name);
        if self.is_running(tmux_name) {
            let svc_sess = self.svc_session();
            tmux::graceful_stop(&svc_sess, tmux_name);
        }
        self.stopping_services.remove(tmux_name);
        self.running_windows.remove(tmux_name);
        if self.joined_service.as_deref() == Some(tmux_name) {
            self.joined_service = None;
        }
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
    // Use workspace branch (from parent folder) not git branch — they may differ
    let ws_branch = crate::tui::app::workspace_branch(wt)
        .unwrap_or_else(|| wt.branch.clone());
    let ws_key = format!("ws-{}", ws_branch.replace('/', "-"));
    if !compose_files.is_empty() {
        crate::services::generate_compose_override(
            p, p, &wt.bind_ip, &compose_files, &wt_cfg.env, &ws_branch, None,
            if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
            &shared_hosts, &ws_key, &config, &wt_cfg.databases,
        );
    }

    wt_cfg.apply_all_env_files(p, &config, &wt.bind_ip, &ws_branch, &ws_key);
    let _ = crate::services::write_env_file(p, &wt.bind_ip);

    let branch_safe = crate::services::branch_safe(&ws_branch);
    // Create DBs if needed (batch — single container for all DBs)
    let mut db_names = Vec::new();
    let mut db_host = "localhost";
    let mut db_port = 5432u16;
    let mut db_user = "postgres";
    let mut db_pw = "postgres";
    for sref in &wt_cfg.shared_services {
        if let Some(db_tpl) = &sref.db_name {
            let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                .replace("{{branch}}", &ws_branch);
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
    // New: databases field (auto-prefixed with {session}_)
    for db_tpl in &wt_cfg.databases {
        let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
            .replace("{{branch}}", &ws_branch);
        db_names.push(format!("{}_{db_name}", config.session));
        if let Some(pg) = config.shared_services.values().find(|s| s.db_user.is_some()) {
            db_host = pg.host.as_deref().unwrap_or("localhost");
            db_port = pg.ports.first().and_then(|p| p.split(':').next()).and_then(|p| p.parse().ok()).unwrap_or(5432);
            db_user = pg.db_user.as_deref().unwrap_or("postgres");
            db_pw = pg.db_password.as_deref().unwrap_or("postgres");
        }
    }
    if !db_names.is_empty() {
        crate::services::create_shared_dbs_batch(db_host, db_port, &db_names, db_user, db_pw);
    }

    crate::services::ensure_global_gitignore();
}
