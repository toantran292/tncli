use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, MouseButton, MouseEventKind};

use super::app::{App, ComboItem, Focus};

// ── App-level actions returned to the main loop ──

pub enum Action {
    None,
    Quit,
    Attach,
    OpenShell,
    RunShortcut,
    EnterCopyMode,
    ExitCopyMode,
}

// ── Event channel (background thread → main loop) ──

pub enum AppEvent {
    /// Terminal event from crossterm
    Terminal(Event),
    /// Periodic tick for refreshing status/logs
    Tick,
    /// Pipeline progress event
    Pipeline(crate::pipeline::PipelineEvent),
    /// Background worktree scan result
    WorktreeScanResult(std::collections::HashMap<String, crate::services::WorktreeInfo>),
    /// Message from background thread
    Message(String),
}

pub struct EventHandler {
    rx: mpsc::Receiver<AppEvent>,
    tx: mpsc::Sender<AppEvent>,
    _thread: thread::JoinHandle<()>,
}

/// Drain any buffered crossterm events (call after re-entering TUI).
pub fn drain_crossterm() {
    while event::poll(Duration::ZERO).unwrap_or(false) {
        let _ = event::read();
    }
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        let external_tx = tx.clone();

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

        Self { rx, tx: external_tx, _thread: thread }
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

    /// Get a sender clone for external threads (pipeline, etc.)
    pub fn event_tx(&self) -> mpsc::Sender<AppEvent> {
        self.tx.clone()
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
        if let Some(svc) = app.log_service_name() {
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

    // Shortcuts popup
    if app.shortcuts_open {
        let max = app.shortcuts_count();
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('c') => {
                app.shortcuts_open = false;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if app.shortcuts_cursor > 0 {
                    app.shortcuts_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.shortcuts_cursor + 1 < max {
                    app.shortcuts_cursor += 1;
                }
            }
            KeyCode::Enter => {
                app.shortcuts_open = false;
                return Action::RunShortcut;
            }
            _ => {}
        }
        return Action::None;
    }

    // Branch name input
    // Confirm dialog (y/n)
    if app.confirm_open {
        match code {
            KeyCode::Char('y') | KeyCode::Enter => {
                app.execute_confirm();
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.confirm_open = false;
                app.set_message("cancelled");
            }
            _ => {}
        }
        return Action::None;
    }

    if app.wt_name_input_open {
        match code {
            KeyCode::Esc => app.wt_name_input_open = false,
            KeyCode::Enter => {
                app.confirm_wt_name();
            }
            KeyCode::Backspace => { app.wt_name_input.pop(); }
            KeyCode::Char(c) => app.wt_name_input.push(c),
            _ => {}
        }
        return Action::None;
    }

    // Workspace repo selection checklist
    if app.ws_select_open {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => app.ws_select_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                if app.ws_select_cursor > 0 { app.ws_select_cursor -= 1; }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.ws_select_cursor + 1 < app.ws_select_items.len() { app.ws_select_cursor += 1; }
            }
            KeyCode::Char(' ') => {
                if let Some(item) = app.ws_select_items.get_mut(app.ws_select_cursor) {
                    item.selected = !item.selected;
                    if !item.selected { item.branch = "-".to_string(); }
                    else { item.branch = app.ws_select_branch.clone(); }
                }
            }
            KeyCode::Char('b') => {
                // Change branch for selected repo — open branch picker
                if let Some(item) = app.ws_select_items.get(app.ws_select_cursor) {
                    if item.selected {
                        let dir_name = item.dir_name.clone();
                        let dir_path = app.dir_path(&dir_name).unwrap_or_default();
                        match crate::services::list_branches(std::path::Path::new(&dir_path)) {
                            Ok(branches) => {
                                if branches.is_empty() {
                                    app.set_message("no branches found");
                                } else {
                                    app.wt_branches = branches.clone();
                                    app.wt_branch_filtered = branches;
                                    app.wt_branch_search.clear();
                                    app.wt_branch_searching = false;
                                    app.wt_branch_cursor = 0;
                                    app.wt_branch_dir = dir_name;
                                    app.branch_checkout_mode = false; // reuse picker for branch selection
                                    app.wt_branch_open = true;
                                }
                            }
                            Err(e) => app.set_message(&format!("git error: {e}")),
                        }
                    } else {
                        app.set_message("enable repo first (Space)");
                    }
                }
            }
            KeyCode::Enter => {
                let selected: Vec<_> = app.ws_select_items.iter()
                    .filter(|i| i.selected)
                    .collect();
                if selected.is_empty() {
                    app.set_message("select at least one repo");
                } else {
                    let ws_name = app.ws_name.clone();
                    let branch = app.ws_select_branch.clone();
                    if let Some(tx) = app.event_tx.clone() {
                        let msg = app.start_create_pipeline(&ws_name, &branch, tx);
                        app.set_message(&msg);
                    }
                }
            }
            _ => {}
        }
        return Action::None;
    }

    // Workspace edit menu (add/remove repo)
    if app.ws_edit_open {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => app.ws_edit_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                if app.ws_edit_cursor > 0 { app.ws_edit_cursor -= 1; }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.ws_edit_cursor < 1 { app.ws_edit_cursor += 1; }
            }
            KeyCode::Enter => {
                let branch = app.ws_edit_branch.clone();
                app.ws_edit_open = false;
                match app.ws_edit_cursor {
                    0 => app.build_ws_add_list(&branch),
                    1 => app.build_ws_remove_list(&branch),
                    _ => {}
                }
            }
            _ => {}
        }
        return Action::None;
    }

    // Add repo picker
    if app.ws_add_open {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => app.ws_add_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                if app.ws_add_cursor > 0 { app.ws_add_cursor -= 1; }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.ws_add_cursor + 1 < app.ws_add_items.len() { app.ws_add_cursor += 1; }
            }
            KeyCode::Char('b') => {
                // Change branch for repo being added
                if let Some(item) = app.ws_add_items.get(app.ws_add_cursor) {
                    let dir_name = item.dir_name.clone();
                    let dir_path = app.dir_path(&dir_name).unwrap_or_default();
                    match crate::services::list_branches(std::path::Path::new(&dir_path)) {
                        Ok(branches) => {
                            if !branches.is_empty() {
                                app.wt_branches = branches.clone();
                                app.wt_branch_filtered = branches;
                                app.wt_branch_search.clear();
                                app.wt_branch_searching = false;
                                app.wt_branch_cursor = 0;
                                app.wt_branch_dir = dir_name;
                                app.branch_checkout_mode = false;
                                app.wt_branch_open = true;
                            }
                        }
                        Err(e) => app.set_message(&format!("git error: {e}")),
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(item) = app.ws_add_items.get(app.ws_add_cursor).cloned() {
                    app.ws_add_open = false;
                    let branch = app.ws_edit_branch.clone();
                    app.add_repo_to_workspace(&item.dir_name, &branch, &item.branch);
                }
            }
            _ => {}
        }
        return Action::None;
    }

    // Remove repo picker
    if app.ws_remove_open {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => app.ws_remove_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                if app.ws_remove_cursor > 0 { app.ws_remove_cursor -= 1; }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.ws_remove_cursor + 1 < app.ws_remove_items.len() { app.ws_remove_cursor += 1; }
            }
            KeyCode::Enter => {
                if let Some((_, wt_key)) = app.ws_remove_items.get(app.ws_remove_cursor).cloned() {
                    app.ws_remove_open = false;
                    let msg = app.delete_worktree(&wt_key);
                    app.set_message(&msg);
                }
            }
            _ => {}
        }
        return Action::None;
    }

    // Cheat-sheet popup
    if app.cheatsheet_open {
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => app.cheatsheet_open = false,
            _ => {}
        }
        return Action::None;
    }

    // Shared services info popup
    if app.shared_info_open {
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('I') => app.shared_info_open = false,
            _ => {}
        }
        return Action::None;
    }

    // Branch menu (checkout/create/fetch)
    if app.branch_menu_open {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => app.branch_menu_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                if app.branch_menu_cursor > 0 { app.branch_menu_cursor -= 1; }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.branch_menu_cursor < 2 { app.branch_menu_cursor += 1; }
            }
            KeyCode::Enter => {
                match app.branch_menu_cursor {
                    0 => {
                        // Checkout existing branch (reuse branch picker)
                        app.open_checkout_picker();
                    }
                    1 => {
                        // Create new branch
                        app.branch_menu_open = false;
                        app.wt_name_input.clear();
                        app.wt_name_input_open = true;
                        app.wt_name_base_branch = "new-branch".to_string();
                        app.wt_menu_dir = app.branch_menu_dir.clone();
                    }
                    2 => {
                        // Pull current branch
                        let dir = app.branch_menu_dir.clone();
                        app.branch_menu_open = false;
                        let branch = app.dir_branch(&dir).unwrap_or_else(|| "main".to_string());
                        let msg = app.git_pull_branch(&dir, &branch);
                        app.set_message(&msg);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        return Action::None;
    }

    // Worktree menu
    if app.wt_menu_open {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => app.wt_menu_open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                if app.wt_menu_cursor > 0 { app.wt_menu_cursor -= 1; }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.wt_menu_cursor < 4 { app.wt_menu_cursor += 1; }
            }
            KeyCode::Enter => {
                match app.wt_menu_cursor {
                    0 => app.create_wt_current_branch(),       // Current branch (fast)
                    1 => app.open_branch_picker(),              // Pick branch
                    2 => { app.scan_worktrees(); app.set_message("worktrees refreshed"); app.wt_menu_open = false; }
                    3 => {
                        let dir = app.wt_menu_dir.clone();
                        let msg = app.setup_main_loopback(&dir);
                        app.set_message(&msg);
                        app.wt_menu_open = false;
                    }
                    4 => {
                        if let Some(ComboItem::InstanceDir { wt_key, branch, is_main: false, .. }) = app.current_combo_item().cloned() {
                            app.wt_menu_open = false;
                            app.ask_confirm(
                                &format!("Delete worktree '{branch}'? (y/n)"),
                                super::app::ConfirmAction::DeleteWorktree { wt_key },
                            );
                        } else {
                            app.set_message("select a worktree to delete");
                            app.wt_menu_open = false;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        return Action::None;
    }

    // Branch picker (with search)
    if app.wt_branch_open {
        if app.wt_branch_searching {
            // Search mode — capture text
            match code {
                KeyCode::Esc => {
                    app.wt_branch_searching = false;
                    app.wt_branch_search.clear();
                    app.filter_branches();
                }
                KeyCode::Enter => {
                    app.wt_branch_searching = false;
                }
                KeyCode::Backspace => {
                    app.wt_branch_search.pop();
                    app.filter_branches();
                }
                KeyCode::Char(c) => {
                    app.wt_branch_search.push(c);
                    app.filter_branches();
                }
                _ => {}
            }
        } else {
            // Navigation mode
            match code {
                KeyCode::Esc | KeyCode::Char('q') => app.wt_branch_open = false,
                KeyCode::Char('/') => {
                    app.wt_branch_searching = true;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.wt_branch_cursor > 0 { app.wt_branch_cursor -= 1; }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if app.wt_branch_cursor + 1 < app.wt_branch_filtered.len() { app.wt_branch_cursor += 1; }
                }
                KeyCode::Enter => {
                    if let Some(branch) = app.wt_branch_filtered.get(app.wt_branch_cursor).cloned() {
                        let dir = app.wt_branch_dir.clone();
                        if app.ws_select_open {
                            // Update branch in ws_select checklist
                            if let Some(item) = app.ws_select_items.iter_mut().find(|i| i.dir_name == dir) {
                                item.branch = branch;
                                item.selected = true;
                            }
                            app.update_ws_select_conflicts();
                        } else if app.ws_add_open {
                            // Update branch in add-repo picker
                            if let Some(item) = app.ws_add_items.iter_mut().find(|i| i.dir_name == dir) {
                                item.branch = branch;
                            }
                        } else if app.branch_checkout_mode {
                            let msg = app.git_checkout(&dir, &branch);
                            app.set_message(&msg);
                        } else {
                            let msg = app.create_worktree(&dir, &branch);
                            app.set_message(&msg);
                        }
                    }
                    app.wt_branch_open = false;
                }
                _ => {}
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
        KeyCode::Char('?') => {
            app.cheatsheet_open = true;
            return Action::None;
        }
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Char('a') => return Action::Attach,
        KeyCode::Char('t') => return Action::OpenShell,
        KeyCode::Char('c') => {
            app.open_shortcuts();
            return Action::None;
        }
        KeyCode::Char('b') => {
            app.open_branch_menu();
            return Action::None;
        }
        KeyCode::Char('e') => {
            app.open_editor();
            return Action::None;
        }
        KeyCode::Char('I') => {
            if !app.config.shared_services.is_empty() {
                app.shared_info_open = true;
            } else {
                app.set_message("no shared services configured");
            }
            return Action::None;
        }
        KeyCode::Char('w') | KeyCode::Char('W') => {
            // On Combo row → create workspace
            if let Some(ComboItem::Combo(ws_name)) = app.current_combo_item().cloned() {
                app.ws_creating = true;
                app.ws_name = ws_name;
                app.wt_name_input.clear();
                app.wt_name_input_open = true;
                app.wt_name_base_branch = "workspace".to_string();
                return Action::None;
            }
            // On Instance main → create workspace (same as Combo)
            if let Some(ComboItem::Instance { is_main: true, .. }) = app.current_combo_item() {
                let ws_name = app.find_parent_combo(app.cursor);
                if !ws_name.is_empty() {
                    app.ws_creating = true;
                    app.ws_name = ws_name;
                    app.wt_name_input.clear();
                    app.wt_name_input_open = true;
                    app.wt_name_base_branch = "workspace".to_string();
                }
                return Action::None;
            }
            // On Instance non-main → workspace edit menu (add/remove repo)
            if let Some(ComboItem::Instance { branch, is_main: false }) = app.current_combo_item().cloned() {
                app.ws_edit_branch = branch;
                app.ws_edit_cursor = 0;
                app.ws_edit_open = true;
                return Action::None;
            }
            // Otherwise → worktree menu
            app.open_wt_menu();
            return Action::None;
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            // Delete workspace instance (with confirm)
            if let Some(ComboItem::Instance { branch, is_main: false }) = app.current_combo_item().cloned() {
                app.ask_confirm(
                    &format!("Delete workspace '{branch}'? (y/n)"),
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
                let panel_top = 2u16;
                let combo_count = app.combo_items.len() as u16;
                if y >= panel_top && y < panel_top + combo_count {
                    let idx = (y - panel_top) as usize;
                    if idx < app.combo_items.len() {
                        app.focus = Focus::Left;
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
                let len = app.current_list_len();
                if app.cursor + 1 < len {
                    app.cursor += 1;
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
            }
            app.log_scroll = 0;
            app.combo_log_idx = 0;
            app.invalidate_log();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let len = app.current_list_len();
            if app.cursor + 1 < len {
                app.cursor += 1;
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
            app.ask_confirm(
                "Stop ALL services? (y/n)",
                super::app::ConfirmAction::StopAll,
            );
        }
        KeyCode::Char('r') => {
            // If any pipeline failed, retry the first failed one
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
                app.invalidate_log();
                app.last_log_size = (0, 0);
            }
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
