use std::path::PathBuf;

use crate::tui::app::{App, ComboItem};

impl App {
    /// Create a worktree for a dir.
    pub fn create_worktree(&mut self, dir_name: &str, branch: &str) -> String {
        let dir_path = match self.dir_path(dir_name) {
            Some(p) => p,
            None => return "dir not found".to_string(),
        };
        let copy_files = self.config.repos.get(dir_name)
            .and_then(|d| d.wt())
            .map(|wt| wt.copy.clone())
            .unwrap_or_default();
        match crate::services::create_worktree(std::path::Path::new(&dir_path), branch, &copy_files) {
            Ok(wt_path) => {
                let wt_key = format!("{dir_name}--{}", branch.replace('/', "-"));
                let ip = crate::services::allocate_ip(&wt_key);
                // Generate .env.tncli + docker-compose.override.yml
                let _ = crate::services::write_env_file(&wt_path, &ip);
                let repo_dir = std::path::Path::new(&dir_path);
                let dir_cfg = self.config.repos.get(dir_name);
                let wt_cfg = dir_cfg.and_then(|d| d.wt());
                let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
                let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
                crate::services::generate_compose_override(repo_dir, &wt_path, &ip, &compose_files, &worktree_env, branch, None, None, &[]);
                // Ensure docker-compose.override.yml is globally gitignored
                crate::services::ensure_global_gitignore();
                self.worktrees.insert(wt_key.clone(), crate::services::WorktreeInfo {
                    branch: branch.to_string(),
                    parent_dir: dir_name.to_string(),
                    bind_ip: ip.clone(),
                    path: wt_path,
                });
                self.rebuild_combo_tree();
                format!("worktree created: {branch} (BIND_IP={ip}). Run: sudo ifconfig lo0 alias {ip}")
            }
            Err(e) => format!("worktree failed: {e}"),
        }
    }

    /// Delete a worktree.
    pub fn delete_worktree(&mut self, wt_key: &str) -> String {
        let wt = match self.worktrees.remove(wt_key) {
            Some(w) => w,
            None => return "worktree not found".to_string(),
        };
        let dir_path = match self.dir_path(&wt.parent_dir) {
            Some(p) => p,
            None => return "parent dir not found".to_string(),
        };

        // Stop tmux services immediately (fast)
        if let Some(dir) = self.config.repos.get(&wt.parent_dir) {
            for svc_name in dir.services.keys() {
                let tmux_name = self.wt_tmux_name(&wt.parent_dir, svc_name, &wt.branch);
                if self.is_running(&tmux_name) {
                    crate::tmux::graceful_stop(&self.session, &tmux_name);
                }
            }
        }

        // Release IP allocation
        if !wt.bind_ip.is_empty() {
            crate::services::release_ip(wt_key);
        }

        // Update UI immediately
        self.wt_collapsed.remove(wt_key);
        self.rebuild_combo_tree();
        let branch = wt.branch.clone();

        // Heavy cleanup in background (docker down, git worktree remove)
        let pre_delete = self.config.repos.get(&wt.parent_dir)
            .and_then(|d| d.wt())
            .map(|wt| wt.pre_delete.clone())
            .unwrap_or_default();
        let wt_path = wt.path.clone();
        let repo_dir = dir_path.clone();
        let wt_branch = wt.branch.clone();
        std::thread::spawn(move || {
            // Pre-delete commands (e.g. dip compose down)
            if !pre_delete.is_empty() && wt_path.exists() {
                let combined = pre_delete.join(" && ");
                let _ = std::process::Command::new("zsh")
                    .args(["-lc", &combined])
                    .current_dir(&wt_path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            // Remove git worktree + cleanup
            let _ = crate::services::remove_worktree(
                std::path::Path::new(&repo_dir), &wt_path, &wt_branch,
            );
        });

        format!("deleting worktree: {branch}...")
    }

    /// Setup main dir as worktree-like environment with 127.0.0.1 binding.
    pub fn setup_main_loopback(&mut self, dir_name: &str) -> String {
        let dir_path = match self.dir_path(dir_name) {
            Some(p) => p,
            None => return "dir not found".to_string(),
        };
        let wt_cfg = match self.config.repos.get(dir_name).and_then(|d| d.wt()) {
            Some(wt) => wt.clone(),
            None => return format!("worktree not configured for {dir_name}"),
        };
        let p = std::path::Path::new(&dir_path);
        let branch = self.dir_branch(dir_name).unwrap_or_else(|| "main".to_string());
        let (svc_overrides, shared_hosts) = self.resolve_shared_overrides(dir_name);
        let compose_files = if wt_cfg.compose_files.is_empty() && p.join("docker-compose.yml").is_file() {
            vec!["docker-compose.yml".to_string()]
        } else {
            wt_cfg.compose_files.clone()
        };
        if !compose_files.is_empty() {
            crate::services::setup_main_as_worktree(
                p, &compose_files, &wt_cfg.env, &branch,
                if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                &shared_hosts,
            );
        }
        // Write env file
        let branch_safe = crate::services::branch_safe(&branch);
        let resolved = crate::services::resolve_env_templates(&wt_cfg.env, "127.0.0.1", &branch_safe, &branch);
        let env_file = wt_cfg.env_file.as_deref().unwrap_or(".env.local");
        crate::services::apply_env_overrides(p, &resolved, env_file);
        let _ = crate::services::write_env_file(p, "127.0.0.1");
        crate::services::ensure_global_gitignore();
        format!("main {dir_name} setup with 127.0.0.1. Restart services to apply.")
    }

    /// Setup ALL dirs with worktree=true to bind 127.0.0.1.
    #[allow(dead_code)]
    pub fn setup_all_main_loopback(&mut self) -> String {
        let mut count = 0;
        let dirs: Vec<(String, crate::config::WorktreeConfig)> = self.config.repos.iter()
            .filter_map(|(name, d)| d.wt().map(|wt| (name.clone(), wt.clone())))
            .collect();
        for (dir_name, wt_cfg) in &dirs {
            if let Some(dir_path) = self.dir_path(dir_name) {
                let p = std::path::Path::new(&dir_path);
                let branch = self.dir_branch(dir_name).unwrap_or_else(|| "main".to_string());
                let (svc_overrides, shared_hosts) = self.resolve_shared_overrides(dir_name);
                let compose_files = if wt_cfg.compose_files.is_empty() && p.join("docker-compose.yml").is_file() {
                    vec!["docker-compose.yml".to_string()]
                } else {
                    wt_cfg.compose_files.clone()
                };
                if !compose_files.is_empty() {
                    crate::services::setup_main_as_worktree(
                        p, &compose_files, &wt_cfg.env, &branch,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts,
                    );
                }
                // Write env file for main
                let branch_safe = crate::services::branch_safe(&branch);
                let resolved = crate::services::resolve_env_templates(&wt_cfg.env, "127.0.0.1", &branch_safe, &branch);
                let env_file = wt_cfg.env_file.as_deref().unwrap_or(".env.local");
                crate::services::apply_env_overrides(p, &resolved, env_file);
                let _ = crate::services::write_env_file(p, "127.0.0.1");
                count += 1;
            }
        }
        crate::services::ensure_global_gitignore();
        format!("{count} dirs bound to 127.0.0.1. Restart services to apply.")
    }

    /// Create workspace: mark as "creating", run heavy work in background.
    /// UI shows "creating..." immediately, `scan_worktrees` picks up results on tick.
    pub fn create_workspace(&mut self, ws_name: &str, branch_name: &str) -> (String, Option<String>) {
        let workspaces = self.config.all_workspaces();
        let entries = match workspaces.get(ws_name) {
            Some(e) => e.clone(),
            None => return ("workspace not found".into(), None),
        };

        let mut unique_dirs: Vec<String> = Vec::new();
        for entry in &entries {
            if let Ok((dir, _)) = self.config.find_service_entry(entry) {
                if !unique_dirs.contains(&dir) {
                    unique_dirs.push(dir);
                }
            }
        }
        if unique_dirs.is_empty() {
            return ("no dirs found in workspace".into(), None);
        }

        // Check /etc/hosts for shared service hostnames
        if !self.config.shared_services.is_empty() {
            let hostnames: Vec<&str> = self.config.shared_services.values()
                .filter_map(|s| s.host.as_deref())
                .collect();
            let missing = crate::services::check_etc_hosts(&hostnames);
            if !missing.is_empty() {
                return (format!("Missing hosts in /etc/hosts: {}. Run: tncli setup", missing.join(", ")), None);
            }
        }

        // Allocate IP + slots (fast, state only)
        let ip = crate::services::allocate_ip(&format!("ws-{branch_name}"));
        if !self.config.shared_services.is_empty() {
            let ws_key = format!("ws-{branch_name}");
            for dir_name in &unique_dirs {
                if let Some(dir) = self.config.repos.get(dir_name) {
                    if let Some(wt_cfg) = dir.wt() {
                        for sref in &wt_cfg.shared_services {
                            if let Some(svc_def) = self.config.shared_services.get(&sref.name) {
                                if let Some(capacity) = svc_def.capacity {
                                    let base_port = svc_def.ports.first()
                                        .and_then(|p| p.split(':').next())
                                        .and_then(|p| p.parse::<u16>().ok())
                                        .unwrap_or(6379);
                                    crate::services::allocate_slot(&sref.name, &ws_key, capacity, base_port);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Mark as creating → UI shows "creating..." immediately
        self.creating_workspaces.insert(branch_name.to_string());
        self.rebuild_combo_tree();

        // Collect all data needed for background thread (can't borrow self)
        let config = self.config.clone();
        let config_path = self.config_path.clone();
        let session = self.session.clone();
        let branch = branch_name.to_string();
        let bind_ip = ip.clone();
        let dirs = unique_dirs.clone();
        let dir_branches: Vec<(String, String)> = unique_dirs.iter()
            .map(|d| (d.clone(), self.dir_branch(d).unwrap_or_else(|| "main".to_string())))
            .collect();
        let dir_paths: Vec<(String, String)> = unique_dirs.iter()
            .filter_map(|d| self.dir_path(d).map(|p| (d.clone(), p)))
            .collect();
        let shared_overrides: Vec<(String, indexmap::IndexMap<String, crate::config::ServiceOverride>, Vec<String>)> =
            unique_dirs.iter()
                .map(|d| {
                    let (ov, hosts) = self.resolve_shared_overrides(d);
                    (d.clone(), ov, hosts)
                })
                .collect();

        // Background: all heavy I/O
        std::thread::spawn(move || {
            let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
            let ws_folder = crate::services::ensure_workspace_folder(config_dir, &branch);
            let network_name = format!("tncli-ws-{branch}");
            let branch_safe = crate::services::branch_safe(&branch);

            // Start shared services
            if !config.shared_services.is_empty() {
                let mut needed: Vec<String> = Vec::new();
                for dir_name in &dirs {
                    if let Some(dir) = config.repos.get(dir_name) {
                        if let Some(wt_cfg) = dir.wt() {
                            for sref in &wt_cfg.shared_services {
                                if !needed.contains(&sref.name) { needed.push(sref.name.clone()); }
                            }
                        }
                    }
                }
                crate::services::generate_shared_compose(config_dir, &session, &config.shared_services);
                let refs: Vec<&str> = needed.iter().map(|s| s.as_str()).collect();
                crate::services::start_shared_services(config_dir, &session, &refs);

                // Create databases
                for dir_name in &dirs {
                    if let Some(dir) = config.repos.get(dir_name) {
                        if let Some(wt_cfg) = dir.wt() {
                            for sref in &wt_cfg.shared_services {
                                if let Some(db_tpl) = &sref.db_name {
                                    let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                                        .replace("{{branch}}", &branch);
                                    let svc_def = config.shared_services.get(&sref.name);
                                    let host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost");
                                    let port = svc_def.and_then(|d| d.ports.first())
                                        .and_then(|p| p.split(':').next())
                                        .and_then(|p| p.parse().ok())
                                        .unwrap_or(5432);
                                    let user = svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres");
                                    let pw = svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres");
                                    crate::services::create_shared_db(host, port, &db_name, user, pw);
                                }
                            }
                        }
                    }
                }
            }

            // Setup main as worktree (127.0.0.1, with env + shared hosts + DB)
            for (dir_name, dir_path) in &dir_paths {
                let p = std::path::Path::new(dir_path);
                let dir_cfg = config.repos.get(dir_name);
                let wt_cfg = dir_cfg.and_then(|d| d.wt());
                if let Some(wt) = wt_cfg {
                    if !wt.compose_files.is_empty() || p.join("docker-compose.yml").is_file() {
                        let compose_files = if wt.compose_files.is_empty() {
                            vec!["docker-compose.yml".to_string()]
                        } else {
                            wt.compose_files.clone()
                        };
                        let (svc_overrides, shared_hosts) = shared_overrides.iter()
                            .find(|(d, _, _)| d == dir_name)
                            .map(|(_, ov, h)| (ov.clone(), h.clone()))
                            .unwrap_or_default();
                        let main_branch = dir_branches.iter()
                            .find(|(d, _)| d == dir_name)
                            .map(|(_, b)| b.as_str())
                            .unwrap_or("main");
                        crate::services::setup_main_as_worktree(
                            p, &compose_files, &wt.env, main_branch,
                            if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                            &shared_hosts,
                        );
                    }

                    // Write env file for main dir
                    let main_branch = dir_branches.iter()
                        .find(|(d, _)| d == dir_name)
                        .map(|(_, b)| b.as_str())
                        .unwrap_or("main");
                    let branch_safe = crate::services::branch_safe(main_branch);
                    let resolved = crate::services::resolve_env_templates(&wt.env, "127.0.0.1", &branch_safe, main_branch);
                    let env_file = wt.env_file.as_deref().unwrap_or(".env.local");
                    crate::services::apply_env_overrides(p, &resolved, env_file);
                    let _ = crate::services::write_env_file(p, "127.0.0.1");

                    // Create DB for main dir
                    for sref in &wt.shared_services {
                        if let Some(db_tpl) = &sref.db_name {
                            let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                                .replace("{{branch}}", main_branch);
                            let svc_def = config.shared_services.get(&sref.name);
                            let host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost");
                            let port = svc_def.and_then(|d| d.ports.first())
                                .and_then(|p| p.split(':').next())
                                .and_then(|p| p.parse().ok())
                                .unwrap_or(5432);
                            let user = svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres");
                            let pw = svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres");
                            crate::services::create_shared_db(host, port, &db_name, user, pw);
                        }
                    }
                }
            }

            // Create worktrees
            let mut wt_dirs: Vec<(String, std::path::PathBuf)> = Vec::new();
            for (dir_name, base_branch) in &dir_branches {
                let dir_path = match dir_paths.iter().find(|(d, _)| d == dir_name) {
                    Some((_, p)) => p.clone(),
                    None => continue,
                };
                let wt_cfg = config.repos.get(dir_name).and_then(|d| d.wt());
                let copy_files = wt_cfg.map(|wt| wt.copy.clone()).unwrap_or_default();

                if let Ok(wt_path) = crate::services::create_worktree_from_base(
                    std::path::Path::new(&dir_path), &branch, base_branch, &copy_files, Some(&ws_folder),
                ) {
                    // Generate compose override
                    let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
                    let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
                    let (svc_overrides, shared_hosts) = shared_overrides.iter()
                        .find(|(d, _, _)| d == dir_name)
                        .map(|(_, ov, h)| (ov.clone(), h.clone()))
                        .unwrap_or_default();
                    crate::services::generate_compose_override(
                        std::path::Path::new(&dir_path), &wt_path, &bind_ip,
                        &compose_files, &worktree_env, &branch, None,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts,
                    );
                    let _ = crate::services::write_env_file(&wt_path, &bind_ip);

                    // Write .env.local
                    let resolved = crate::services::resolve_env_templates(&worktree_env, &bind_ip, &branch_safe, &branch);
                    let env_file = wt_cfg.and_then(|wt| wt.env_file.as_deref()).unwrap_or(".env.local");
                    crate::services::apply_env_overrides(&wt_path, &resolved, env_file);

                    wt_dirs.push((dir_name.clone(), wt_path));
                }
            }

            // Run setup commands (login shell to load nvm/rvm)
            for (dir_name, wt_path) in &wt_dirs {
                let setup = config.repos.get(dir_name)
                    .and_then(|d| d.wt())
                    .map(|wt| wt.setup.clone()).unwrap_or_default();
                if !setup.is_empty() {
                    let combined = setup.join(" && ");
                    let _ = std::process::Command::new("zsh")
                        .args(["-lc", &combined])
                        .current_dir(wt_path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
            }

            // Create network + regenerate overrides with network
            crate::services::create_docker_network(&network_name);
            for (dir_name, wt_path) in &wt_dirs {
                let wt_cfg = config.repos.get(dir_name).and_then(|d| d.wt());
                let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
                let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
                let dir_path = dir_paths.iter().find(|(d, _)| d == dir_name)
                    .map(|(_, p)| p.clone()).unwrap_or_default();
                let (svc_overrides, shared_hosts) = shared_overrides.iter()
                    .find(|(d, _, _)| d == dir_name)
                    .map(|(_, ov, h)| (ov.clone(), h.clone()))
                    .unwrap_or_default();
                crate::services::generate_compose_override(
                    std::path::Path::new(&dir_path), wt_path, &bind_ip,
                    &compose_files, &worktree_env, &branch, Some(&network_name),
                    if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                    &shared_hosts,
                );
            }

            crate::services::ensure_global_gitignore();
        });

        (format!("creating workspace {branch_name} (BIND_IP={ip})..."), Some(ip))
    }

    /// Delete workspace: mark as deleting, run cleanup in background.
    /// Worktrees stay in state with "deleting" status until cleanup finishes.
    pub fn delete_workspace_by_name(&mut self, branch_name: &str) -> (String, Option<String>) {
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();

        // Mark workspace as deleting (UI shows "deleting..." state)
        self.deleting_workspaces.insert(branch_name.to_string());
        self.rebuild_combo_tree();

        // Collect worktree info for background cleanup
        let wt_keys: Vec<String> = self.worktrees.keys()
            .filter(|k| k.ends_with(&format!("--{}", branch_name.replace('/', "-"))))
            .cloned()
            .collect();

        let mut cleanup_items: Vec<(String, PathBuf, String, Vec<String>)> = Vec::new();
        for wt_key in &wt_keys {
            if let Some(wt) = self.worktrees.get(wt_key) {
                let dir_path = self.dir_path(&wt.parent_dir).unwrap_or_default();
                let pre_delete = self.config.repos.get(&wt.parent_dir)
                    .and_then(|d| d.wt())
                    .map(|wt| wt.pre_delete.clone())
                    .unwrap_or_default();

                // Stop tmux services immediately
                if let Some(dir) = self.config.repos.get(&wt.parent_dir) {
                    for svc_name in dir.services.keys() {
                        let tmux_name = self.wt_tmux_name(&wt.parent_dir, svc_name, &wt.branch);
                        if self.is_running(&tmux_name) {
                            crate::tmux::graceful_stop(&self.session, &tmux_name);
                        }
                    }
                }

                cleanup_items.push((dir_path, wt.path.clone(), wt.branch.clone(), pre_delete));
            }
        }

        // Release shared service slots + IP
        let ws_key = format!("ws-{branch_name}");
        for (svc_name, _) in &self.config.shared_services {
            crate::services::release_slot(svc_name, &ws_key);
        }
        crate::services::release_ip(&ws_key);

        // Collect databases to drop
        let branch_safe = branch_name.replace('/', "_").replace('-', "_");
        let mut dbs_to_drop: Vec<(String, u16, String, String, String)> = Vec::new(); // (host, port, db, user, pw)
        for wt_key in &wt_keys {
            if let Some(wt) = self.worktrees.get(wt_key) {
                if let Some(dir) = self.config.repos.get(&wt.parent_dir) {
                    for sref in dir.wt().into_iter().flat_map(|wt| &wt.shared_services) {
                        if let Some(db_tpl) = &sref.db_name {
                            let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                                .replace("{{branch}}", branch_name);
                            let svc_def = self.config.shared_services.get(&sref.name);
                            let host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost").to_string();
                            let port = svc_def.and_then(|d| d.ports.first())
                                .and_then(|p| p.split(':').next())
                                .and_then(|p| p.parse().ok())
                                .unwrap_or(5432);
                            let user = svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres").to_string();
                            let pw = svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres").to_string();
                            dbs_to_drop.push((host, port, db_name, user, pw));
                        }
                    }
                }
            }
        }

        // Background: run all cleanup, then remove from state
        let network = format!("tncli-ws-{branch_name}");
        let branch_owned = branch_name.to_string();
        let config_dir_owned = config_dir.clone();
        std::thread::spawn(move || {
            // Pre-delete commands + git worktree remove for each dir
            for (dir_path, wt_path, wt_branch, pre_delete) in &cleanup_items {
                if !pre_delete.is_empty() && wt_path.exists() {
                    let combined = pre_delete.join(" && ");
                    let _ = std::process::Command::new("zsh")
                        .args(["-c", &combined])
                        .current_dir(wt_path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
                let _ = crate::services::remove_worktree(
                    std::path::Path::new(dir_path), wt_path, wt_branch,
                );
            }
            // Drop databases from shared postgres
            for (host, port, db_name, user, pw) in &dbs_to_drop {
                crate::services::drop_shared_db(host, *port, db_name, user, pw);
            }
            crate::services::remove_docker_network(&network);
            crate::services::delete_workspace_folder(&config_dir_owned, &branch_owned);
        });

        let ip_to_teardown: Option<String> = None;

        let msg = format!("workspace {} deleted ({} worktrees)", branch_name, wt_keys.len());
        (msg, ip_to_teardown)
    }

    /// Open worktree menu for current dir.
    pub fn open_wt_menu(&mut self) {
        let dir_name = match self.current_combo_item() {
            Some(ComboItem::InstanceDir { dir, .. }) => dir.clone(),
            Some(ComboItem::InstanceService { dir, .. }) => dir.clone(),
            _ => return,
        };
        if !self.config.repos.get(&dir_name).is_some_and(|d| d.has_worktree()) {
            self.set_message(&format!("worktree not enabled for '{dir_name}' -- add worktree: block in tncli.yml"));
            return;
        }
        self.wt_menu_dir = dir_name;
        self.wt_menu_cursor = 0;
        self.wt_menu_open = true;
    }

    /// Open name input for creating worktree from current branch.
    pub fn create_wt_current_branch(&mut self) {
        let dir_name = self.wt_menu_dir.clone();
        let branch = match self.dir_branch(&dir_name) {
            Some(b) => b,
            None => { self.set_message("not a git repo"); return; }
        };
        self.wt_menu_open = false;
        self.wt_name_base_branch = branch;
        self.wt_name_input.clear();
        self.wt_name_input_open = true;
    }

    /// Confirm worktree/workspace creation. Returns BIND_IP if created successfully.
    pub fn confirm_wt_name(&mut self) {
        let new_branch = self.wt_name_input.trim().to_string();
        if new_branch.is_empty() {
            self.set_message("name cannot be empty");
            return;
        }
        self.wt_name_input_open = false;

        if self.ws_creating {
            let ws_name = self.ws_name.clone();
            self.ws_creating = false;
            let (msg, _) = self.create_workspace(&ws_name, &new_branch);
            self.set_message(&msg);
        } else {
            let dir_name = self.wt_menu_dir.clone();
            let base = self.wt_name_base_branch.clone();
            let msg = self.create_worktree_new_branch(&dir_name, &new_branch, &base);
            self.set_message(&msg);
        }
    }

    /// Create worktree with a NEW branch from a base branch.
    pub fn create_worktree_new_branch(&mut self, dir_name: &str, new_branch: &str, base_branch: &str) -> String {
        let dir_path = match self.dir_path(dir_name) {
            Some(p) => p,
            None => return "dir not found".to_string(),
        };
        let wt_cfg = self.config.repos.get(dir_name).and_then(|d| d.wt());
        let copy_files = wt_cfg.map(|wt| wt.copy.clone()).unwrap_or_default();
        match crate::services::create_worktree_from_base(
            std::path::Path::new(&dir_path), new_branch, base_branch, &copy_files, None
        ) {
            Ok(wt_path) => {
                let wt_key = format!("{dir_name}--{}", new_branch.replace('/', "-"));
                let ip = crate::services::allocate_ip(&wt_key);
                let _ = crate::services::write_env_file(&wt_path, &ip);
                let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
                let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
                let repo_dir = std::path::Path::new(&dir_path);
                crate::services::generate_compose_override(repo_dir, &wt_path, &ip, &compose_files, &worktree_env, new_branch, None, None, &[]);
                crate::services::ensure_global_gitignore();
                self.worktrees.insert(wt_key.clone(), crate::services::WorktreeInfo {
                    branch: new_branch.to_string(),
                    parent_dir: dir_name.to_string(),
                    bind_ip: ip.clone(),
                    path: wt_path,
                });
                self.rebuild_combo_tree();
                format!("worktree created: {new_branch} (BIND_IP={ip}). Run migrations before starting services.")
            }
            Err(e) => format!("worktree failed: {e}"),
        }
    }

    fn resolve_shared_overrides(&self, dir_name: &str) -> (indexmap::IndexMap<String, crate::config::ServiceOverride>, Vec<String>) {
        let dir = match self.config.repos.get(dir_name) {
            Some(d) => d,
            None => return (Default::default(), Vec::new()),
        };
        let wt_cfg = match dir.wt() {
            Some(wt) => wt,
            None => return (Default::default(), Vec::new()),
        };
        let mut overrides = wt_cfg.service_overrides.clone();
        let mut hosts: Vec<String> = Vec::new();

        for sref in &wt_cfg.shared_services {
            // Add profiles: disabled for shared services
            if !overrides.contains_key(&sref.name) {
                overrides.insert(sref.name.clone(), crate::config::ServiceOverride {
                    environment: indexmap::IndexMap::new(),
                    profiles: vec!["disabled".to_string()],
                    mem_limit: None,
                });
            }
            // Collect host from top-level shared_services definition
            if let Some(svc_def) = self.config.shared_services.get(&sref.name) {
                if let Some(host) = &svc_def.host {
                    if !hosts.contains(host) {
                        hosts.push(host.clone());
                    }
                }
            }
        }
        (overrides, hosts)
    }

}
