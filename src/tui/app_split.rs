use crate::tmux;
use super::app::App;

impl App {
    pub fn ensure_split(&mut self) {
        let wid = match &self.tui_window_id {
            Some(id) => id.clone(),
            None => return,
        };
        let panes = tmux::list_pane_ids(&wid);
        if panes.len() < 2 {
            // Right pane died — if it was a global service, kill ghost window in svc session
            if let Some(ref svc) = self.joined_service {
                if svc.starts_with("_global~") {
                    let svc_sess = self.svc_session();
                    tmux::kill_window(&svc_sess, svc);
                }
            }
            let placeholder = "echo; echo '  Select a running service'; echo; echo '  j/k navigate · Tab focus · n/N cycle'; stty -echo; tail -f /dev/null";
            tmux::split_window_right(75, Some(placeholder));
            let all_panes = tmux::list_pane_ids(&wid);
            self.right_pane_id = all_panes.into_iter()
                .find(|p| self.tui_pane_id.as_ref() != Some(p));
            if let Some(ref rpid) = self.right_pane_id {
                tmux::set_pane_title(rpid, "service");
            }
            self.joined_service = None;
        }
    }

    pub fn setup_split(&mut self) {
        let wid = match &self.tui_window_id {
            Some(id) => id.clone(),
            None => return,
        };

        // Record our pane ID before split
        self.tui_pane_id = tmux::current_pane_id();

        let placeholder = "echo; echo '  Select a running service'; echo; echo '  j/k navigate · Tab focus · n/N cycle'; stty -echo; tail -f /dev/null";
        tmux::split_window_right(75, Some(placeholder));

        // Detect right pane ID (the new one, not ours)
        let all_panes = tmux::list_pane_ids(&wid);
        self.right_pane_id = all_panes.into_iter()
            .find(|p| self.tui_pane_id.as_ref() != Some(p));

        tmux::set_window_option(&wid, "pane-border-status", "top");
        tmux::set_window_option(&wid, "pane-border-format",
            " #{?pane_active,#[fg=colour39#,bold],#[fg=colour252]}#{pane_title}#[default] ");
        if let Some(ref pid) = self.tui_pane_id {
            tmux::set_pane_title(pid, &self.session);
        }
        if let Some(ref pid) = self.right_pane_id {
            tmux::set_pane_title(pid, "service");
        }
    }

    pub fn teardown_split(&mut self) {
        let wid = match &self.tui_window_id {
            Some(id) => id.clone(),
            None => return,
        };
        let svc_sess = self.svc_session();

        // Restore service back to its window
        if let Some(svc) = self.joined_service.take() {
            if let Some(ref rpid) = self.right_pane_id {
                if tmux::window_exists(&svc_sess, &svc) {
                    let _ = tmux::swap_pane(&svc_sess, &svc, rpid);
                } else {
                    tmux::ensure_session(&svc_sess);
                    tmux::break_pane_to(rpid, &svc_sess, &svc);
                }
            }
        }
        // Kill any remaining panes that aren't our TUI pane
        if let Some(ref tui_pid) = self.tui_pane_id {
            for p in tmux::list_pane_ids(&wid) {
                if p != *tui_pid {
                    tmux::kill_pane(&p);
                }
            }
        }
        tmux::unset_window_option(&wid, "pane-border-status");
        tmux::unset_window_option(&wid, "pane-border-format");
        self.right_pane_id = None;
    }

    pub fn swap_display_service(&mut self) {
        let svc_sess = self.svc_session();

        let new_svc = self.log_service_name();

        if new_svc == self.joined_service {
            // Same service but cursor may have changed context — update title
            if let (Some(svc), Some(rpid)) = (&self.joined_service, &self.right_pane_id) {
                let title = self.build_pane_title(svc);
                tmux::set_pane_title(rpid, &title);
            }
            return;
        }

        // Step 1: Restore current service back to its window (swap back)
        if let Some(old) = self.joined_service.take() {
            if let Some(rpid) = &self.right_pane_id {
                if tmux::window_exists(&svc_sess, &old) {
                    let _ = tmux::swap_pane(&svc_sess, &old, rpid);
                    self.redetect_right_pane();
                }
            }
        }

        // Step 2: Show new service (swap in)
        if let Some(ref new) = new_svc {
            if let Some(rpid) = &self.right_pane_id {
                if tmux::window_exists(&svc_sess, new) && tmux::swap_pane(&svc_sess, new, rpid).is_ok() {
                    self.joined_service = Some(new.clone());
                    self.redetect_right_pane();
                    if let Some(rpid) = &self.right_pane_id {
                        let title = self.build_pane_title(new);
                        tmux::set_pane_title(rpid, &title);
                    }
                }
            }
        } else if let Some(rpid) = &self.right_pane_id {
            tmux::set_pane_title(rpid, "service");
        }
    }

    pub(crate) fn redetect_right_pane(&mut self) {
        if let Some(wid) = &self.tui_window_id {
            let all_panes = tmux::list_pane_ids(wid);
            self.right_pane_id = all_panes.into_iter()
                .find(|p| self.tui_pane_id.as_ref() != Some(p));
        }
    }

    fn build_pane_title(&self, svc: &str) -> String {
        let cycle = self.log_cycle_info();
        let branch_tag = self.selected_dir_name()
            .and_then(|d| {
                self.selected_work_dir(&d)
                    .and_then(|p| self.wt_git_branch(std::path::Path::new(&p)))
                    .or_else(|| self.dir_branch(&d))
            })
            .map(|b| format!("({b}) "))
            .unwrap_or_default();

        if let Some((cur, total)) = cycle {
            format!("{branch_tag}{svc} [{cur}/{total}]")
        } else {
            format!("{branch_tag}{svc}")
        }
    }
}
