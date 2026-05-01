use crate::tmux;
use crate::tui::app::{App, WsSelectItem, PendingPopup, POPUP_RESULT_FILE, workspace_branch};

impl App {
    pub fn build_ws_select(&mut self, ws_branch: &str) {
        let ws_name = self.ws_name.clone();

        let all_ws = self.config.all_workspaces();
        let entries = all_ws.get(&ws_name).cloned().unwrap_or_default();
        let mut unique_dirs = Vec::new();
        for entry in &entries {
            if let Some((dir, _)) = self.config.find_service_entry_quiet(entry) {
                if !unique_dirs.contains(&dir) { unique_dirs.push(dir); }
            }
        }

        let conflicts: Vec<String> = unique_dirs.iter()
            .filter(|d| self.has_branch_conflict(d, ws_branch))
            .cloned().collect();
        if !conflicts.is_empty() {
            self.set_message(&format!("branch conflict: {}", conflicts.join(", ")));
            return;
        }

        self.ws_select_items = unique_dirs.iter().map(|dir_name| {
            let alias = self.config.repos.get(dir_name)
                .and_then(|d| d.alias.as_deref())
                .unwrap_or(dir_name)
                .to_string();
            let base = if let Some(ref src) = self.ws_source_branch {
                self.worktrees.values()
                    .find(|wt| wt.parent_dir == *dir_name && workspace_branch(wt).as_deref() == Some(src))
                    .and_then(|wt| self.wt_git_branch(&wt.path))
                    .unwrap_or_else(|| self.config.default_branch_for(dir_name))
            } else {
                self.config.default_branch_for(dir_name)
            };
            WsSelectItem {
                dir_name: dir_name.clone(),
                alias,
                selected: true,
                branch: base,
                conflict: false,
            }
        }).collect();

        // format: alias|source|target|git_path per item, comma-separated
        let items_str: Vec<String> = self.ws_select_items.iter()
            .map(|i| {
                let path = self.dir_path(&i.dir_name).unwrap_or_default();
                format!("{}|{}|{}|{}", i.alias, i.branch, ws_branch, path)
            })
            .collect();
        let exe = std::env::current_exe().unwrap_or_default();
        let cmd = format!(
            "{} popup --type ws-select --data '{}'",
            exe.display(),
            items_str.join(",").replace('\'', "'\\''"),
        );
        let h = format!("{}", (self.ws_select_items.len() + 4).min(20).max(6));
        let title = format!(" Create workspace: {} ", ws_branch);
        tmux::display_popup_styled(&tmux::PopupOptions {
            width: "55", height: &h,
            title: Some(&title),
            border_style: Some("fg=green"),
            style: None,
            border_lines: Some("rounded"),
        }, &cmd);
        self.pending_popup = Some(PendingPopup::WsRepoSelect { ws_name, ws_branch: ws_branch.to_string() });
    }

    pub fn build_ws_add_list(&mut self, branch: &str) {
        let existing_dirs: Vec<String> = self.worktrees.values()
            .filter(|wt| workspace_branch(wt).as_deref() == Some(branch))
            .map(|wt| wt.parent_dir.clone())
            .collect();

        let available: Vec<String> = self.dir_names.iter()
            .filter(|d| !existing_dirs.contains(d))
            .map(|d| {
                let alias = self.config.repos.get(d)
                    .and_then(|dir| dir.alias.as_deref())
                    .unwrap_or(d);
                format!("{}\t{}", d, alias)
            })
            .collect();

        if available.is_empty() {
            self.set_message("all repos already in workspace");
            return;
        }

        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let items = available.join("\n");
        let cmd = format!(
            "printf '{}' | fzf --prompt='Add repo> ' --with-nth=2 --delimiter='\t' | cut -f1 > {}",
            items.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("50%", "40%", &cmd);
        self.pending_popup = Some(PendingPopup::WsAdd { branch: branch.to_string() });
    }

    pub fn build_ws_remove_list(&mut self, branch: &str) {
        let repos: Vec<(String, String)> = self.worktrees.iter()
            .filter(|(_, wt)| workspace_branch(wt).as_deref() == Some(branch))
            .map(|(wt_key, wt)| {
                let alias = self.config.repos.get(&wt.parent_dir)
                    .and_then(|d| d.alias.as_deref())
                    .unwrap_or(&wt.parent_dir)
                    .to_string();
                (wt_key.clone(), alias)
            })
            .collect();

        if repos.is_empty() {
            self.set_message("no repos to remove");
            return;
        }

        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let items: Vec<String> = repos.iter().map(|(k, a)| format!("{}\t{}", k, a)).collect();
        let input = items.join("\n");
        let cmd = format!(
            "printf '{}' | fzf --prompt='Remove repo> ' --with-nth=2 --delimiter='\t' | cut -f1 > {}",
            input.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("50%", "40%", &cmd);
        self.pending_popup = Some(PendingPopup::WsRemove);
    }
}
