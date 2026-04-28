mod ansi;
pub mod app;
mod event;
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

    let config_path = config::find_config()?;
    let mut app = App::new(config_path)?;
    app.refresh_status();

    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;

    let result = run_loop(&mut terminal, &mut app);

    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();

    if let Err(ref e) = result {
        write_crash_log(&format!("Error: {e:#}"));
    }

    result
}

fn run_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    let mut events = EventHandler::new(Duration::from_secs(1));
    let mut prev_focus = app.focus;

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
                _ => {}
            }
        }

        // Handle tick once (refresh status + log, not per-tick)
        if got_tick {
            app.refresh_status();
            if !app.copy_mode {
                app.invalidate_log();
            }
        }

        // Handle actions that need terminal access
        match action {
            Action::Quit => break,
            Action::Attach => {
                // Stop event thread before leaving TUI
                drop(events);

                let _ = execute!(std::io::stdout(), DisableMouseCapture);
                ratatui::restore();

                let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
                crate::tmux::resize_all_windows(&app.session, term_w, term_h);
                app.last_log_size = (0, 0);

                let target = app.selected_service_name();
                let _ = crate::tmux::attach(&app.session, target.as_deref());

                // Re-enter TUI + restart event thread
                *terminal = ratatui::init();
                if app.focus == app::Focus::Left {
                    execute!(std::io::stdout(), EnableMouseCapture)?;
                }
                prev_focus = app.focus;
                drain_crossterm();
                events = EventHandler::new(Duration::from_secs(1));
                app.refresh_status();
                app.invalidate_log();
                app.last_log_size = (0, 0);
            }
            Action::OpenShell => {
                let dir = app.selected_dir_name().and_then(|d|
                    app.selected_work_dir(&d).or_else(|| app.dir_path(&d))
                );
                if let Some(dir) = dir {
                    drop(events);
                    let _ = execute!(std::io::stdout(), DisableMouseCapture);
                    ratatui::restore();

                    let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
                    crate::tmux::resize_all_windows(&app.session, term_w, term_h);
                    let _ = crate::tmux::open_shell(&app.session, &dir);

                    *terminal = ratatui::init();
                    if app.focus == app::Focus::Left {
                        execute!(std::io::stdout(), EnableMouseCapture)?;
                    }
                    prev_focus = app.focus;
                    drain_crossterm();
                    events = EventHandler::new(Duration::from_secs(1));
                    app.refresh_status();
                    app.invalidate_log();
                    app.last_log_size = (0, 0);
                } else {
                    app.set_message("no service selected");
                }
            }
            Action::RunShortcut => {
                if let Some((cmd, desc, dir)) = app.selected_shortcut() {
                    drop(events);
                    let _ = execute!(std::io::stdout(), DisableMouseCapture);
                    ratatui::restore();

                    let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
                    crate::tmux::resize_all_windows(&app.session, term_w, term_h);
                    let _ = crate::tmux::run_in_window(&app.session, &dir, &cmd, &desc);

                    *terminal = ratatui::init();
                    if app.focus == app::Focus::Left {
                        execute!(std::io::stdout(), EnableMouseCapture)?;
                    }
                    prev_focus = app.focus;
                    drain_crossterm();
                    events = EventHandler::new(Duration::from_secs(1));
                    app.refresh_status();
                    app.invalidate_log();
                    app.last_log_size = (0, 0);
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
