use crate::tmux;
use super::super::app::{App, ComboItem, PendingPopup, POPUP_RESULT_FILE, workspace_branch};

impl App {
    pub fn popup_git_menu(&mut self) {
        match self.current_combo_item().cloned() {
            Some(ComboItem::Instance { branch, is_main }) => {
                let label = if is_main { "main" } else { &branch };
                self.popup_menu(&format!("Git ({label})"), &[
                    "pull all repos",
                ], PendingPopup::GitPullAll { branch: branch.clone(), is_main });
            }
            Some(ComboItem::InstanceDir { dir, wt_key, is_main, .. }) |
            Some(ComboItem::InstanceService { dir, wt_key, is_main, .. }) => {
                let path = if is_main {
                    self.dir_path(&dir)
                } else {
                    self.worktrees.get(&wt_key).map(|wt| wt.path.to_string_lossy().into_owned())
                        .or_else(|| self.dir_path(&dir))
                };
                let Some(path) = path else { self.set_message("dir not found"); return; };

                if is_main {
                    self.popup_menu("Git (main)", &[
                        "pull origin",
                        "diff view",
                    ], PendingPopup::GitMenu { dir, path });
                } else {
                    self.popup_menu("Git", &[
                        "checkout branch",
                        "pull origin",
                        "diff view",
                    ], PendingPopup::GitMenu { dir, path });
                }
            }
            _ => { self.set_message("select a dir first"); }
        }
    }

    pub fn popup_branch_picker(&mut self, dir_name: &str, checkout_mode: bool) {
        let dir_path = if checkout_mode {
            self.selected_work_dir(dir_name).unwrap_or_else(|| self.dir_path(dir_name).unwrap_or_default())
        } else {
            self.dir_path(dir_name).unwrap_or_default()
        };

        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let cmd = format!(
            "git -C '{}' branch -a --sort=-committerdate | sed 's/^[* ]*//' | sed 's|remotes/origin/||' | sort -u | fzf --prompt='Branch> ' > {}",
            dir_path, POPUP_RESULT_FILE
        );
        tmux::display_popup("70%", "60%", &cmd);
        self.pending_popup = Some(PendingPopup::BranchPicker { dir: dir_name.to_string(), checkout_mode });
    }
}

pub fn handle(app: &mut App, popup: PendingPopup, result: Option<String>) {
    match popup {
        PendingPopup::BranchPicker { dir, checkout_mode } => {
            if let Some(branch) = result {
                if checkout_mode {
                    let msg = app.git_checkout(&dir, &branch);
                    app.set_message(&msg);
                } else if app.ws_creating {
                    app.ws_creating = false;
                    app.build_ws_select(&branch);
                } else {
                    let msg = app.create_worktree(&dir, &branch);
                    app.set_message(&msg);
                }
            }
        }
        PendingPopup::GitPullAll { branch, is_main } => {
            if result.as_deref() == Some("pull all repos") {
                let mut script = String::from("#!/bin/zsh\n");
                let dirs: Vec<(String, String)> = if is_main {
                    app.dir_names.iter().filter_map(|d| {
                        let path = app.dir_path(d)?;
                        let b = app.config.default_branch_for(d);
                        Some((d.clone(), format!("cd '{}' && git pull origin {}", path, b)))
                    }).collect()
                } else {
                    app.worktrees.values()
                        .filter(|wt| workspace_branch(wt).as_deref() == Some(&branch))
                        .map(|wt| {
                            let path = wt.path.to_string_lossy();
                            (wt.parent_dir.clone(), format!(
                                "cd '{}' && git pull origin \"$(git rev-parse --abbrev-ref HEAD)\"", path
                            ))
                        }).collect()
                };
                for (i, (name, cmd)) in dirs.iter().enumerate() {
                    script.push_str(&format!(
                        "( {} > /tmp/tncli-pull-{i}.log 2>&1 && echo '\\033[32m✓ {name}\\033[0m' || echo '\\033[31m✗ {name}\\033[0m'; cat /tmp/tncli-pull-{i}.log; rm -f /tmp/tncli-pull-{i}.log; echo ) &\n",
                        cmd
                    ));
                }
                script.push_str("wait\necho '\\033[32m[Done]\\033[0m'\n");
                let script_path = "/tmp/tncli-pull-all.sh";
                let _ = std::fs::write(script_path, &script);
                let _ = std::process::Command::new("chmod").args(["+x", script_path]).output();
                let log = "/tmp/tncli-pull-all.log";
                let run = format!("{} 2>&1 | tee '{}'; less -R --mouse +G '{}'; rm -f '{}' '{}'",
                    script_path, log, log, log, script_path);
                tmux::display_popup("80%", "80%", &run);
            }
        }
        PendingPopup::GitMenu { dir, path } => {
            if let Some(choice) = result {
                match choice.as_str() {
                    "checkout branch" => {
                        app.popup_stack.push(PendingPopup::GitMenu { dir: dir.clone(), path: path.clone() });
                        app.popup_branch_picker(&dir, true);
                    }
                    "pull origin" => {
                        let branch = app.dir_branch(&dir).unwrap_or_else(|| "main".to_string());
                        let cmd = format!("git -C '{}' pull origin {}", path, branch);
                        tmux::display_popup("70%", "50%",
                            &format!("({}) 2>&1 | less -R --mouse +G", cmd));
                    }
                    "diff view" => {
                        let cmd = format!(
                            "cd '{}' && git diff --color=always | less -R --mouse",
                            path
                        );
                        tmux::display_popup("90%", "90%", &cmd);
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}
