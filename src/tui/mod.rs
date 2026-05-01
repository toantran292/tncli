pub mod app;
pub(crate) mod app_collapse;
mod app_editor;
mod app_split;
mod app_status;
mod event;
mod popups;
mod screens;
mod ui;

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use ratatui::DefaultTerminal;

use crate::config;
use app::App;
use event::{Action, AppEvent, EventHandler};

fn install_hooks() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = ratatui::restore();
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
        write_crash_log(&format!("{info}"));
        original(info);
    }));
}

fn write_crash_log(info: &str) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let dir = format!("{home}/.tncli");
    let _ = std::fs::create_dir_all(&dir);
    let path = format!("{dir}/crash.log");
    let timestamp = std::process::Command::new("date")
        .arg("+%Y-%m-%d %H:%M:%S")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {info}", timestamp.trim());
    }
}

pub fn run_tui() -> Result<()> {
    install_hooks();

    if !crate::tmux::in_tmux() {
        let config_path = config::find_config()?;
        let cfg = crate::config::Config::load(&config_path)?;
        return auto_enter_tmux(&cfg.session);
    }

    let config_path = config::find_config()?;
    let mut app = App::new(config_path)?;
    app.refresh_status();

    if let (Some(wid), Some(sess)) = (crate::tmux::current_window_id(), crate::tmux::current_session_name()) {
        app.tui_window_id = Some(wid);
        app.tui_session = Some(sess);
        app.setup_split();
    }

    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;

    let result = run_loop(&mut terminal, &mut app);

    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();

    app.teardown_split();

    if let Err(ref e) = result {
        write_crash_log(&format!("Error: {e:#}"));
    }

    result
}

fn auto_enter_tmux(session: &str) -> Result<()> {
    let exe = std::env::current_exe()?.to_string_lossy().to_string();
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();
    let tui_session = "tncli";

    if crate::tmux::window_exists(tui_session, session) {
        let panes = crate::tmux::list_pane_ids(&format!("={tui_session}:{session}"));
        if let Some(first) = panes.first() {
            crate::tmux::send_keys(first, "q");
        }
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if !crate::tmux::window_exists(tui_session, session) { break; }
        }
        if crate::tmux::window_exists(tui_session, session) {
            crate::tmux::kill_window(tui_session, session);
        }
    }

    let cmd = format!("{exe} ui; tmux detach-client 2>/dev/null");
    if crate::tmux::session_exists(tui_session) {
        crate::tmux::new_window_in_dir(tui_session, session, &cwd, &cmd);
    } else {
        crate::tmux::new_session_in_dir(tui_session, session, &cwd, &cmd);
    }

    crate::tmux::attach_session(&format!("={tui_session}:{session}"));

    Ok(())
}

fn run_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    let events = EventHandler::new(Duration::from_secs(1));
    app.event_tx = Some(events.event_tx());
    let mut prev_cursor = app.cursor;
    let mut prev_running_count = app.running_windows.len();

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        let first = events.next()?;
        let mut batch = vec![first];
        batch.extend(events.drain());

        let mut action = Action::None;
        let mut got_tick = false;
        for evt in batch {
            match evt {
                AppEvent::Tick => { got_tick = true; }
                AppEvent::Terminal(Event::Key(key)) => {
                    let a = event::handle_key(app, key);
                    match a {
                        Action::None => {}
                        other => { action = other; break; }
                    }
                }
                AppEvent::Terminal(Event::Mouse(mouse)) => {
                    event::handle_mouse(app, mouse);
                }
                AppEvent::Terminal(Event::Resize(_, _)) => {}
                AppEvent::Pipeline(evt) => {
                    app.handle_pipeline_event(evt);
                }
                AppEvent::WorktreeScanResult(worktrees) => {
                    app.apply_scan_result(worktrees);
                }
                AppEvent::Message(msg) => {
                    app.set_message(&msg);
                }
                _ => {}
            }
        }

        if got_tick {
            app.refresh_status();
            app.ensure_split();
            app.poll_popup_result();
        }

        // Swap display on cursor change, status change, or n/N cycle
        let cursor_changed = app.cursor != prev_cursor;
        let status_changed = app.running_windows.len() != prev_running_count;
        if cursor_changed || status_changed || app.swap_pending {
            prev_cursor = app.cursor;
            prev_running_count = app.running_windows.len();
            app.swap_pending = false;
            app.swap_display_service();
        }

        match action {
            Action::Quit => break,
            Action::None => {}
        }
    }
    Ok(())
}
