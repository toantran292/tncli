use anyhow::Result;
use std::collections::HashSet;
use std::process::Command;

pub fn session_exists(session: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", &format!("={session}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn list_windows(session: &str) -> HashSet<String> {
    let output = Command::new("tmux")
        .args([
            "list-windows",
            "-t",
            &format!("={session}"),
            "-F",
            "#{window_name}",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .trim()
            .lines()
            .map(|s| s.to_string())
            .collect(),
        _ => HashSet::new(),
    }
}

pub fn window_exists(session: &str, window: &str) -> bool {
    list_windows(session).contains(window)
}

pub fn create_session_if_needed(session: &str) -> bool {
    if session_exists(session) {
        return false;
    }
    let _ = Command::new("tmux")
        .args(["new-session", "-d", "-s", session, "-n", "_tncli_init"])
        .output();
    // Schedule cleanup of init window after first real window is created
    std::thread::spawn({
        let session = session.to_string();
        move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if window_exists(&session, "_tncli_init") {
                kill_window(&session, "_tncli_init");
            }
        }
    });
    true
}

pub fn cleanup_init_window(session: &str) {
    if window_exists(session, "_tncli_init") {
        kill_window(session, "_tncli_init");
    }
}

pub fn kill_window(session: &str, window: &str) {
    let _ = Command::new("tmux")
        .args(["kill-window", "-t", &format!("={session}:{window}")])
        .output();
}

/// Graceful stop: send Ctrl+C, brief wait, then kill window.
pub fn graceful_stop(session: &str, window: &str) {
    let target = format!("={session}:{window}");
    // Send Ctrl+C
    let _ = Command::new("tmux")
        .args(["send-keys", "-t", &target, "C-c"])
        .output();
    // Brief wait for graceful shutdown (500ms — enough for most Docker containers to start cleanup)
    std::thread::sleep(std::time::Duration::from_millis(500));
    // Kill window
    kill_window(session, window);
}

pub fn kill_session(session: &str) {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", &format!("={session}")])
        .output();
}

pub fn new_window(session: &str, name: &str, shell_cmd: &str) {
    let full_cmd = format!(
        "{shell_cmd}; echo -e '\\n\\033[33m[tncli] process exited. press enter to close.\\033[0m'; read"
    );
    let _ = Command::new("tmux")
        .args([
            "new-window",
            "-d",
            "-t",
            &format!("={session}"),
            "-n",
            name,
            "zsh",
            "-ic",
            &full_cmd,
        ])
        .output();
}

/// Create a new tmux window that auto-closes when command finishes.
/// Uses zsh -ic (interactive) so .zshrc loads (nvm, rvm, etc.).
pub fn new_window_autoclose(session: &str, name: &str, shell_cmd: &str) {
    let _ = Command::new("tmux")
        .args([
            "new-window",
            "-d",
            "-t",
            &format!("={session}"),
            "-n",
            name,
            "zsh",
            "-ic",
            shell_cmd,
        ])
        .output();
}


/// Capture last N lines from scrollback + current visible pane.
pub fn capture_pane(session: &str, window: &str, lines: usize) -> Vec<String> {
    let target = format!("={session}:{window}");
    let start = format!("-{lines}");
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", &target, "-e", "-p", "-S", &start])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let raw = match String::from_utf8(o.stdout) {
                Ok(s) => s,
                Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
            };
            // Cap at requested lines to prevent unbounded allocation
            let result: Vec<String> = raw.lines().map(String::from).collect();
            if result.len() > lines + 100 {
                result[result.len() - lines - 100..].to_vec()
            } else {
                result
            }
        }
        _ => Vec::new(),
    }
}

// ── Split-pane TUI commands ──

/// Check if we're running inside tmux.
pub fn in_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// Get current tmux session name. Returns None if not in tmux.
pub fn current_session_name() -> Option<String> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#{session_name}"])
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

/// Get current tmux window ID (e.g. "@5"). Returns None if not in tmux.
pub fn current_window_id() -> Option<String> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#{window_id}"])
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if id.is_empty() { None } else { Some(id) }
}

/// Get current pane ID (e.g. "%5"). Absolute, independent of pane-base-index.
pub fn current_pane_id() -> Option<String> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#{pane_id}"])
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if id.is_empty() { None } else { Some(id) }
}

/// List all pane IDs in a window.
pub fn list_pane_ids(window_id: &str) -> Vec<String> {
    Command::new("tmux")
        .args(["list-panes", "-t", window_id, "-F", "#{pane_id}"])
        .output()
        .ok()
        .and_then(|o| if o.status.success() {
            Some(String::from_utf8_lossy(&o.stdout).trim().lines().map(String::from).collect())
        } else {
            None
        })
        .unwrap_or_default()
}

/// Split current pane horizontally. Right pane gets `size_pct`% width.
/// Uses -d to keep focus on left (current) pane.
/// If cmd is provided, runs it in the new pane instead of the default shell.
pub fn split_window_right(size_pct: u16, cmd: Option<&str>) -> bool {
    let size = format!("{size_pct}%");
    let mut args = vec!["split-window", "-dh", "-l", &size];
    if let Some(c) = cmd {
        args.push(c);
    }
    Command::new("tmux")
        .args(&args)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Kill pane by ID (e.g. "%10").
pub fn kill_pane(pane_id: &str) {
    let _ = Command::new("tmux")
        .args(["kill-pane", "-t", pane_id])
        .output();
}

/// Break a pane (by ID) back to dest_session as a new window with given name.
pub fn break_pane_to(pane_id: &str, dest_session: &str, window_name: &str) -> bool {
    Command::new("tmux")
        .args([
            "break-pane", "-d",
            "-s", pane_id,
            "-t", &format!("={dest_session}:"),
            "-n", window_name,
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Select (focus) a pane by ID.
pub fn select_pane(pane_id: &str) {
    let _ = Command::new("tmux")
        .args(["select-pane", "-t", pane_id])
        .output();
}


/// Set pane title by pane ID.
pub fn set_pane_title(pane_id: &str, title: &str) {
    let _ = Command::new("tmux")
        .args(["select-pane", "-t", pane_id, "-T", title])
        .output();
}

/// Set a window-level option.
pub fn set_window_option(window_id: &str, option: &str, value: &str) {
    let _ = Command::new("tmux")
        .args(["set-option", "-w", "-t", window_id, option, value])
        .output();
}

/// Unset a window-level option (revert to default).
pub fn unset_window_option(window_id: &str, option: &str) {
    let _ = Command::new("tmux")
        .args(["set-option", "-wu", "-t", window_id, option])
        .output();
}

/// Swap content of two panes (instant, no layout change).
/// Swaps source_session:source_window with target pane ID.
/// Returns Ok(()) on success, Err(error_message) on failure.
pub fn swap_pane(source_session: &str, source_window: &str, target_pane_id: &str) -> Result<(), String> {
    let src = format!("={source_session}:{source_window}");
    let output = Command::new("tmux")
        .args(["swap-pane", "-d", "-s", &src, "-t", target_pane_id])
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(format!("exec: {e}")),
    }
}

/// Show a message in tmux status line.
pub fn display_message(msg: &str) {
    let _ = Command::new("tmux")
        .args(["display-message", msg])
        .output();
}

/// Show a tmux popup running a command. Non-blocking (returns immediately).
/// -E closes popup when command exits.
pub fn display_popup(width: &str, height: &str, cmd: &str) {
    let _ = Command::new("tmux")
        .args(["display-popup", "-E", "-w", width, "-h", height, cmd])
        .output();
}

pub struct PopupOptions<'a> {
    pub width: &'a str,
    pub height: &'a str,
    pub title: Option<&'a str>,
    pub border_style: Option<&'a str>,
    pub style: Option<&'a str>,
    pub border_lines: Option<&'a str>,
}

pub fn display_popup_styled(opts: &PopupOptions, cmd: &str) {
    let mut args = vec!["display-popup", "-E", "-w", opts.width, "-h", opts.height];
    if let Some(t) = opts.title {
        args.push("-T");
        args.push(t);
    }
    if let Some(s) = opts.border_style {
        args.push("-S");
        args.push(s);
    }
    if let Some(s) = opts.style {
        args.push("-s");
        args.push(s);
    }
    if let Some(b) = opts.border_lines {
        args.push("-b");
        args.push(b);
    }
    args.push(cmd);
    let _ = Command::new("tmux").args(&args).output();
}

/// Ensure a session exists (create if not). No init window cleanup.
pub fn ensure_session(session: &str) {
    if !session_exists(session) {
        let _ = Command::new("tmux")
            .args(["new-session", "-d", "-s", session])
            .output();
    }
}

pub fn send_keys(target: &str, keys: &str) {
    let _ = Command::new("tmux")
        .args(["send-keys", "-t", target, keys])
        .output();
}

pub fn new_window_in_dir(session: &str, name: &str, cwd: &str, shell_cmd: &str) {
    let _ = Command::new("tmux")
        .args([
            "new-window", "-d",
            "-t", &format!("={session}"),
            "-c", cwd,
            "-n", name,
            "zsh", "-c", shell_cmd,
        ])
        .output();
}

pub fn new_session_in_dir(session: &str, name: &str, cwd: &str, shell_cmd: &str) {
    let _ = Command::new("tmux")
        .args([
            "new-session", "-d",
            "-s", session,
            "-c", cwd,
            "-n", name,
            "zsh", "-c", shell_cmd,
        ])
        .output();
}

pub fn attach_session(target: &str) {
    let _ = Command::new("tmux")
        .args(["attach-session", "-t", target])
        .status();
}

pub fn attach(session: &str, window: Option<&str>) -> Result<()> {
    if let Some(win) = window {
        let _ = Command::new("tmux")
            .args(["select-window", "-t", &format!("={session}:{win}")])
            .output();
    }

    // Save original status-right, set hint for detaching
    let original_status = Command::new("tmux")
        .args(["show-option", "-t", &format!("={session}"), "-gv", "status-right"])
        .output()
        .ok()
        .and_then(|o| if o.status.success() { String::from_utf8(o.stdout).ok() } else { None })
        .unwrap_or_default()
        .trim()
        .to_string();

    let _ = Command::new("tmux")
        .args([
            "set-option", "-t", &format!("={session}"),
            "status-right",
            " #[fg=yellow,bold] Ctrl+b d #[default]to return to tncli ",
        ])
        .output();

    let in_tmux = std::env::var("TMUX").is_ok();
    let status = if in_tmux {
        Command::new("tmux")
            .args(["switch-client", "-t", &format!("={session}")])
            .status()?
    } else {
        Command::new("tmux")
            .args(["attach-session", "-t", &format!("={session}")])
            .status()?
    };

    // Restore original status-right
    let _ = Command::new("tmux")
        .args([
            "set-option", "-t", &format!("={session}"),
            "status-right",
            &original_status,
        ])
        .output();

    if !status.success() {
        anyhow::bail!("tmux attach failed");
    }
    Ok(())
}
