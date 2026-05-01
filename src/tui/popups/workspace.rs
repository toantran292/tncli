use crate::tmux;
use super::super::app::{App, PendingPopup, POPUP_RESULT_FILE};

pub fn handle(app: &mut App, popup: PendingPopup, result: Option<String>) {
    match popup {
        PendingPopup::WsBranchPick { ws_name, ws_branch, mut items_data, idx } => {
            if let Some(branch) = result {
                // Update the target branch at idx in items_data
                let mut parts: Vec<String> = items_data.split(',').map(|s| s.to_string()).collect();
                if let Some(entry) = parts.get_mut(idx) {
                    let fields: Vec<&str> = entry.splitn(5, '|').collect();
                    if fields.len() >= 4 {
                        let sel = fields.get(4).unwrap_or(&"1");
                        *entry = format!("{}|{}|{}|{}|{}", fields[0], fields[1], branch, fields[3], sel);
                        items_data = parts.join(",");
                    }
                }
            }
            // Re-open ws-select popup with updated data
            reopen_ws_select(app, &ws_name, &ws_branch, &items_data);
            return;
        }
        PendingPopup::WsEdit { branch } => {
            if let Some(choice) = result {
                match choice.as_str() {
                    "Create new workspace" => {
                        app.ws_creating = true;
                        app.popup_input("Workspace branch name:",
                            PendingPopup::NameInput { context: "workspace".to_string() });
                    }
                    "Add repo" => {
                        app.popup_stack.push(PendingPopup::WsEdit { branch: branch.clone() });
                        app.build_ws_add_list(&branch);
                    }
                    "Remove repo" => {
                        app.popup_stack.push(PendingPopup::WsEdit { branch: branch.clone() });
                        app.build_ws_remove_list(&branch);
                    }
                    _ => {}
                }
            }
        }
        PendingPopup::WsAdd { branch } => {
            if let Some(dir_name) = result {
                app.add_repo_to_workspace(&dir_name, &branch, &branch);
            }
        }
        PendingPopup::WsRemove => {
            if let Some(wt_key) = result {
                let msg = app.delete_worktree(&wt_key);
                app.set_message(&msg);
            }
        }
        PendingPopup::WsRepoSelect { ws_name, ws_branch } => {
            if let Some(ref text) = result {
                if let Some(rest) = text.strip_prefix("BRANCH_PICK:") {
                    // Format: BRANCH_PICK:idx:items_data
                    if let Some((idx_str, items_data)) = rest.split_once(':') {
                        let idx = idx_str.parse::<usize>().unwrap_or(0);
                        // Extract path and alias for fzf
                        let items: Vec<&str> = items_data.split(',').collect();
                        let (alias, path) = items.get(idx)
                            .and_then(|e| {
                                let f: Vec<&str> = e.splitn(5, '|').collect();
                                if f.len() >= 4 { Some((f[0].to_string(), f[3].to_string())) } else { None }
                            })
                            .unwrap_or_default();

                        open_branch_picker(app, &ws_name, &ws_branch, items_data, idx, &alias, &path);
                    }
                    return;
                }
            }
            if let Some(text) = result {
                let entries: Vec<(String, String)> = text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter_map(|l| {
                        let parts: Vec<&str> = l.trim().splitn(2, ':').collect();
                        if parts.len() == 2 {
                            Some((parts[0].trim().to_string(), parts[1].trim().to_string()))
                        } else { None }
                    })
                    .collect();

                if entries.is_empty() {
                    app.set_message("no repos selected");
                    return;
                }

                // Check branch conflicts via git worktree list
                let mut conflicts = Vec::new();
                for (alias, target) in &entries {
                    let dir_name = app.ws_select_items.iter()
                        .find(|i| i.alias == *alias)
                        .map(|i| i.dir_name.clone())
                        .unwrap_or_else(|| alias.clone());
                    if let Some(path) = app.dir_path(&dir_name) {
                        if crate::services::git::is_branch_in_worktree(std::path::Path::new(&path), target) {
                            conflicts.push(alias.clone());
                        }
                    }
                }
                if !conflicts.is_empty() {
                    app.set_message(&format!("branch already in use: {}", conflicts.join(", ")));
                    return;
                }

                let selected_aliases: Vec<String> = entries.iter().map(|(a, _)| a.clone()).collect();
                app.ws_select_items.retain(|i| selected_aliases.contains(&i.alias));
                for (alias, target) in &entries {
                    if let Some(item) = app.ws_select_items.iter_mut().find(|i| i.alias == *alias) {
                        item.branch = target.clone();
                    }
                }

                app.ws_name = ws_name;
                if let Some(tx) = app.event_tx.clone() {
                    let msg = app.start_create_pipeline(&app.ws_name.clone(), &ws_branch, tx);
                    app.set_message(&msg);
                }
            }
        }
        _ => {}
    }
}

fn open_branch_picker(app: &mut App, ws_name: &str, ws_branch: &str, items_data: &str, idx: usize, alias: &str, path: &str) {
    let _ = std::fs::remove_file(POPUP_RESULT_FILE);
    // fetch + fzf in a single tmux popup
    let title = format!(" {} — select branch ", alias);
    let cmd = format!(
        "printf '\\033[33m  Fetching branches...\\033[0m' && git -C '{}' fetch origin --prune -q 2>/dev/null; git -C '{}' branch -a --sort=-committerdate | sed 's/^[* ]*//' | sed 's|remotes/origin/||' | sort -u | fzf --prompt='Branch> ' --reverse > {}",
        path, path, POPUP_RESULT_FILE
    );
    tmux::display_popup_styled(&tmux::PopupOptions {
        width: "50%", height: "70%",
        title: Some(&title),
        border_style: Some("fg=magenta"),
        style: None,
        border_lines: Some("rounded"),
    }, &cmd);
    app.pending_popup = Some(PendingPopup::WsBranchPick {
        ws_name: ws_name.to_string(),
        ws_branch: ws_branch.to_string(),
        items_data: items_data.to_string(),
        idx,
    });
}

pub(super) fn reopen_ws_select(app: &mut App, ws_name: &str, ws_branch: &str, items_data: &str) {
    let exe = std::env::current_exe().unwrap_or_default();
    let cmd = format!(
        "{} popup --type ws-select --data '{}'",
        exe.display(),
        items_data.replace('\'', "'\\''"),
    );
    let item_count = items_data.split(',').count();
    let h = format!("{}", (item_count + 4).min(20).max(6));
    let title = format!(" Create workspace: {} ", ws_branch);
    tmux::display_popup_styled(&tmux::PopupOptions {
        width: "55", height: &h,
        title: Some(&title),
        border_style: Some("fg=green"),
        style: None,
        border_lines: Some("rounded"),
    }, &cmd);
    app.pending_popup = Some(PendingPopup::WsRepoSelect {
        ws_name: ws_name.to_string(),
        ws_branch: ws_branch.to_string(),
    });
}
