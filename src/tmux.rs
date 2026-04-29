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


/// Get cursor position in a tmux pane. Returns (x, y) relative to pane.
pub fn cursor_position(session: &str, window: &str) -> Option<(u16, u16)> {
    let target = format!("={session}:{window}");
    let output = Command::new("tmux")
        .args(["display-message", "-t", &target, "-p", "#{cursor_x},#{cursor_y}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = s.trim().split(',').collect();
    if parts.len() == 2 {
        let x: u16 = parts[0].parse().ok()?;
        let y: u16 = parts[1].parse().ok()?;
        Some((x, y))
    } else {
        None
    }
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

/// Send keys to a tmux pane.
pub fn send_keys(session: &str, window: &str, keys: &[&str]) {
    let target = format!("={session}:{window}");
    let mut args = vec!["send-keys", "-t", &target];
    args.extend(keys);
    let _ = Command::new("tmux").args(&args).output();
}

/// Create a temporary tmux session, attach, kill session on return.
fn run_temp_session(shell_cmd: &str) -> Result<()> {
    let tmp = "_tncli_temp";
    kill_session(tmp); // clean up leftover

    let _ = Command::new("tmux")
        .args(["new-session", "-d", "-s", tmp, "zsh", "-ic", shell_cmd])
        .output();

    let _ = Command::new("tmux")
        .args([
            "set-option", "-t", &format!("={tmp}"),
            "status-right",
            " #[fg=yellow,bold] Ctrl+b d #[default]to return to tncli ",
        ])
        .output();

    let in_tmux = std::env::var("TMUX").is_ok();
    let _status = if in_tmux {
        Command::new("tmux")
            .args(["switch-client", "-t", &format!("={tmp}")])
            .status()
    } else {
        Command::new("tmux")
            .args(["attach-session", "-t", &format!("={tmp}")])
            .status()
    };

    kill_session(tmp);
    Ok(())
}

/// Run a command in a temporary session, show output, auto-close on keypress.
pub fn run_in_window(_session: &str, dir: &str, cmd: &str, desc: &str) -> Result<()> {
    let full_cmd = format!(
        "cd '{}' && echo '\\033[1;33m[tncli]\\033[0m running: {}' && echo '' && {} ; echo '' && echo '\\033[1;32m[tncli]\\033[0m finished. press any key to close.' && read -n 1 && tmux detach-client",
        dir, desc, cmd
    );
    run_temp_session(&full_cmd)
}

/// Open a shell at a directory in a temporary session, kill on return.
pub fn open_shell(_session: &str, dir: &str) -> Result<()> {
    run_temp_session(&format!("cd '{}' && exec zsh", dir))
}

pub fn resize_window(session: &str, window: &str, width: u16, height: u16) {
    let _ = Command::new("tmux")
        .args([
            "resize-window",
            "-t",
            &format!("={session}:{window}"),
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
        ])
        .output();
}

pub fn resize_all_windows(session: &str, width: u16, height: u16) {
    for win in list_windows(session) {
        resize_window(session, &win, width, height);
    }
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
