use anyhow::{bail, Result};
use std::fmt::Write;
use std::path::Path;

use crate::config::Config;
use crate::{lock, tmux};

const GREEN: &str = "\x1b[0;32m";
const YELLOW: &str = "\x1b[0;33m";
const BLUE: &str = "\x1b[0;34m";
const CYAN: &str = "\x1b[0;36m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const NC: &str = "\x1b[0m";

pub fn cmd_start(config: &Config, config_path: &Path, target: &str) -> Result<()> {
    let pairs = config.resolve_services(target)?;
    let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

    lock::ensure_dir();

    let created_session = tmux::create_session_if_needed(&config.session);
    let mut started = 0u32;
    let mut skipped = 0u32;

    for (dir_name, svc_name) in &pairs {
        if tmux::window_exists(&config.session, svc_name) {
            eprintln!("{YELLOW}warning:{NC} '{svc_name}' is already running — skipping");
            skipped += 1;
            continue;
        }

        let resolved = config.resolve_service(config_dir, dir_name, svc_name)?;

        if !resolved.work_dir.is_dir() {
            bail!("directory '{}' for service '{}/{}' does not exist", resolved.work_dir.display(), dir_name, svc_name);
        }

        let mut full_cmd = String::new();
        if let Some(env_vars) = &resolved.env {
            write!(full_cmd, "{env_vars} ").unwrap();
        }
        write!(full_cmd, "cd '{}'", resolved.work_dir.display()).unwrap();
        if let Some(pre) = &resolved.pre_start {
            write!(full_cmd, " && {pre}").unwrap();
        }
        write!(full_cmd, " && {}", resolved.cmd).unwrap();

        tmux::new_window(&config.session, svc_name, &full_cmd);
        lock::acquire(&config.session, svc_name);
        println!("{GREEN}>>>{NC} started {BOLD}{svc_name}{NC} ({DIM}{dir_name}{NC})");
        started += 1;
    }

    if created_session {
        tmux::cleanup_init_window(&config.session);
    }

    if started > 0 {
        println!("\n{GREEN}{started} service(s) started{NC} in session {CYAN}{}{NC}", config.session);
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
            println!("{GREEN}>>>{NC} stopped all services (session {CYAN}{}{NC} killed)", config.session);
        } else {
            println!("{BLUE}>>>{NC} no running session '{}'", config.session);
        }
        return Ok(());
    }

    let target = target.unwrap();
    let pairs = config.resolve_services(target)?;
    let mut stopped = 0u32;

    for (_, svc_name) in &pairs {
        if tmux::window_exists(&config.session, svc_name) {
            tmux::graceful_stop(&config.session, svc_name);
            lock::release(&config.session, svc_name);
            println!("{GREEN}>>>{NC} stopped {BOLD}{svc_name}{NC}");
            stopped += 1;
        } else {
            eprintln!("{YELLOW}warning:{NC} '{svc_name}' is not running");
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

    for (dir_name, dir) in &config.repos {
        println!("{BOLD}{dir_name}{NC}");
        for svc_name in dir.services.keys() {
            if windows.contains(svc_name) {
                println!("  {GREEN}●{NC} {svc_name}");
            } else {
                println!("  {DIM}○ {svc_name}{NC}");
            }
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
    for (dir_name, dir) in &config.repos {
        let alias = dir.alias.as_deref().map(|a| format!(" ({a})")).unwrap_or_default();
        println!("  {BOLD}{dir_name}{alias}{NC}");
        for (svc_name, svc) in &dir.services {
            let cmd = svc.cmd.as_deref().unwrap_or("n/a");
            println!("    {svc_name}: {cmd}");
        }
    }

    let workspaces = config.all_workspaces();
    if !workspaces.is_empty() {
        println!("\n{BOLD}Workspaces:{NC}");
        for (name, entries) in &workspaces {
            println!("  {name}: {}", entries.join(", "));
        }
    }

    Ok(())
}

pub fn cmd_workspace_create(config: &Config, config_path: &Path, workspace: &str, branch: &str, from_stage: Option<usize>) -> Result<()> {
    use crate::pipeline::{self, PipelineEvent};
    use std::collections::HashSet;

    // Build skip set from --from-stage (1-based)
    let skip_stages: HashSet<usize> = match from_stage {
        Some(n) if n > 1 => (0..n - 1).collect(),
        _ => HashSet::new(),
    };

    let ctx = pipeline::context::CreateContext::from_config(config, config_path, workspace, branch, skip_stages)?;
    let bind_ip = ctx.bind_ip.clone();

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        pipeline::run_create_pipeline(ctx, tx);
    });

    // Print progress synchronously
    while let Ok(evt) = rx.recv() {
        match evt {
            PipelineEvent::StageStarted { index, name, total } => {
                println!("{BLUE}>>>{NC} [{}/{}] {name}", index + 1, total);
            }
            PipelineEvent::StageCompleted { .. } => {
                println!("    {GREEN}done{NC}");
            }
            PipelineEvent::StageSkipped { index } => {
                let label = pipeline::stages::CreateStage::all().get(index)
                    .map(|s| s.label()).unwrap_or("?");
                println!("{DIM}    skipped: {label}{NC}");
            }
            PipelineEvent::PipelineCompleted => {
                let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
                println!("\n{GREEN}Workspace ready:{NC} BIND_IP={bind_ip}");
                println!("  cd {}/workspace--{branch}", config_dir.display());
                break;
            }
            PipelineEvent::PipelineFailed { stage, error } => {
                eprintln!("\n{YELLOW}Failed at stage {}:{NC} {error}", stage + 1);
                eprintln!("{DIM}Retry: tncli workspace create {workspace} {branch} --from-stage {}{NC}", stage + 1);
                bail!("workspace creation failed at stage {}", stage + 1);
            }
        }
    }

    Ok(())
}

pub fn cmd_workspace_delete(config: &Config, config_path: &Path, branch: &str) -> Result<()> {
    use crate::pipeline::{self, PipelineEvent};
    use crate::pipeline::context::{DeleteContext, CleanupItem, DbDropItem};
    use std::collections::HashSet;

    let config_dir = config_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    // Collect worktree info for cleanup
    let mut cleanup_items = Vec::new();
    let mut dbs_to_drop = Vec::new();
    let branch_safe = crate::services::branch_safe(branch);

    for (dir_name, dir) in &config.repos {
        let dir_path = if std::path::Path::new(dir_name).is_absolute() {
            dir_name.to_string()
        } else {
            config_dir.join(dir_name).to_string_lossy().into_owned()
        };

        let ws_folder = config_dir.join(format!("workspace--{branch}"));
        let wt_path = ws_folder.join(dir_name);
        if !wt_path.exists() { continue; }

        let pre_delete = dir.wt()
            .map(|wt| wt.pre_delete.clone())
            .unwrap_or_default();

        cleanup_items.push(CleanupItem {
            dir_path,
            wt_path,
            wt_branch: branch.to_string(),
            pre_delete,
        });

        // Collect DBs to drop
        if let Some(wt_cfg) = dir.wt() {
            for sref in &wt_cfg.shared_services {
                if let Some(db_tpl) = &sref.db_name {
                    let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                        .replace("{{branch}}", branch);
                    let svc_def = config.shared_services.get(&sref.name);
                    dbs_to_drop.push(DbDropItem {
                        host: svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost").to_string(),
                        port: svc_def.and_then(|d| d.ports.first())
                            .and_then(|p| p.split(':').next())
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(5432),
                        db_name,
                        user: svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres").to_string(),
                        password: svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres").to_string(),
                    });
                }
            }
        }
    }

    let ctx = DeleteContext {
        branch: branch.to_string(),
        config: config.clone(),
        config_dir,
        session: config.session.clone(),
        wt_keys: Vec::new(),
        cleanup_items,
        dbs_to_drop,
        network: format!("tncli-ws-{branch}"),
        skip_stages: HashSet::new(),
    };

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        pipeline::run_delete_pipeline(ctx, tx);
    });

    while let Ok(evt) = rx.recv() {
        match evt {
            PipelineEvent::StageStarted { index, name, total } => {
                println!("{BLUE}>>>{NC} [{}/{}] {name}", index + 1, total);
            }
            PipelineEvent::StageCompleted { .. } => {
                println!("    {GREEN}done{NC}");
            }
            PipelineEvent::PipelineCompleted => {
                println!("\n{GREEN}Workspace '{branch}' deleted{NC}");
                break;
            }
            PipelineEvent::PipelineFailed { stage, error } => {
                eprintln!("\n{YELLOW}Delete failed at stage {}:{NC} {error}", stage + 1);
                bail!("workspace deletion failed");
            }
            _ => {}
        }
    }

    Ok(())
}

pub fn cmd_workspace_list(config: &Config, config_path: &Path) -> Result<()> {
    let workspaces = config.all_workspaces();
    let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
    let ip_allocs = crate::services::load_ip_allocations();

    // Collect active workspace instances (branch → IP)
    let mut ws_branches: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(config_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(branch) = name.strip_prefix("workspace--") {
                ws_branches.push(branch.to_string());
            }
        }
    }
    ws_branches.sort();

    // Show available workspace definitions
    println!("{BOLD}Workspace definitions:{NC}");
    for (name, entries) in &workspaces {
        println!("  {BOLD}{name}{NC}: {}", entries.join(", "));
    }

    if ws_branches.is_empty() {
        println!("\n{DIM}No active workspace instances{NC}");
        return Ok(());
    }

    // Show each active instance with services + ports
    for branch in &ws_branches {
        let ws_key = format!("ws-{branch}");
        let ip = ip_allocs.get(&ws_key)
            .map(|s| s.as_str())
            .unwrap_or("?");
        let branch_safe = crate::services::branch_safe(branch);

        println!("\n{GREEN}Workspace: {BOLD}{branch}{NC} {DIM}({ip}){NC}");

        // Find which combo this workspace belongs to (by checking dirs)
        let ws_folder = config_dir.join(format!("workspace--{branch}"));

        for (dir_name, dir) in &config.repos {
            let wt_dir = ws_folder.join(dir_name);
            if !wt_dir.exists() {
                continue;
            }

            let alias = dir.alias.as_deref().map(|a| format!(" ({a})")).unwrap_or_default();
            println!("  {BOLD}{dir_name}{alias}{NC}");

            // Services with ports
            for (svc_name, svc) in &dir.services {
                let cmd = svc.cmd.as_deref().unwrap_or("n/a");
                // Extract port from cmd if possible (--port N, -p N, :PORT)
                let port = extract_port_from_cmd(cmd);
                if let Some(p) = port {
                    println!("    {CYAN}{svc_name}{NC} → {ip}:{p}  {DIM}{cmd}{NC}");
                } else {
                    println!("    {CYAN}{svc_name}{NC}  {DIM}{cmd}{NC}");
                }
            }

            // Show DB info from shared_services
            let shared_svcs = dir.wt().map(|wt| &wt.shared_services);
            for sref in shared_svcs.into_iter().flatten() {
                if let Some(db_template) = &sref.db_name {
                    let db_name = db_template.replace("{{branch_safe}}", &branch_safe)
                        .replace("{{branch}}", branch);
                    let svc_def = config.shared_services.get(&sref.name);
                    let host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost");
                    let port = svc_def.and_then(|d| d.ports.first())
                        .and_then(|p| p.split(':').next())
                        .unwrap_or("5432");
                    println!("    {DIM}db: {db_name} @ {host}:{port}{NC}");
                }
            }

            // Show worktree env URLs
            let wt_env = dir.wt().map(|wt| &wt.env);
            for (key, val) in wt_env.into_iter().flatten() {
                if key.contains("URL") || key.contains("ORIGIN") {
                    let resolved = val.replace("{{bind_ip}}", ip)
                        .replace("{{branch_safe}}", &branch_safe)
                        .replace("{{branch}}", branch);
                    println!("    {DIM}{key}={resolved}{NC}");
                }
            }
        }
    }

    // Shared services summary
    if !config.shared_services.is_empty() {
        println!("\n{BOLD}Shared services:{NC}");
        for (name, svc) in &config.shared_services {
            let ports_str = svc.ports.join(", ");
            let host = svc.host.as_deref().unwrap_or("localhost");
            println!("  {CYAN}{name}{NC}: {host} [{ports_str}] {DIM}({}){NC}", svc.image);
        }
    }

    Ok(())
}

/// Extract port number from a service command string.
fn extract_port_from_cmd(cmd: &str) -> Option<u16> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    // Check pairs of consecutive tokens for --port N / -p N
    for pair in parts.windows(2) {
        if pair[0] == "--port" || pair[0] == "-p" {
            if let Ok(p) = pair[1].parse::<u16>() {
                return Some(p);
            }
        }
    }
    // Check for --port=N form
    for part in &parts {
        if let Some(val) = part.strip_prefix("--port=") {
            if let Ok(p) = val.parse::<u16>() {
                return Some(p);
            }
        }
    }
    None
}

pub fn cmd_setup(config: &Config) -> Result<()> {
    // 1. Setup loopback IPs: 127.0.0.2 - 127.0.0.100
    println!("{BOLD}Setting up loopback IPs (127.0.0.2 - 127.0.0.100)...{NC}");
    let mut ips = Vec::new();
    for n in 2..=100u8 {
        ips.push(format!("127.0.0.{n}"));
    }

    // Build a single script to add all aliases
    let script = ips.iter()
        .map(|ip| format!("ifconfig lo0 alias {ip} 2>/dev/null"))
        .collect::<Vec<_>>()
        .join("; ");

    let status = std::process::Command::new("sudo")
        .args(["sh", "-c", &script])
        .status()?;

    if status.success() {
        println!("{GREEN}>>>{NC} {GREEN}99 loopback IPs configured{NC} (127.0.0.2 - 127.0.0.100)");
    } else {
        eprintln!("{YELLOW}warning:{NC} failed to setup loopback IPs (sudo required)");
    }

    // 2. Setup /etc/hosts for shared services
    let mut hostnames: Vec<String> = Vec::new();
    for svc in config.shared_services.values() {
        if let Some(host) = &svc.host {
            if !hostnames.contains(host) {
                hostnames.push(host.clone());
            }
        }
    }

    if !hostnames.is_empty() {
        let hosts_content = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
        let missing: Vec<&String> = hostnames.iter()
            .filter(|h| !hosts_content.contains(h.as_str()))
            .collect();

        if missing.is_empty() {
            println!("{GREEN}>>>{NC} /etc/hosts already configured");
        } else {
            println!("{BOLD}Adding to /etc/hosts:{NC}");
            for h in &missing {
                println!("  127.0.0.1 {h}");
            }
            let entries: Vec<String> = missing.iter()
                .map(|h| format!("127.0.0.1 {h}"))
                .collect();
            let cmd = format!("echo '\n# tncli shared services\n{}' >> /etc/hosts", entries.join("\n"));
            let status = std::process::Command::new("sudo")
                .args(["sh", "-c", &cmd])
                .status()?;
            if status.success() {
                println!("{GREEN}>>>{NC} {GREEN}/etc/hosts updated{NC}");
            } else {
                eprintln!("{YELLOW}warning:{NC} failed to update /etc/hosts");
            }
        }
    }

    // 3. Setup global gitignore
    crate::services::ensure_global_gitignore();
    println!("{GREEN}>>>{NC} global gitignore configured");

    println!("\n{GREEN}Setup complete!{NC}");
    Ok(())
}

pub fn cmd_update() -> Result<()> {
    println!("{BOLD}Checking for updates...{NC}");

    let output = std::process::Command::new("curl")
        .args(["-sL", "https://api.github.com/repos/toantran292/tncli/releases/latest"])
        .output()?;

    let body = String::from_utf8_lossy(&output.stdout);
    let latest = body
        .lines()
        .find(|l| l.contains("\"tag_name\""))
        .and_then(|l| l.split('"').nth(3))
        .unwrap_or("")
        .trim_start_matches('v');

    let current = crate::VERSION;

    if latest.is_empty() {
        bail!("could not fetch latest version");
    }

    if latest == current {
        println!("{GREEN}Already up to date: v{current}{NC}");
        return Ok(());
    }

    println!("Current: v{current} → Latest: v{latest}");
    println!("{BLUE}>>>{NC} Downloading update...");

    let status = std::process::Command::new("bash")
        .args(["-c", "curl -fsSL https://raw.githubusercontent.com/toantran292/tncli/main/install.sh | bash"])
        .status()?;

    if !status.success() {
        bail!("update failed");
    }

    Ok(())
}
