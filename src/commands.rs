use anyhow::{bail, Result};
use std::path::Path;

use crate::config::Config;
use crate::{lock, tmux};

// ANSI color helpers
// const RED: unused, errors go through anyhow
const GREEN: &str = "\x1b[0;32m";
const YELLOW: &str = "\x1b[0;33m";
const BLUE: &str = "\x1b[0;34m";
const CYAN: &str = "\x1b[0;36m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const NC: &str = "\x1b[0m";

pub fn cmd_start(config: &Config, config_path: &Path, target: &str) -> Result<()> {
    let services = config.resolve_services(target)?;
    let config_dir = config_path
        .parent()
        .unwrap_or_else(|| Path::new("."));

    lock::ensure_dir();

    let created_session = tmux::create_session_if_needed(&config.session);
    let mut started = 0u32;
    let mut skipped = 0u32;

    for svc in &services {
        if tmux::window_exists(&config.session, svc) {
            eprintln!("{YELLOW}warning:{NC} '{svc}' is already running — skipping");
            skipped += 1;
            continue;
        }

        let service = config
            .services
            .get(svc)
            .unwrap_or_else(|| panic!("service '{}' not found in config", svc));

        let cmd = service
            .cmd
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("service '{}' has no 'cmd' defined", svc))?;

        let work_dir = match &service.dir {
            Some(dir) => {
                let p = Path::new(dir);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    config_dir.join(dir)
                }
            }
            None => config_dir.to_path_buf(),
        };

        if !work_dir.is_dir() {
            bail!(
                "directory '{}' for service '{}' does not exist",
                work_dir.display(),
                svc
            );
        }

        let mut full_cmd = format!("cd '{}'", work_dir.display());
        if let Some(pre) = &service.pre_start {
            full_cmd.push_str(&format!(" && {pre}"));
        }
        full_cmd.push_str(&format!(" && {cmd}"));
        if let Some(env_vars) = &service.env {
            full_cmd = format!("{env_vars} {full_cmd}");
        }

        tmux::new_window(&config.session, svc, &full_cmd);
        lock::acquire(&config.session, svc);
        println!("{GREEN}>>>{NC} started {BOLD}{svc}{NC}");
        started += 1;
    }

    if created_session {
        tmux::cleanup_init_window(&config.session);
    }

    if started > 0 {
        println!(
            "\n{GREEN}{started} service(s) started{NC} in session {CYAN}{}{NC}",
            config.session
        );
        println!("{DIM}attach: tncli attach{NC}");
    }
    if skipped > 0 {
        println!("{YELLOW}{skipped} service(s) skipped (already running){NC}");
    }

    Ok(())
}

pub fn cmd_stop(config: &Config, target: Option<&str>) -> Result<()> {
    lock::ensure_dir();

    if target.is_none() {
        if tmux::session_exists(&config.session) {
            tmux::kill_session(&config.session);
            lock::release_all(&config.session);
            println!(
                "{GREEN}>>>{NC} stopped all services (session {CYAN}{}{NC} killed)",
                config.session
            );
        } else {
            println!(
                "{BLUE}>>>{NC} no running session '{}'",
                config.session
            );
        }
        return Ok(());
    }

    let target = target.unwrap();
    let services = config.resolve_services(target)?;
    let mut stopped = 0u32;

    for svc in &services {
        if tmux::window_exists(&config.session, svc) {
            tmux::kill_window(&config.session, svc);
            lock::release(&config.session, svc);
            println!("{GREEN}>>>{NC} stopped {BOLD}{svc}{NC}");
            stopped += 1;
        } else {
            eprintln!("{YELLOW}warning:{NC} '{svc}' is not running");
        }
    }

    if !tmux::session_exists(&config.session) {
        lock::release_all(&config.session);
    } else {
        let remaining = tmux::list_windows(&config.session);
        if remaining.is_empty() {
            tmux::kill_session(&config.session);
            lock::release_all(&config.session);
        }
    }

    println!("{GREEN}{stopped} service(s) stopped{NC}");
    Ok(())
}

pub fn cmd_restart(config: &Config, config_path: &Path, target: &str) -> Result<()> {
    cmd_stop(config, Some(target))?;
    cmd_start(config, config_path, target)
}

pub fn cmd_status(config: &Config) -> Result<()> {
    if !tmux::session_exists(&config.session) {
        println!("{DIM}no active session '{}'{NC}", config.session);
        return Ok(());
    }

    println!("{BOLD}Session:{NC} {CYAN}{}{NC}\n", config.session);

    let windows = tmux::list_windows(&config.session);

    println!("{BOLD}{:<20} {:<10}{NC}", "SERVICE", "STATUS");
    println!("{:<20} {:<10}", "-------", "------");

    for svc in config.services.keys() {
        if windows.contains(svc) {
            println!("{:<20} {GREEN}{:<10}{NC}", svc, "running");
        } else {
            println!("{:<20} {DIM}{:<10}{NC}", svc, "stopped");
        }
    }

    println!("\n{DIM}attach: tncli attach{NC}");
    Ok(())
}

pub fn cmd_attach(config: &Config, target: Option<&str>) -> Result<()> {
    if !tmux::session_exists(&config.session) {
        bail!("no active session '{}'", config.session);
    }
    tmux::attach(&config.session, target)
}

pub fn cmd_logs(config: &Config, target: &str) -> Result<()> {
    if !tmux::window_exists(&config.session, target) {
        bail!("service '{}' is not running", target);
    }
    let lines = tmux::capture_pane(&config.session, target, 100);
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

pub fn cmd_list(config: &Config) -> Result<()> {
    println!("{BOLD}Services:{NC}");
    for (name, svc) in &config.services {
        let cmd = svc.cmd.as_deref().unwrap_or("n/a");
        println!("  {name}: {cmd}");
    }

    println!("\n{BOLD}Combinations:{NC}");
    for (name, svcs) in &config.combinations {
        println!("  {name}: {}", svcs.join(", "));
    }

    Ok(())
}
