use crate::tmux;
use crate::tui::app::{App, workspace_branch};

impl App {
    /// Start a service using worktree info. Works for both main (virtual worktree) and real worktrees.
    fn start_service_with_info(&mut self, parent_dir: &str, svc: &str, wt: &crate::services::WorktreeInfo, tmux_name: &str) {
        let dir = match self.config.repos.get(parent_dir) {
            Some(d) => d,
            None => { self.set_message("dir not found"); return; }
        };

        // Check repo service first, then global service
        let (cmd, _is_global_wt_level) = if let Some(service) = dir.services.get(svc) {
            match &service.cmd {
                Some(c) => (c.clone(), false),
                None => { self.set_message("no cmd defined"); return; }
            }
        } else if let Some(gs) = self.config.global_services.get(svc) {
            (gs.cmd.clone(), false) // repo context: always use repo dir, not workspace dir
        } else {
            self.set_message("service not found");
            return;
        };

        if tmux::window_exists(&self.svc_session(), tmux_name) {
            self.set_message(&format!("{tmux_name} already running"));
            return;
        }

        let wt_dir = wt.path.to_string_lossy().to_string();
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
            super::svc_actions::ensure_main_ready_sync(&config, &config_dir, &dir_name, &wt_clone);
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
            // autoclose: when global service exits, window closes (no "press enter")
            tmux::new_window_autoclose(&svc_session, &tmux_clone, &full_cmd);
        });
        self.starting_services.insert(tmux_name.clone());
        self.set_message(&format!("starting: {svc_name}"));
    }

    pub fn do_start(&mut self) {
        if let Some(item) = self.current_combo_item().cloned() {
            match item {
                crate::tui::app::ComboItem::Combo(_) => {
                    self.set_message("select a workspace instance first");
                    return;
                }
                crate::tui::app::ComboItem::InstanceService { dir, svc, wt_key, tmux_name, is_main, .. } => {
                    if is_main {
                        self.start_main_service(&dir, &svc);
                    } else {
                        self.start_wt_service(&dir, &svc, &wt_key, &tmux_name);
                    }
                    return;
                }
                crate::tui::app::ComboItem::Instance { branch, is_main } => {
                    if is_main {
                        self.start_main_instance();
                    } else {
                        self.start_workspace_instance(&branch);
                    }
                    return;
                }
                crate::tui::app::ComboItem::InstanceDir { branch, dir, wt_key, is_main } => {
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
}
