mod ansi;
pub mod app;
mod event;
mod screens;
mod ui;

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use ratatui::DefaultTerminal;

use crate::config;
use app::App;
use event::{Action, AppEvent, EventHandler, drain_crossterm};

/// Install panic hook that restores terminal + writes crash log.
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

    // If not in tmux, auto-enter: create window in service session and attach
    if !crate::tmux::in_tmux() {
        let config_path = config::find_config()?;
        let cfg = crate::config::Config::load(&config_path)?;
        return auto_enter_tmux(&cfg.session);
    }

    let config_path = config::find_config()?;
    let mut app = App::new(config_path)?;
    app.refresh_status();

    // Setup tmux split-pane mode
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

    // Cleanup split-pane
    app.teardown_split();

    if let Err(ref e) = result {
        write_crash_log(&format!("Error: {e:#}"));
    }

    result
}

/// Auto-enter tmux: TUI runs in session "tncli" (window = config session name).
/// Services run in session "tncli_{session}" (e.g. "tncli_boom").
fn auto_enter_tmux(session: &str) -> Result<()> {
    let exe = std::env::current_exe()?.to_string_lossy().to_string();
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();
    let tui_session = "tncli";

    // Gracefully quit stale tncli instance for this project
    if crate::tmux::window_exists(tui_session, session) {
        let panes = crate::tmux::list_pane_ids(&format!("={tui_session}:{session}"));
        if let Some(first) = panes.first() {
            let _ = std::process::Command::new("tmux")
                .args(["send-keys", "-t", first, "q"])
                .output();
        }
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if !crate::tmux::window_exists(tui_session, session) { break; }
        }
        if crate::tmux::window_exists(tui_session, session) {
            crate::tmux::kill_window(tui_session, session);
        }
    }

    // Create/reuse "tncli" session, add window for this project
    let cmd = format!("{exe} ui; tmux detach-client 2>/dev/null");
    if crate::tmux::session_exists(tui_session) {
        // Session exists (maybe other projects) — add window
        let _ = std::process::Command::new("tmux")
            .args([
                "new-window", "-d",
                "-t", &format!("={tui_session}"),
                "-c", &cwd,
                "-n", session,
                "zsh", "-c", &cmd,
            ])
            .output();
    } else {
        // Create session with this project as first window
        let _ = std::process::Command::new("tmux")
            .args([
                "new-session", "-d",
                "-s", tui_session,
                "-c", &cwd,
                "-n", session,
                "zsh", "-c", &cmd,
            ])
            .output();
    }

    // Attach to tncli session, focusing this project's window
    let _ = std::process::Command::new("tmux")
        .args(["attach-session", "-t", &format!("={tui_session}:{session}")])
        .status();

    Ok(())
}

fn run_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    let mut events = EventHandler::new(Duration::from_secs(1));
    app.event_tx = Some(events.event_tx());
    let mut prev_focus = app.focus;
    let mut prev_cursor = app.cursor;
    let mut prev_running_count = app.running_windows.len();

    loop {
        // Auto toggle mouse based on focus
        if app.focus != prev_focus {
            match app.focus {
                app::Focus::Left => { execute!(std::io::stdout(), EnableMouseCapture)?; }
                app::Focus::Right => { execute!(std::io::stdout(), DisableMouseCapture)?; }
            }
            prev_focus = app.focus;
        }

        // Draw
        terminal.draw(|f| ui::draw(f, app))?;

        // Wait for next event (blocking — CPU idle while waiting)
        let first = events.next()?;
        // Then drain any remaining buffered events
        let mut batch = vec![first];
        batch.extend(events.drain());

        // Process all events in batch
        let mut action = Action::None;
        let mut got_tick = false;
        for evt in batch {
            match evt {
                AppEvent::Tick => {
                    got_tick = true; // defer — handle once after all events
                }
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

        // Handle tick once (refresh status + log, not per-tick)
        if got_tick {
            app.refresh_status();
            if app.is_split_mode() {
                app.ensure_split();
            }
            if !app.copy_mode {
                app.invalidate_log();
            }
        }

        // Split-pane: swap on cursor change, status change, or n/N cycle
        if app.is_split_mode() {
            let cursor_changed = app.cursor != prev_cursor;
            let status_changed = app.running_windows.len() != prev_running_count;
            if cursor_changed || status_changed || app.swap_pending {
                prev_cursor = app.cursor;
                prev_running_count = app.running_windows.len();
                app.swap_pending = false;
                app.swap_display_service();
            }
        }

        // Handle actions that need terminal access
        match action {
            Action::Quit => break,
            Action::Attach => {
                // Teardown split before leaving (so service windows appear in service session)
                app.teardown_split();

                // Stop event thread before leaving TUI
                drop(events);

                let _ = execute!(std::io::stdout(), DisableMouseCapture);
                ratatui::restore();

                let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
                crate::tmux::resize_all_windows(&app.svc_session(), term_w, term_h);
                app.last_log_size = (0, 0);

                let target = app.selected_service_name();
                let _ = crate::tmux::attach(&app.svc_session(), target.as_deref());

                // Re-enter TUI + restart event thread
                *terminal = ratatui::init();
                if app.focus == app::Focus::Left {
                    execute!(std::io::stdout(), EnableMouseCapture)?;
                }
                prev_focus = app.focus;
                drain_crossterm();
                events = EventHandler::new(Duration::from_secs(1));
                app.event_tx = Some(events.event_tx());
                app.refresh_status();
                app.invalidate_log();
                app.last_log_size = (0, 0);

                // Re-create split
                app.setup_split();
            }
            Action::OpenShell => {
                let dir = app.selected_dir_name().and_then(|d|
                    app.selected_work_dir(&d).or_else(|| app.dir_path(&d))
                );
                if let Some(dir) = dir {
                    app.teardown_split();
                    drop(events);
                    let _ = execute!(std::io::stdout(), DisableMouseCapture);
                    ratatui::restore();

                    let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
                    crate::tmux::resize_all_windows(&app.svc_session(), term_w, term_h);
                    let _ = crate::tmux::open_shell(&app.svc_session(), &dir);

                    *terminal = ratatui::init();
                    if app.focus == app::Focus::Left {
                        execute!(std::io::stdout(), EnableMouseCapture)?;
                    }
                    prev_focus = app.focus;
                    drain_crossterm();
                    events = EventHandler::new(Duration::from_secs(1));
                    app.event_tx = Some(events.event_tx());
                    app.refresh_status();
                    app.invalidate_log();
                    app.last_log_size = (0, 0);
                    app.setup_split();
                } else {
                    app.set_message("no service selected");
                }
            }
            Action::RunShortcut => {
                if let Some((cmd, desc, dir)) = app.selected_shortcut() {
                    app.teardown_split();
                    drop(events);
                    let _ = execute!(std::io::stdout(), DisableMouseCapture);
                    ratatui::restore();

                    let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
                    crate::tmux::resize_all_windows(&app.svc_session(), term_w, term_h);
                    let _ = crate::tmux::run_in_window(&app.svc_session(), &dir, &cmd, &desc);

                    *terminal = ratatui::init();
                    if app.focus == app::Focus::Left {
                        execute!(std::io::stdout(), EnableMouseCapture)?;
                    }
                    prev_focus = app.focus;
                    drain_crossterm();
                    events = EventHandler::new(Duration::from_secs(1));
                    app.event_tx = Some(events.event_tx());
                    app.refresh_status();
                    app.invalidate_log();
                    app.last_log_size = (0, 0);
                    app.setup_split();
                    app.set_message(&format!("finished: {desc}"));
                }
            }
            Action::EnterCopyMode => {
                app.copy_mode = true;
                app.invalidate_log();
                execute!(std::io::stdout(), DisableMouseCapture)?;
            }
            Action::ExitCopyMode => {
                app.copy_mode = false;
                if app.focus == app::Focus::Left {
                    execute!(std::io::stdout(), EnableMouseCapture)?;
                }
            }
            Action::None => {}
        }
    }
    Ok(())
}
