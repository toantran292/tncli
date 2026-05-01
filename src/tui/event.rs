use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, MouseButton, MouseEventKind};

use super::app::{App, ComboItem};

pub enum Action {
    None,
    Quit,
}

pub enum AppEvent {
    Terminal(Event),
    Tick,
    Pipeline(crate::pipeline::PipelineEvent),
    WorktreeScanResult(std::collections::HashMap<String, crate::services::WorktreeInfo>),
    Message(String),
}

pub struct EventHandler {
    rx: mpsc::Receiver<AppEvent>,
    tx: mpsc::Sender<AppEvent>,
    _thread: thread::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        let external_tx = tx.clone();

        let thread = thread::spawn(move || {
            loop {
                if event::poll(tick_rate).unwrap_or(false) {
                    for _ in 0..64 {
                        if let Ok(evt) = event::read() {
                            if tx.send(AppEvent::Terminal(evt)).is_err() {
                                return;
                            }
                        }
                        if !event::poll(Duration::ZERO).unwrap_or(false) {
                            break;
                        }
                    }
                } else {
                    if tx.send(AppEvent::Tick).is_err() {
                        return;
                    }
                }
            }
        });

        Self { rx, tx: external_tx, _thread: thread }
    }

    pub fn drain(&self) -> Vec<AppEvent> {
        let mut events = Vec::new();
        while let Ok(evt) = self.rx.try_recv() {
            events.push(evt);
        }
        events
    }

    pub fn next(&self) -> anyhow::Result<AppEvent> {
        Ok(self.rx.recv()?)
    }

    pub fn event_tx(&self) -> mpsc::Sender<AppEvent> {
        self.tx.clone()
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    let code = key.code;

    // Global keys
    match code {
        KeyCode::Esc => { return Action::None; }
        KeyCode::Char('?') => { app.popup_cheatsheet(); return Action::None; }
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Char('t') => {
            // Shell in popup
            let dir = app.selected_dir_name().and_then(|d|
                app.selected_work_dir(&d).or_else(|| app.dir_path(&d))
            );
            if let Some(dir) = dir {
                crate::tmux::display_popup("90%", "85%", &format!("cd '{}' && exec zsh", dir));
            } else {
                app.set_message("select a dir first");
            }
            return Action::None;
        }
        KeyCode::Char('c') => { app.popup_shortcuts(); return Action::None; }
        KeyCode::Char('g') => { app.popup_git_menu(); return Action::None; }
        KeyCode::Char('e') => { app.open_editor(); return Action::None; }
        KeyCode::Char('E') => {
            let path = app.config_path.to_string_lossy().to_string();
            if std::process::Command::new("zed").arg(&path).spawn().is_ok() {
                app.set_message("opened config in zed");
            } else if std::process::Command::new("code").arg(&path).spawn().is_ok() {
                app.set_message("opened config in code");
            } else {
                app.set_message("no editor found");
            }
            return Action::None;
        }
        KeyCode::Char('I') => {
            app.popup_shared_info();
            return Action::None;
        }
        KeyCode::Char('w') | KeyCode::Char('W') => {
            // Detect context: which workspace combo and whether non-main
            let ws_name = app.find_parent_combo(app.cursor);
            let branch = match app.current_combo_item().cloned() {
                Some(ComboItem::Instance { branch, is_main: false }) => Some(branch),
                Some(ComboItem::InstanceDir { branch, is_main: false, .. }) => Some(branch),
                Some(ComboItem::InstanceService { branch, is_main: false, .. }) => Some(branch),
                _ => None,
            };

            if let Some(branch) = branch {
                // Non-main: workspace menu (create new, add repo, remove repo)
                app.ws_name = if ws_name.is_empty() { app.combos.first().cloned().unwrap_or_default() } else { ws_name };
                app.popup_menu("Workspace", &["Create new workspace", "Add repo", "Remove repo"],
                    super::app::PendingPopup::WsEdit { branch });
            } else {
                // Main or Combo: create new workspace
                app.ws_name = if ws_name.is_empty() { app.combos.first().cloned().unwrap_or_default() } else { ws_name };
                app.ws_creating = true;
                app.popup_input("Workspace branch name:",
                    super::app::PendingPopup::NameInput {
                        context: "workspace".to_string(),
                    });
            }
            return Action::None;
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            if let Some(ComboItem::Instance { branch, is_main: false }) = app.current_combo_item().cloned() {
                app.popup_confirm(
                    &format!("Delete workspace '{branch}'?"),
                    super::app::ConfirmAction::DeleteWorkspace { branch },
                );
            }
            return Action::None;
        }
        KeyCode::Char('R') => {
            let msg = app.reload_config();
            app.refresh_status();
            app.set_message(&msg);
            return Action::None;
        }
        _ => {}
    }

    handle_left_keys(app, code);
    app.clamp_cursor();
    Action::None
}

pub fn handle_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) {
    let y = mouse.row;

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let panel_top = 0u16; // no ratatui border — content starts at row 0
            let combo_count = app.combo_items.len() as u16;
            if y >= panel_top && y < panel_top + combo_count {
                let idx = (y - panel_top) as usize;
                if idx < app.combo_items.len() {
                    app.cursor = idx;
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if app.cursor > 0 { app.cursor -= 1; }
        }
        MouseEventKind::ScrollDown => {
            let len = app.current_list_len();
            if app.cursor + 1 < len { app.cursor += 1; }
        }
        _ => {}
    }
}

fn handle_left_keys(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            if app.cursor > 0 { app.cursor -= 1; }
            app.combo_log_idx = 0;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let len = app.current_list_len();
            if app.cursor + 1 < len { app.cursor += 1; }
            app.combo_log_idx = 0;
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            app.do_toggle();
        }
        KeyCode::Char('s') => {
            app.do_start();
        }
        KeyCode::Char('o') => {
            app.do_open_url();
        }
        KeyCode::Char('B') => {
            app.do_recreate_db();
        }
        KeyCode::Char('x') => {
            app.do_stop();
        }
        KeyCode::Char('X') => {
            app.popup_confirm(
                "Stop ALL services?",
                super::app::ConfirmAction::StopAll,
            );
        }
        KeyCode::Char('r') => {
            let failed_pipeline = app.active_pipelines.iter()
                .position(|p| p.failed.is_some());
            if let Some(idx) = failed_pipeline {
                let pipeline = app.active_pipelines.remove(idx);
                if let Some((failed_stage, _)) = &pipeline.failed {
                    let branch = pipeline.branch.clone();
                    let operation = pipeline.operation.clone();
                    let failed = *failed_stage;

                    if let Some(tx) = app.event_tx.clone() {
                        if operation.contains("Creating") || operation.contains("Retrying") {
                            let ws_name = app.combos.first().cloned().unwrap_or_default();
                            use crate::pipeline;
                            use std::collections::HashSet;
                            let skip: HashSet<usize> = (0..failed).collect();
                            if let Ok(ctx) = pipeline::context::CreateContext::from_config(
                                &app.config, &app.config_path, &ws_name, &branch, skip,
                            ) {
                                app.active_pipelines.push(super::app::PipelineDisplay {
                                    operation: "Retrying workspace".into(),
                                    branch: branch.clone(),
                                    current_stage: failed,
                                    total_stages: 7,
                                    stage_name: "Resuming...".into(),
                                    failed: None,
                                });
                                std::thread::spawn(move || {
                                    let (ptx, prx) = std::sync::mpsc::channel();
                                    std::thread::spawn(move || {
                                        while let Ok(evt) = prx.recv() {
                                            if tx.send(AppEvent::Pipeline(evt)).is_err() { break; }
                                        }
                                    });
                                    pipeline::run_create_pipeline(ctx, ptx);
                                });
                                app.set_message(&format!("retrying from stage {}...", failed + 1));
                            }
                        }
                    }
                }
            } else if app.active_pipelines.is_empty() {
                app.do_restart();
            }
        }
        KeyCode::Char('n') => {
            app.cycle_combo_log(1);
        }
        KeyCode::Char('N') => {
            app.cycle_combo_log(-1);
        }
        KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
            if let Some(rpid) = &app.right_pane_id {
                crate::tmux::select_pane(rpid);
            }
        }
        _ => {}
    }
}
