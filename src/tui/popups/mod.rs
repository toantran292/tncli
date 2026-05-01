mod git;
mod shortcuts;
mod workspace;

use crate::tmux;
use super::app::{App, ConfirmAction, PendingPopup, POPUP_RESULT_FILE};

impl App {
    pub fn popup_menu(&mut self, title: &str, options: &[&str], popup: PendingPopup) {
        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let items = options.join("\n");
        let cmd = format!(
            "printf '{}' | fzf --prompt='{} > ' --no-info --reverse > {}",
            items.replace('\'', "'\\''"), title.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("50%", "40%", &cmd);
        self.pending_popup = Some(popup);
    }

    pub fn popup_input(&mut self, title: &str, popup: PendingPopup) {
        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let exe = std::env::current_exe().unwrap_or_default();
        let cmd = format!("{} popup --type input", exe.display());
        let t = format!(" {} ", title);
        tmux::display_popup_styled(&tmux::PopupOptions {
            width: "40", height: "5",
            title: Some(&t),
            border_style: Some("fg=green"),
            style: None,
            border_lines: Some("rounded"),
        }, &cmd);
        self.pending_popup = Some(popup);
    }

    pub fn popup_confirm(&mut self, msg: &str, action: ConfirmAction) {
        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let exe = std::env::current_exe().unwrap_or_default();
        let cmd = format!("{} popup --type confirm", exe.display());
        let t = format!(" {} ", msg);
        tmux::display_popup_styled(&tmux::PopupOptions {
            width: "40%", height: "6",
            title: Some(&t),
            border_style: Some("fg=red"),
            style: None,
            border_lines: Some("rounded"),
        }, &cmd);
        self.pending_popup = Some(PendingPopup::Confirm { action });
    }

    fn relaunch_popup(&mut self, popup: PendingPopup) {
        match popup {
            PendingPopup::GitMenu { dir, path } => {
                self.popup_menu("Git", &["checkout branch", "pull origin", "diff view"],
                    PendingPopup::GitMenu { dir, path });
            }
            PendingPopup::WsEdit { branch } => {
                self.popup_menu("Workspace", &["Create new workspace", "Add repo", "Remove repo"],
                    PendingPopup::WsEdit { branch });
            }
            _ => {}
        }
    }

    pub fn poll_popup_result(&mut self) {
        let popup = match self.pending_popup.take() {
            Some(p) => p,
            None => return,
        };

        let result = match std::fs::read_to_string(POPUP_RESULT_FILE) {
            Ok(s) => {
                let _ = std::fs::remove_file(POPUP_RESULT_FILE);
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            }
            Err(_) => {
                self.pending_popup = Some(popup);
                return;
            }
        };

        if result.is_none() {
            // ESC on branch picker → re-open ws-select without changes
            if let PendingPopup::WsBranchPick { ws_name, ws_branch, items_data, .. } = popup {
                workspace::reopen_ws_select(self, &ws_name, &ws_branch, &items_data);
                return;
            }
            if let Some(parent) = self.popup_stack.pop() {
                self.relaunch_popup(parent);
            }
            return;
        }

        self.popup_stack.clear();

        match popup {
            PendingPopup::BranchPicker { .. } | PendingPopup::GitPullAll { .. }
                | PendingPopup::GitMenu { .. } => git::handle(self, popup, result),
            PendingPopup::WsEdit { .. } | PendingPopup::WsAdd { .. }
                | PendingPopup::WsRemove | PendingPopup::WsRepoSelect { .. }
                | PendingPopup::WsBranchPick { .. } => workspace::handle(self, popup, result),
            PendingPopup::Shortcut => shortcuts::handle(self, popup, result),
            PendingPopup::NameInput { context } => {
                if let Some(name) = result {
                    if name.is_empty() { return; }
                    if context.starts_with("branch:") {
                        let dir = context.strip_prefix("branch:").unwrap_or("");
                        let msg = self.create_branch_and_checkout(dir, &name);
                        self.set_message(&msg);
                    } else if context == "workspace" {
                        if self.event_tx.is_some() {
                            self.build_ws_select(&name);
                        }
                    }
                }
            }
            PendingPopup::Confirm { action } => {
                if let Some(answer) = result {
                    if answer.trim().eq_ignore_ascii_case("y") {
                        self.confirm_action = action;
                        self.execute_confirm();
                    } else {
                        self.set_message("cancelled");
                    }
                }
            }
        }
    }
}
