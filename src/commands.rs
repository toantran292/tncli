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

    let created_session = tmux::create_session_if_needed(&config.svc_session());
    let mut started = 0u32;
    let mut skipped = 0u32;

    for (dir_name, svc_name) in &pairs {
        if tmux::window_exists(&config.svc_session(), svc_name) {
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

        tmux::new_window(&config.svc_session(), svc_name, &full_cmd);
        lock::acquire(&config.svc_session(), svc_name);
        println!("{GREEN}>>>{NC} started {BOLD}{svc_name}{NC} ({DIM}{dir_name}{NC})");
        started += 1;
    }

    if created_session {
        tmux::cleanup_init_window(&config.svc_session());
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
        if tmux::session_exists(&config.svc_session()) {
            tmux::kill_session(&config.svc_session());
            lock::release_all(&config.svc_session());
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
        if tmux::window_exists(&config.svc_session(), svc_name) {
            tmux::graceful_stop(&config.svc_session(), svc_name);
            lock::release(&config.svc_session(), svc_name);
            println!("{GREEN}>>>{NC} stopped {BOLD}{svc_name}{NC}");
            stopped += 1;
        } else {
            eprintln!("{YELLOW}warning:{NC} '{svc_name}' is not running");
        }
    }

    if !tmux::session_exists(&config.svc_session()) {
        lock::release_all(&config.svc_session());
    } else {
        let remaining = tmux::list_windows(&config.svc_session());
        if remaining.is_empty() {
            tmux::kill_session(&config.svc_session());
            lock::release_all(&config.svc_session());
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
    if !tmux::session_exists(&config.svc_session()) {
        println!("{DIM}no active session '{}'{NC}", config.session);
        return Ok(());
    }

    println!("{BOLD}Session:{NC} {CYAN}{}{NC}\n", config.session);

    let windows = tmux::list_windows(&config.svc_session());

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
    if !tmux::session_exists(&config.svc_session()) {
        bail!("no active session '{}'", config.session);
    }
    tmux::attach(&config.svc_session(), target)
}

pub fn cmd_logs(config: &Config, target: &str) -> Result<()> {
    if !tmux::window_exists(&config.svc_session(), target) {
        bail!("service '{}' is not running", target);
    }
    let lines = tmux::capture_pane(&config.svc_session(), target, 100);
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

pub fn cmd_workspace_create(config: &Config, config_path: &Path, workspace: &str, branch: &str, from_stage: Option<usize>, repos: Option<&str>) -> Result<()> {
    use crate::pipeline::{self, PipelineEvent};
    use std::collections::HashSet;

    crate::services::migrate_legacy_ips();

    // Build skip set from --from-stage (1-based)
    let skip_stages: HashSet<usize> = match from_stage {
        Some(n) if n > 1 => (0..n - 1).collect(),
        _ => HashSet::new(),
    };

    // Parse --repos "repo1:branch1,repo2:branch2"
    let selected_dirs: Option<Vec<(String, String)>> = repos.map(|r| {
        r.split(',').filter_map(|entry| {
            let parts: Vec<&str> = entry.splitn(2, ':').collect();
            if parts.len() == 2 {
                Some((parts[0].to_string(), parts[1].to_string()))
            } else {
                Some((parts[0].to_string(), branch.to_string()))
            }
        }).collect()
    });

    let ctx = if let Some(selected) = selected_dirs {
        pipeline::context::CreateContext::from_config_with_selection(config, config_path, workspace, branch, selected)?
    } else {
        pipeline::context::CreateContext::from_config(config, config_path, workspace, branch, skip_stages)?
    };
    let bind_ip = ctx.bind_ip.clone();

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        pipeline::run_create_pipeline(ctx, tx);
    });

    // Print progress synchronously
    while let Ok(evt) = rx.recv() {
        match evt {
            PipelineEvent::StageStarted { index, name, total, .. } => {
                println!("{BLUE}>>>{NC} [{}/{}] {name}", index + 1, total);
            }
            PipelineEvent::StageCompleted { .. } => {
                println!("    {GREEN}done{NC}");
            }
            PipelineEvent::StageSkipped { index, .. } => {
                let label = pipeline::stages::CreateStage::all().get(index)
                    .map(|s| s.label()).unwrap_or("?");
                println!("{DIM}    skipped: {label}{NC}");
            }
            PipelineEvent::PipelineCompleted { .. } => {
                let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
                println!("\n{GREEN}Workspace ready:{NC} BIND_IP={bind_ip}");
                println!("  cd {}/workspace--{branch}", config_dir.display());
                break;
            }
            PipelineEvent::PipelineFailed { stage, error, .. } => {
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
            // Resolve through main workspace folder
            let default_branch = config.global_default_branch();
            let ws_path = config_dir.join(format!("workspace--{default_branch}")).join(dir_name);
            if ws_path.exists() {
                ws_path.to_string_lossy().into_owned()
            } else {
                config_dir.join(dir_name).to_string_lossy().into_owned()
            }
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
            let pg_svc = config.shared_services.values().find(|s| s.db_user.is_some());
            let pg_host = config.shared_host("postgres");
            let pg_port = pg_svc.and_then(|s| s.ports.first())
                .and_then(|p| p.split(':').next()).and_then(|p| p.parse().ok()).unwrap_or(5432u16);
            let pg_user = pg_svc.and_then(|s| s.db_user.as_deref()).unwrap_or("postgres");
            let pg_pw = pg_svc.and_then(|s| s.db_password.as_deref()).unwrap_or("postgres");

            // Legacy: shared_services with db_name
            for sref in &wt_cfg.shared_services {
                if let Some(db_tpl) = &sref.db_name {
                    let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                        .replace("{{branch}}", branch);
                    dbs_to_drop.push(DbDropItem {
                        host: pg_host.clone(), port: pg_port,
                        db_name, user: pg_user.to_string(), password: pg_pw.to_string(),
                    });
                }
            }
            // New: databases field (auto-prefixed with {session}_)
            for db_tpl in &wt_cfg.databases {
                let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                    .replace("{{branch}}", branch);
                dbs_to_drop.push(DbDropItem {
                    host: pg_host.clone(), port: pg_port,
                    db_name: format!("{}_{db_name}", config.session),
                    user: pg_user.to_string(), password: pg_pw.to_string(),
                });
            }
        }
    }

    let ctx = DeleteContext {
        branch: branch.to_string(),
        config: config.clone(),
        config_dir,
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
            PipelineEvent::StageStarted { index, name, total, .. } => {
                println!("{BLUE}>>>{NC} [{}/{}] {name}", index + 1, total);
            }
            PipelineEvent::StageCompleted { .. } => {
                println!("    {GREEN}done{NC}");
            }
            PipelineEvent::PipelineCompleted { .. } => {
                println!("\n{GREEN}Workspace '{branch}' deleted{NC}");
                break;
            }
            PipelineEvent::PipelineFailed { stage, error, .. } => {
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
    // 1. Setup loopback IPs: 127.0.{1..N}.{2..M} (one subnet per session)
    let subnet_count = crate::services::SETUP_SUBNET_COUNT;
    let host_max = crate::services::SETUP_HOST_MAX;

    // Build IP list (needed for both alias check and LaunchDaemon script)
    let mut ips = Vec::new();
    for subnet in 1..=subnet_count {
        for host in 2..=host_max {
            ips.push(format!("127.0.{subnet}.{host}"));
        }
    }
    let hosts_per_subnet = host_max - 1;
    let total = ips.len();

    // Check if aliases already exist by testing a sample IP
    let already_setup = std::process::Command::new("ping")
        .args(["-c", "1", "-W", "1", "127.0.1.2"])
        .output()
        .is_ok_and(|o| o.status.success());

    if already_setup {
        println!("{GREEN}>>>{NC} loopback IPs already configured ({total} IPs, {subnet_count} subnets × {hosts_per_subnet} hosts)");
    } else {
        println!("{BOLD}Setting up loopback IPs (127.0.{{1..{subnet_count}}}.{{2..{host_max}}})...{NC}");

        let script = ips.iter()
            .map(|ip| format!("ifconfig lo0 alias {ip} 2>/dev/null"))
            .collect::<Vec<_>>()
            .join("; ");

        let status = std::process::Command::new("sudo")
            .args(["sh", "-c", &script])
            .status()?;

        if status.success() {
            println!("{GREEN}>>>{NC} {GREEN}{total} loopback IPs configured{NC} ({subnet_count} subnets × {hosts_per_subnet} hosts)");
        } else {
            eprintln!("{YELLOW}warning:{NC} failed to setup loopback IPs (sudo required)");
        }

        // Flush DNS cache (loopback aliases disturb network stack)
        let _ = std::process::Command::new("sudo")
            .args(["dscacheutil", "-flushcache"])
            .status();
        let _ = std::process::Command::new("sudo")
            .args(["killall", "-HUP", "mDNSResponder"])
            .status();
    }

    // 1b. Install LaunchDaemon so loopback aliases survive reboot
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let script_path = format!("{home}/.tncli/setup-loopback.sh");
    let plist_path = "/Library/LaunchDaemons/com.tncli.loopback.plist";

    // Always update the shell script (may have changed subnet/host count)
    let _ = std::fs::create_dir_all(format!("{home}/.tncli"));
    let script_content = format!(
        "#!/bin/sh\n{}\n",
        ips.iter()
            .map(|ip| format!("ifconfig lo0 alias {ip} 2>/dev/null"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let _ = std::fs::write(&script_path, &script_content);
    let _ = std::process::Command::new("chmod").args(["+x", &script_path]).status();

    if std::path::Path::new(plist_path).exists() {
        println!("{GREEN}>>>{NC} LaunchDaemon already installed");
    } else {
        let plist_content = format!(
r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tncli.loopback</string>
    <key>ProgramArguments</key>
    <array>
        <string>{script_path}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"#);
        let tmp_plist = format!("{home}/.tncli/com.tncli.loopback.plist");
        let _ = std::fs::write(&tmp_plist, &plist_content);
        let install_status = std::process::Command::new("sudo")
            .args(["cp", &tmp_plist, plist_path])
            .status();
        let _ = std::fs::remove_file(&tmp_plist);

        if install_status.is_ok_and(|s| s.success()) {
            let _ = std::process::Command::new("sudo")
                .args(["chown", "root:wheel", plist_path])
                .status();
            println!("{GREEN}>>>{NC} LaunchDaemon installed (loopback aliases persist across reboot)");
        } else {
            eprintln!("{YELLOW}warning:{NC} failed to install LaunchDaemon at {plist_path}");
        }
    }

    // 2. Setup /etc/hosts for shared services (including *.tncli.test for fast resolution + Prisma compat)
    let mut hostnames: Vec<String> = Vec::new();
    for (name, svc) in &config.shared_services {
        let host = svc.host.clone().unwrap_or_else(|| format!("{}.{name}.tncli.test", config.session));
        if !hostnames.contains(&host) {
            hostnames.push(host);
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

    // 4. Install Caddy (reverse proxy)
    let has_caddy = std::process::Command::new("caddy").arg("version")
        .output().is_ok_and(|o| o.status.success());
    if has_caddy {
        println!("{GREEN}>>>{NC} caddy already installed");
    } else {
        println!("{BOLD}Installing caddy...{NC}");
        let status = std::process::Command::new("brew").args(["install", "caddy"]).status();
        if status.is_ok_and(|s| s.success()) {
            println!("{GREEN}>>>{NC} {GREEN}caddy installed{NC}");
        } else {
            eprintln!("{YELLOW}warning:{NC} failed to install caddy — proxy won't work");
        }
    }

    // 5. Setup dnsmasq for *.tncli.test wildcard resolution
    println!("\n{BOLD}[4/4] DNS (*.tncli.test → 127.0.0.1){NC}");
    let dns_status = crate::services::dns::status();
    if dns_status.is_ready() {
        println!("{GREEN}>>>{NC} dnsmasq already configured and running");
        // Retry verification (DNS cache may need a moment after loopback setup)
        let mut resolved = false;
        for _ in 0..3 {
            if crate::services::dns::verify_resolution() {
                resolved = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        if resolved {
            println!("{GREEN}>>>{NC} *.tncli.test resolves correctly");
        } else {
            eprintln!("{YELLOW}warning:{NC} DNS resolution not working — try: sudo brew services restart dnsmasq");
        }
    } else {
        match crate::services::dns::setup_dnsmasq() {
            Ok(actions) => {
                for action in &actions {
                    println!("{GREEN}>>>{NC} {action}");
                }
                // Verify
                // Give DNS a moment to start
                std::thread::sleep(std::time::Duration::from_secs(2));
                if crate::services::dns::verify_resolution() {
                    println!("{GREEN}>>>{NC} *.tncli.test resolves correctly");
                } else {
                    eprintln!("{YELLOW}warning:{NC} DNS resolution not yet working — may need a few seconds");
                }
            }
            Err(e) => {
                eprintln!("{YELLOW}warning:{NC} dnsmasq setup failed: {e}");
                eprintln!("  Manual setup: brew install dnsmasq && see docs");
            }
        }
    }

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

    // Self-update: download binary directly, replace ourselves
    let os = if cfg!(target_os = "macos") { "darwin" } else { "linux" };
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
    let url = format!(
        "https://github.com/toantran292/tncli/releases/download/v{latest}/tncli-{os}-{arch}.tar.gz"
    );

    let tmpdir = std::env::temp_dir().join("tncli-update");
    let _ = std::fs::create_dir_all(&tmpdir);
    let tar_path = tmpdir.join("tncli.tar.gz");

    // Download
    let status = std::process::Command::new("curl")
        .args(["-sL", "-o", &tar_path.to_string_lossy(), &url])
        .status()?;
    if !status.success() {
        bail!("download failed");
    }

    // Extract
    let status = std::process::Command::new("tar")
        .args(["xzf", &tar_path.to_string_lossy(), "-C", &tmpdir.to_string_lossy()])
        .status()?;
    if !status.success() {
        bail!("extract failed");
    }

    let binary = tmpdir.join(format!("tncli-{os}-{arch}"));
    if !binary.exists() {
        bail!("binary not found in archive");
    }

    // Remove quarantine on macOS
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("xattr")
            .args(["-rd", "com.apple.quarantine", &binary.to_string_lossy()])
            .status();
    }

    // Install to ~/.local/bin (no sudo needed)
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let install_dir = format!("{home}/.local/bin");
    let install_path = format!("{install_dir}/tncli");

    // Ensure ~/.local/bin exists
    let _ = std::fs::create_dir_all(&install_dir);

    // Copy binary
    let cp_status = std::process::Command::new("cp")
        .args([&binary.to_string_lossy().to_string(), &install_path])
        .status();

    if !cp_status.map(|s| s.success()).unwrap_or(false) {
        bail!("failed to copy binary to {install_path}");
    }

    // Chmod + codesign + remove quarantine
    let _ = std::process::Command::new("chmod").args(["+x", &install_path]).status();
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("codesign")
            .args(["-s", "-", "--force", &install_path])
            .status();
        let _ = std::process::Command::new("xattr")
            .args(["-rd", "com.apple.quarantine", &install_path])
            .status();
    }

    // Ensure ~/.local/bin is in PATH
    let path = std::env::var("PATH").unwrap_or_default();
    if !path.contains(&install_dir) {
        let shell_rc = format!("{home}/.zshrc");
        let export_line = format!("export PATH=\"$HOME/.local/bin:$PATH\"");
        let rc_content = std::fs::read_to_string(&shell_rc).unwrap_or_default();
        if !rc_content.contains(".local/bin") {
            let _ = std::fs::OpenOptions::new().create(true).append(true).open(&shell_rc)
                .and_then(|mut f| {
                    use std::io::Write;
                    writeln!(f, "\n# tncli\n{export_line}")
                });
            println!("\n{YELLOW}Added ~/.local/bin to PATH in ~/.zshrc{NC}");
            println!("{DIM}Run: source ~/.zshrc (or restart terminal){NC}");
        }
    }

    // Remove old binary from /usr/local/bin if it exists
    let old_path = "/usr/local/bin/tncli";
    if std::path::Path::new(old_path).exists() {
        println!("{BLUE}>>>{NC} Removing old binary from {old_path}...");
        let _ = std::process::Command::new("sudo").args(["rm", old_path]).status();
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmpdir);

    println!("\n{GREEN}v{latest} installed to {install_path}{NC}");
    Ok(())
}

pub fn cmd_db_reset(config: &Config, workspace_branch: &str) -> Result<()> {
    let config_path = crate::config::find_config()?;
    let config_dir = config_path.parent().unwrap_or(Path::new("."));

    // Collect all DBs for this workspace across all repos
    // For each repo, resolve actual branch: workspace branch or per-repo default
    let mut dbs: Vec<(String, String, u16, String, String)> = Vec::new(); // (repo, db_name, port, user, pw)

    for (dir_name, dir) in &config.repos {
        let wt_cfg = match dir.wt() {
            Some(wt) => wt,
            None => continue,
        };

        // Resolve actual branch for this repo in this workspace
        let repo_branch = if workspace_branch == config.global_default_branch() {
            // Main workspace — use per-repo default branch
            config.default_branch_for(dir_name)
        } else {
            // Worktree workspace — use workspace branch (or detect from git)
            let ws_dir = config_dir.join(format!("workspace--{workspace_branch}")).join(dir_name);
            if ws_dir.exists() {
                // Read actual git branch from worktree
                let output = std::process::Command::new("git")
                    .args(["-C", &ws_dir.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
                    .output();
                output.ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_else(|| workspace_branch.to_string())
            } else {
                workspace_branch.to_string()
            }
        };

        let branch_safe = crate::services::branch_safe(&repo_branch);
        let pg_svc = config.shared_services.values().find(|s| s.db_user.is_some());
        let pg_port = pg_svc.and_then(|s| s.ports.first())
            .and_then(|p| p.split(':').next()).and_then(|p| p.parse().ok()).unwrap_or(5432u16);
        let pg_user = pg_svc.and_then(|s| s.db_user.as_deref()).unwrap_or("postgres");
        let pg_pw = pg_svc.and_then(|s| s.db_password.as_deref()).unwrap_or("postgres");

        for sref in &wt_cfg.shared_services {
            if let Some(db_tpl) = &sref.db_name {
                let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                    .replace("{{branch}}", &repo_branch);
                dbs.push((dir_name.clone(), db_name, pg_port, pg_user.to_string(), pg_pw.to_string()));
            }
        }
        for db_tpl in &wt_cfg.databases {
            let db_name = db_tpl.replace("{{branch_safe}}", &branch_safe)
                .replace("{{branch}}", &repo_branch);
            dbs.push((dir_name.clone(), format!("{}_{db_name}", config.session), pg_port, pg_user.to_string(), pg_pw.to_string()));
        }
    }

    if dbs.is_empty() {
        println!("{YELLOW}No databases found for workspace '{workspace_branch}'{NC}");
        return Ok(());
    }

    println!("{BOLD}Resetting databases for workspace '{workspace_branch}':{NC}");
    for (repo, db_name, _, _, _) in &dbs {
        println!("  {repo}: {db_name}");
    }
    println!();

    // Group DBs by host:port for batch operations
    let db_names: Vec<String> = dbs.iter().map(|(_, db, _, _, _)| db.clone()).collect();
    let host = config.shared_services.values()
        .find(|s| s.db_user.is_some())
        .and_then(|s| s.host.as_deref())
        .unwrap_or("localhost");
    let port = dbs.first().map(|(_, _, p, _, _)| *p).unwrap_or(5432);
    let user = dbs.first().map(|(_, _, _, u, _)| u.as_str()).unwrap_or("postgres");
    let pw = dbs.first().map(|(_, _, _, _, p)| p.as_str()).unwrap_or("postgres");

    // Batch drop (single container)
    print!("{BLUE}>>>{NC} dropping {} databases...", db_names.len());
    if crate::services::drop_shared_dbs_batch(host, port, &db_names, user, pw) {
        println!(" {GREEN}ok{NC}");
    } else {
        println!(" {YELLOW}some failed{NC}");
    }

    // Batch create (single container)
    print!("{BLUE}>>>{NC} creating {} databases...", db_names.len());
    crate::services::create_shared_dbs_batch(host, port, &db_names, user, pw);
    println!(" {GREEN}ok{NC}");

    println!("\n{GREEN}Database reset complete for workspace '{workspace_branch}'.{NC}");
    println!("Run migrations to restore schema (e.g. via TUI shortcuts).");
    Ok(())
}

// ── Proxy commands ──

pub fn cmd_proxy_start() -> Result<()> {
    use crate::services::proxy;

    if proxy::is_proxy_running() {
        println!("{GREEN}proxy already running{NC} (pid {})", proxy::read_pid().unwrap_or(0));
        return Ok(());
    }

    // Register routes from config before starting
    let config_path = crate::config::find_config()?;
    let config = crate::config::Config::load(&config_path)?;
    register_proxy_routes_from_config(&config);

    // Find our own binary path
    let exe = std::env::current_exe()?;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let _ = std::fs::create_dir_all(format!("{home}/.tncli"));
    let log_path = format!("{home}/.tncli/proxy.log");

    let child = std::process::Command::new(&exe)
        .args(["proxy", "serve"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    println!("{GREEN}proxy started{NC} (pid {})", child.id());
    println!("  log: {log_path} (errors only, max 1MB)");
    Ok(())
}

/// Register proxy routes for main + all existing worktrees.
fn register_proxy_routes_from_config(config: &crate::config::Config) {
    use crate::services::proxy;

    // Collect proxy ports: repo-level + per-service (name, port)
    let mut proxy_entries: Vec<(&str, u16)> = Vec::new();
    for (_, dir) in &config.repos {
        // Repo-level proxy_port → uses alias as hostname component
        if let (Some(alias), Some(port)) = (dir.alias.as_deref(), dir.proxy_port) {
            proxy_entries.push((alias, port));
        }
        // Per-service proxy_port → uses service name as hostname component
        for (svc_name, svc) in &dir.services {
            if let Some(port) = svc.proxy_port {
                proxy_entries.push((svc_name.as_str(), port));
            }
        }
    }

    if proxy_entries.is_empty() {
        return;
    }

    // Register main workspace routes
    let default_branch = config.default_branch.as_deref().unwrap_or("main");
    let main_ip = crate::services::main_ip(&config.session, default_branch);
    let branch_safe = crate::services::branch_safe(default_branch);
    let main_services: Vec<(&str, u16, &str)> = proxy_entries.iter()
        .map(|&(name, port)| (name, port, main_ip.as_str()))
        .collect();
    proxy::register_routes(&config.session, &branch_safe, &main_services);

    // Scan workspace folders on disk → allocate IPs if missing → register routes
    if let Some(config_dir) = std::env::current_dir().ok() {
        for entry in std::fs::read_dir(&config_dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(branch) = name.strip_prefix("workspace--") {
                if !entry.path().is_dir() { continue; }
                let ws_key = format!("ws-{branch}");
                let ip = crate::services::allocate_ip(&config.session, &ws_key);
                let bs = crate::services::branch_safe(branch);
                let services: Vec<(&str, u16, &str)> = proxy_entries.iter()
                    .map(|&(name, port)| (name, port, ip.as_str()))
                    .collect();
                proxy::register_routes(&config.session, &bs, &services);
            }
        }
    }
}

pub fn cmd_proxy_stop() -> Result<()> {
    use crate::services::proxy;

    if let Some(pid) = proxy::read_pid() {
        let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
        proxy::remove_pid();
        println!("{GREEN}proxy stopped{NC} (was pid {pid})");
    } else {
        println!("proxy not running");
    }
    Ok(())
}

pub fn cmd_proxy_restart() -> Result<()> {
    cmd_proxy_stop()?;
    // Clear stale routes
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let _ = std::fs::remove_file(format!("{home}/.tncli/proxy-routes.json"));
    cmd_proxy_start()
}

pub fn cmd_proxy_status() -> Result<()> {
    use crate::services::proxy;

    if proxy::is_proxy_running() {
        println!("{GREEN}proxy running{NC} (pid {})", proxy::read_pid().unwrap_or(0));
    } else {
        println!("{YELLOW}proxy not running{NC}");
    }

    let routes = proxy::load_routes();
    if routes.routes.is_empty() {
        println!("no routes configured");
    } else {
        println!("\n{BOLD}Listen ports:{NC} {:?}", routes.listen_ports);
        println!("\n{BOLD}Routes:{NC}");
        let mut entries: Vec<_> = routes.routes.iter().collect();
        entries.sort_by_key(|(k, _)| (*k).clone());
        for (hostname, target) in entries {
            println!("  {BLUE}{hostname}{NC} → {target}");
        }
    }
    Ok(())
}

pub fn cmd_proxy_install() -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe_path = exe.to_string_lossy();

    #[cfg(target_os = "macos")]
    {
        let plist_dir = format!("{}/Library/LaunchAgents", std::env::var("HOME")?);
        let plist_path = format!("{plist_dir}/com.tncli.proxy.plist");
        let log_path = format!("{}/.tncli/proxy.log", std::env::var("HOME")?);

        let plist = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tncli.proxy</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_path}</string>
        <string>proxy</string>
        <string>serve</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log_path}</string>
    <key>StandardErrorPath</key>
    <string>{log_path}</string>
</dict>
</plist>"#);

        let _ = std::fs::create_dir_all(&plist_dir);
        std::fs::write(&plist_path, &plist)?;

        // Unload first if already loaded
        let _ = std::process::Command::new("launchctl")
            .args(["unload", &plist_path])
            .output();

        let status = std::process::Command::new("launchctl")
            .args(["load", &plist_path])
            .status()?;

        if status.success() {
            println!("{GREEN}proxy daemon installed and started{NC}");
            println!("  plist: {plist_path}");
            println!("  log:   {log_path}");
        } else {
            bail!("failed to load launchd plist");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        println!("{YELLOW}daemon install not yet supported on this OS{NC}");
        println!("Run manually: {exe_path} proxy serve");
    }

    Ok(())
}

pub fn cmd_proxy_uninstall() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = format!("{}/Library/LaunchAgents/com.tncli.proxy.plist", std::env::var("HOME")?);

        if std::path::Path::new(&plist_path).exists() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", &plist_path])
                .output();
            let _ = std::fs::remove_file(&plist_path);
            println!("{GREEN}proxy daemon uninstalled{NC}");
        } else {
            println!("proxy daemon not installed");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        println!("{YELLOW}daemon uninstall not yet supported on this OS{NC}");
    }

    Ok(())
}
