use crate::config::Config;
use super::app::{App, ComboItem, ConfirmAction, workspace_branch};

impl App {
    pub fn execute_confirm(&mut self) {
        self.confirm_open = false;
        let action = std::mem::take(&mut self.confirm_action);
        match action {
            ConfirmAction::DeleteWorkspace { branch } => {
                if let Some(tx) = self.event_tx.clone() {
                    let (msg, _) = self.start_delete_pipeline(&branch, tx);
                    self.set_message(&msg);
                } else {
                    self.set_message("internal error: no event sender");
                }
            }
            ConfirmAction::StopAll => {
                self.do_stop_all();
            }
            ConfirmAction::None => {}
        }
    }

    pub fn create_branch_and_checkout(&mut self, dir_name: &str, branch: &str) -> String {
        let dir_path = self.selected_work_dir(dir_name)
            .or_else(|| self.dir_path(dir_name))
            .unwrap_or_default();
        match crate::services::git::checkout_new_branch(&dir_path, branch) {
            Ok(()) => {
                self.scan_worktrees();
                format!("created and checked out {branch} in {dir_name}")
            }
            Err(e) => format!("create branch failed: {e}"),
        }
    }

    pub fn git_checkout(&mut self, dir_name: &str, branch: &str) -> String {
        let dir_path = self.selected_work_dir(dir_name)
            .or_else(|| self.dir_path(dir_name))
            .unwrap_or_default();
        match crate::services::git::checkout(&dir_path, branch) {
            Ok(()) => {
                self.scan_worktrees();
                format!("checked out {branch} in {dir_name}")
            }
            Err(e) => format!("checkout failed: {e}"),
        }
    }

    pub fn dir_branch(&self, dir_name: &str) -> Option<String> {
        let dir_path = self.dir_path(dir_name)?;
        crate::services::git::current_branch(&dir_path)
    }

    pub fn wt_git_branch(&self, path: &std::path::Path) -> Option<String> {
        crate::services::git::current_branch(&path.to_string_lossy())
    }

    pub fn reload_config(&mut self) -> String {
        match Config::load(&self.config_path) {
            Ok(config) => {
                let old_dirs = self.dir_names.len();
                let old_combos = self.combos.len();

                self.session = config.session.clone();
                self.dir_names = config.repos.keys().cloned().collect();
                self.combos = config.all_workspaces().keys().cloned().collect();
                self.config = config;
                self.rebuild_combo_tree();
                self.clamp_cursor();

                // Re-generate shared compose + detect changes
                let shared_changed = self.regenerate_shared_compose();
                // Re-generate env files for all dirs
                self.regenerate_all_env_files();

                let svc_count: usize = self.config.repos.values().map(|d| d.services.len()).sum();
                let mut msg = format!(
                    "config reloaded -- {} dirs, {} services, {} combos (was {}/{})",
                    self.dir_names.len(), svc_count, self.combos.len(), old_dirs, old_combos
                );
                if shared_changed {
                    msg.push_str(" | shared services changed — restart to apply");
                }
                msg
            }
            Err(e) => format!("reload failed: {e}"),
        }
    }

    fn regenerate_all_env_files(&self) {

        for dir_name in &self.dir_names {
            let wt_cfg = match self.config.repos.get(dir_name).and_then(|d| d.wt()) {
                Some(wt) => wt.clone(),
                None => continue,
            };

            // Main workspace
            if let Some(dir_path) = self.dir_path(dir_name) {
                let p = std::path::Path::new(&dir_path);
                let branch = self.dir_branch(dir_name).unwrap_or_else(|| "main".to_string());
                let ws_key = format!("ws-{}", branch.replace('/', "-"));
                wt_cfg.apply_all_env_files(p, &self.config, &self.main_bind_ip, &branch, &ws_key);
                let _ = crate::services::write_env_file(p, &self.main_bind_ip);

                // Compose override for main
                let (svc_overrides, shared_hosts) = crate::pipeline::context::resolve_shared_overrides(&self.config, dir_name);
                let compose_files = if wt_cfg.compose_files.is_empty() && p.join("docker-compose.yml").is_file() {
                    vec!["docker-compose.yml".to_string()]
                } else {
                    wt_cfg.compose_files.clone()
                };
                if !compose_files.is_empty() {
                    crate::services::generate_compose_override(
                        p, p, &self.main_bind_ip, &compose_files, &wt_cfg.env, &branch, None,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts, &ws_key, &self.config, &wt_cfg.databases,
                    );
                }
            }

            // Worktrees
            for (_, wt) in &self.worktrees {
                if wt.parent_dir != *dir_name { continue; }
                let ws_branch = workspace_branch(wt).unwrap_or_else(|| wt.branch.clone());
                let ws_key = format!("ws-{}", ws_branch.replace('/', "-"));
                wt_cfg.apply_all_env_files(&wt.path, &self.config, &wt.bind_ip, &ws_branch, &ws_key);
                let _ = crate::services::write_env_file(&wt.path, &wt.bind_ip);

                // Compose override for worktree
                let (svc_overrides, shared_hosts) = crate::pipeline::context::resolve_shared_overrides(&self.config, dir_name);
                let compose_files = if wt_cfg.compose_files.is_empty() && wt.path.join("docker-compose.yml").is_file() {
                    vec!["docker-compose.yml".to_string()]
                } else {
                    wt_cfg.compose_files.clone()
                };
                if !compose_files.is_empty() {
                    let dir_path = self.dir_path(dir_name).unwrap_or_default();
                    crate::services::generate_compose_override(
                        std::path::Path::new(&dir_path), &wt.path, &wt.bind_ip,
                        &compose_files, &wt_cfg.env, &ws_branch, None,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts, &ws_key, &self.config, &wt_cfg.databases,
                    );
                }
            }
        }
    }

    fn regenerate_shared_compose(&self) -> bool {
        if self.config.shared_services.is_empty() {
            return false;
        }
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
        let shared_path = config_dir.join("docker-compose.shared.yml");
        let old_content = std::fs::read_to_string(&shared_path).unwrap_or_default();
        crate::services::generate_shared_compose(config_dir, &self.session, &self.config.shared_services);
        let new_content = std::fs::read_to_string(&shared_path).unwrap_or_default();
        old_content != new_content
    }

    pub fn open_editor(&mut self) {
        let config_dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));

        let path: Option<String> = match self.current_combo_item().cloned() {
            Some(ComboItem::Combo(_)) => {
                self.set_message("select a workspace instance");
                return;
            }
            Some(ComboItem::Instance { branch, is_main }) => {
                if is_main {
                    Some(self.main_workspace_dir().to_string_lossy().into_owned())
                } else {
                    Some(config_dir.join(format!("workspace--{branch}")).to_string_lossy().into_owned())
                }
            }
            Some(ComboItem::InstanceDir { wt_key, dir, is_main, .. }) => {
                if is_main {
                    self.dir_path(&dir)
                } else {
                    self.worktrees.get(&wt_key).map(|wt| wt.path.to_string_lossy().into_owned())
                }
            }
            Some(ComboItem::InstanceService { wt_key, dir, is_main, .. }) => {
                if is_main {
                    self.dir_path(&dir)
                } else {
                    self.worktrees.get(&wt_key).map(|wt| wt.path.to_string_lossy().into_owned())
                }
            }
            None => None,
        };

        let Some(path) = path else {
            self.set_message("no selection");
            return;
        };

        if std::process::Command::new("zed").arg(&path).spawn().is_ok() {
            self.set_message("opened in zed");
        } else if std::process::Command::new("code").arg(&path).spawn().is_ok() {
            self.set_message("opened in code");
        } else {
            self.set_message("no editor found");
        }
    }

    pub fn selected_shortcut(&self) -> Option<(String, String, String)> {
        let shortcut = self.shortcuts_items.get(self.shortcuts_cursor)?;
        let dir_name = self.selected_dir_name()?;

        // Use worktree path if in workspace/worktree context
        let dir_path = self.selected_work_dir(&dir_name)
            .unwrap_or_else(|| self.dir_path(&dir_name).unwrap_or_default());

        // Build env exports from worktree config so shortcuts use correct DB/Redis URLs
        let mut cmd = String::new();
        if let Some(wt_cfg) = self.config.repos.get(&dir_name).and_then(|d| d.wt()) {
            let (bind_ip, branch) = match self.current_combo_item() {
                Some(ComboItem::InstanceDir { wt_key, is_main, branch, .. }) |
                Some(ComboItem::InstanceService { wt_key, is_main, branch, .. }) => {
                    if *is_main {
                        (self.main_bind_ip.clone(), branch.clone())
                    } else {
                        let ip = self.worktrees.get(wt_key)
                            .map(|wt| wt.bind_ip.clone())
                            .unwrap_or_else(|| self.main_bind_ip.clone());
                        (ip, branch.clone())
                    }
                }
                _ => (self.main_bind_ip.clone(), "main".to_string()),
            };
            let branch_safe = crate::services::branch_safe(&branch);
            let ws_key = format!("ws-{}", branch.replace('/', "-"));
            let db_names: Vec<String> = wt_cfg.databases.iter()
                .map(|tpl| {
                    let name = tpl.replace("{{branch_safe}}", &branch_safe).replace("{{branch}}", &branch);
                    format!("{}_{name}", self.config.session)
                })
                .collect();
            // NODE_OPTIONS for dns resolution
            let home = std::env::var("HOME").unwrap_or_default();
            let patch = format!("{home}/.tncli/node-bind-host.js");
            if std::path::Path::new(&patch).exists() {
                cmd.push_str(&format!("export NODE_OPTIONS=\"--dns-result-order=ipv4first --require {patch} ${{NODE_OPTIONS:-}}\" && "));
            }
            cmd.push_str(&format!("export BIND_IP={bind_ip}"));
            // Global env -> worktree env
            let mut merged_env = self.config.env.clone();
            for (k, v) in &wt_cfg.env {
                merged_env.insert(k.clone(), v.clone());
            }
            for (k, v) in &merged_env {
                let val = v.replace("{{bind_ip}}", &bind_ip)
                    .replace("{{branch_safe}}", &branch_safe)
                    .replace("{{branch}}", &branch);
                let val = crate::services::resolve_slot_templates(&val, &ws_key);
                let val = crate::services::resolve_config_templates(&val, &self.config, &branch_safe);
                let val = crate::services::resolve_db_templates(&val, &db_names);
                cmd.push_str(&format!(" && export {k}='{val}'"));
            }
            cmd.push_str(" && ");
        }
        cmd.push_str(&shortcut.cmd);

        Some((cmd, shortcut.desc.clone(), dir_path))
    }

    pub fn selected_work_dir(&self, dir_name: &str) -> Option<String> {
        match self.current_combo_item()? {
            ComboItem::InstanceDir { wt_key, is_main, dir, .. } | ComboItem::InstanceService { wt_key, is_main, dir, .. } => {
                if *is_main {
                    self.dir_path(dir)
                } else {
                    self.worktrees.get(wt_key).map(|wt| wt.path.to_string_lossy().into_owned())
                }
            }
            _ => {
                let _ = dir_name;
                None
            }
        }
    }
}
