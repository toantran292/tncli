use crate::tmux;
use crate::tui::app::{App, ComboItem};

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
pub(super) fn ensure_main_ready_sync(
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
