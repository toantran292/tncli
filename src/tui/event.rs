use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, MouseButton, MouseEventKind};

use super::app::{App, Focus, Section};

// ── App-level actions returned to the main loop ──

pub enum Action {
    None,
    Quit,
    Attach,
    EnterCopyMode,
    ExitCopyMode,
}

// ── Event channel (background thread → main loop) ──

pub enum AppEvent {
    /// Terminal event from crossterm
    Terminal(Event),
    /// Periodic tick for refreshing status/logs
    Tick,
}

pub struct EventHandler {
    rx: mpsc::Receiver<AppEvent>,
    _thread: thread::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();

        let thread = thread::spawn(move || {
            loop {
                // Wait for event or tick timeout
                if event::poll(tick_rate).unwrap_or(false) {
                    // Drain up to 64 events per batch
                    for _ in 0..64 {
                        if let Ok(evt) = event::read() {
                            if tx.send(AppEvent::Terminal(evt)).is_err() {
                                return; // main thread dropped receiver
                            }
                        }
                        // Check for more pending events
                        if !event::poll(Duration::ZERO).unwrap_or(false) {
                            break;
                        }
                    }
                } else {
                    // Timeout → send tick
                    if tx.send(AppEvent::Tick).is_err() {
                        return;
                    }
                }
            }
        });

        Self { rx, _thread: thread }
    }

    /// Non-blocking: drain all pending events.
    pub fn drain(&self) -> Vec<AppEvent> {
        let mut events = Vec::new();
        while let Ok(evt) = self.rx.try_recv() {
            events.push(evt);
        }
        events
    }

    /// Blocking: wait for next event.
    pub fn next(&self) -> anyhow::Result<AppEvent> {
        Ok(self.rx.recv()?)
    }
}

// ── Key/Mouse dispatch ──

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    let code = key.code;

    // Interactive mode — forward keys to tmux pane
    if app.interactive_mode {
        if code == KeyCode::Esc {
            app.interactive_mode = false;
            app.set_message("interactive mode off");
            return Action::None;
        }
        if let Some(svc) = app.selected_service_for_logs().map(|s| s.to_string()) {
            let tmux_key: Option<String> = match code {
                KeyCode::Char(c) => Some(c.to_string()),
                KeyCode::Enter => Some("Enter".into()),
                KeyCode::Backspace => Some("BSpace".into()),
                KeyCode::Tab => Some("Tab".into()),
                KeyCode::Up => Some("Up".into()),
                KeyCode::Down => Some("Down".into()),
                KeyCode::Left => Some("Left".into()),
                KeyCode::Right => Some("Right".into()),
                KeyCode::Home => Some("Home".into()),
                KeyCode::End => Some("End".into()),
                KeyCode::Delete => Some("DC".into()),
                _ => None,
            };
            if let Some(k) = tmux_key {
                crate::tmux::send_keys(&app.session, &svc, &[&k]);
                app.log_scroll = 0;
                app.invalidate_log();
                app.invalidate_parsed();
            }
        }
        return Action::None;
    }

    // Search input mode
    if app.search_mode {
        match code {
            KeyCode::Esc => {
                app.search_mode = false;
                app.search_query.clear();
                app.search_matches.clear();
                app.invalidate_parsed();
            }
            KeyCode::Enter => {
                app.search_mode = false;
                if !app.search_query.is_empty() {
                    app.update_search_matches();
                    let count = app.search_matches.len();
                    if count > 0 {
                        app.jump_to_match(0, 20);
                        app.set_message(&format!("{count} matches found"));
                    } else {
                        app.set_message("no matches found");
                    }
                }
            }
            KeyCode::Backspace => {
                app.search_query.pop();
            }
            KeyCode::Char(c) => {
                app.search_query.push(c);
            }
            _ => {}
        }
        return Action::None;
    }

    // Copy mode
    if app.copy_mode {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => return Action::ExitCopyMode,
            KeyCode::Up | KeyCode::Char('k') => app.scroll_up(1),
            KeyCode::Down | KeyCode::Char('j') => app.scroll_down(1),
            KeyCode::PageUp => app.scroll_up(20),
            KeyCode::PageDown => app.scroll_down(20),
            KeyCode::Char('G') => app.scroll_to_bottom(),
            KeyCode::Char('g') => app.scroll_to_top(),
            KeyCode::Char('/') => {
                app.search_mode = true;
                app.search_query.clear();
                app.search_matches.clear();
                app.invalidate_parsed();
            }
            KeyCode::Char('n') => app.jump_to_match(1, 20),
            KeyCode::Char('N') => app.jump_to_match(-1, 20),
            _ => {}
        }
        return Action::None;
    }

    // Global keys
    match code {
        KeyCode::Esc => {
            if !app.search_query.is_empty() {
                app.search_query.clear();
                app.search_matches.clear();
                app.invalidate_parsed();
            }
            return Action::None;
        }
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Char('a') => return Action::Attach,
        KeyCode::Char('R') => {
            app.reload_config();
            app.refresh_status();
            app.set_message("config reloaded");
            return Action::None;
        }
        _ => {}
    }

    if app.focus == Focus::Right && code == KeyCode::Char('y') {
        return Action::EnterCopyMode;
    }

    if app.focus == Focus::Right && code == KeyCode::Char('i') {
        app.interactive_mode = true;
        app.log_scroll = 0;
        app.set_message("interactive mode — type to send keys to pane (Esc to exit)");
        return Action::None;
    }

    if app.focus == Focus::Right && code == KeyCode::Char('/') {
        app.search_mode = true;
        app.search_query.clear();
        app.search_matches.clear();
        app.invalidate_parsed();
        return Action::None;
    }

    if app.focus == Focus::Right && !app.search_query.is_empty() {
        if code == KeyCode::Char('n') {
            app.jump_to_match(1, 20);
            return Action::None;
        }
        if code == KeyCode::Char('N') {
            app.jump_to_match(-1, 20);
            return Action::None;
        }
    }

    match app.focus {
        Focus::Left => handle_left_keys(app, code),
        Focus::Right => handle_right_keys(app, code),
    }

    app.clamp_cursor();
    Action::None
}

pub fn handle_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) {
    let x = mouse.column;
    let y = mouse.row;

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if x < LEFT_W {
                let panel_top = 3u16;
                let svc_count = app.services.len() as u16;
                let combo_start = panel_top + svc_count + 2;

                if y >= panel_top && y < panel_top + svc_count {
                    let idx = (y - panel_top) as usize;
                    if idx < app.services.len() {
                        app.focus = Focus::Left;
                        app.section = Section::Services;
                        app.cursor = idx;
                        app.log_scroll = 0;
                        app.invalidate_log();
                    }
                } else if y >= combo_start && y < combo_start + app.combos.len() as u16 {
                    let idx = (y - combo_start) as usize;
                    if idx < app.combos.len() {
                        app.focus = Focus::Left;
                        app.section = Section::Combos;
                        app.cursor = idx;
                        app.log_scroll = 0;
                        app.invalidate_log();
                    }
                }
            } else {
                app.focus = Focus::Right;
            }
        }
        MouseEventKind::ScrollUp => {
            if x < LEFT_W {
                app.focus = Focus::Left;
                if app.cursor > 0 {
                    app.cursor -= 1;
                } else if app.section == Section::Combos {
                    app.section = Section::Services;
                    app.cursor = app.services.len().saturating_sub(1);
                }
                app.invalidate_log();
            } else {
                app.focus = Focus::Right;
                app.scroll_up(3);
            }
        }
        MouseEventKind::ScrollDown => {
            if x < LEFT_W {
                app.focus = Focus::Left;
                let len = app.current_list().len();
                if app.cursor + 1 < len {
                    app.cursor += 1;
                } else if app.section == Section::Services && !app.combos.is_empty() {
                    app.section = Section::Combos;
                    app.cursor = 0;
                }
                app.invalidate_log();
            } else {
                app.focus = Focus::Right;
                app.scroll_down(3);
            }
        }
        _ => {}
    }
}

fn handle_left_keys(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            if app.cursor > 0 {
                app.cursor -= 1;
            } else if app.section == Section::Combos {
                app.section = Section::Services;
                app.cursor = app.services.len().saturating_sub(1);
            }
            app.log_scroll = 0;
            app.combo_log_idx = 0;
            app.invalidate_log();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let len = app.current_list().len();
            if app.cursor + 1 < len {
                app.cursor += 1;
            } else if app.section == Section::Services && !app.combos.is_empty() {
                app.section = Section::Combos;
                app.cursor = 0;
            }
            app.log_scroll = 0;
            app.combo_log_idx = 0;
            app.invalidate_log();
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            app.do_toggle();
            app.invalidate_log();
            app.last_log_size = (0, 0);
        }
        KeyCode::Char('s') => {
            app.do_start();
            app.invalidate_log();
            app.last_log_size = (0, 0);
        }
        KeyCode::Char('x') => {
            app.do_stop();
            app.invalidate_log();
        }
        KeyCode::Char('X') => {
            app.do_stop_all();
            app.invalidate_log();
        }
        KeyCode::Char('r') => {
            app.do_restart();
            app.invalidate_log();
            app.last_log_size = (0, 0);
        }
        KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
            app.focus = Focus::Right;
            app.log_scroll = 0;
        }
        _ => {}
    }
}

fn handle_right_keys(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => app.scroll_up(1),
        KeyCode::Down | KeyCode::Char('j') => app.scroll_down(1),
        KeyCode::PageUp => app.scroll_up(20),
        KeyCode::PageDown => app.scroll_down(20),
        KeyCode::Char('G') => app.scroll_to_bottom(),
        KeyCode::Char('g') => app.scroll_to_top(),
        KeyCode::Char('n') => app.cycle_combo_log(1),
        KeyCode::Char('N') => app.cycle_combo_log(-1),
        KeyCode::Tab | KeyCode::Char('h') | KeyCode::Left => {
            app.focus = Focus::Left;
        }
        _ => {}
    }
}

const LEFT_W: u16 = 28;
