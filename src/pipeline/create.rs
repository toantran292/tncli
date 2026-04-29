use std::path::PathBuf;

use anyhow::{bail, Result};

use super::context::CreateContext;
use super::stages::CreateStage;

/// Mutable state accumulated across stages.
pub struct CreateState {
    pub ws_folder: PathBuf,
    pub network_name: String,
    pub branch_safe: String,
    pub bind_ip: String,
    pub wt_dirs: Vec<(String, PathBuf)>,
}

impl CreateState {
    pub fn new(ctx: &CreateContext) -> Self {
        Self {
            ws_folder: PathBuf::new(),
            network_name: format!("tncli-ws-{}", ctx.branch),
            branch_safe: crate::services::branch_safe(&ctx.branch),
            bind_ip: ctx.bind_ip.clone(),
            wt_dirs: Vec::new(),
        }
    }
}

/// Execute a single stage.
pub fn execute_stage(stage: &CreateStage, ctx: &CreateContext, state: &mut CreateState) -> Result<()> {
    match stage {
        CreateStage::Validate => stage_validate(ctx),
        CreateStage::Provision => stage_provision(ctx, state),
        CreateStage::Infra => stage_infra(ctx, state),
        CreateStage::Source => stage_source_parallel(ctx, state),
        CreateStage::Configure => stage_configure_parallel(ctx, state),
        CreateStage::Setup => stage_setup_parallel(ctx, state),
        CreateStage::Network => stage_network(ctx, state),
    }
}

// ── Stage 1: Validate ──

fn stage_validate(ctx: &CreateContext) -> Result<()> {
    if !ctx.config.shared_services.is_empty() {
        let hostnames: Vec<&str> = ctx.config.shared_services.values()
            .filter_map(|s| s.host.as_deref())
            .collect();
        let missing = crate::services::check_etc_hosts(&hostnames);
        if !missing.is_empty() {
            bail!("Missing hosts in /etc/hosts: {}. Run: tncli setup", missing.join(", "));
        }
    }
    Ok(())
}

// ── Stage 2: Provision ──

fn stage_provision(ctx: &CreateContext, state: &mut CreateState) -> Result<()> {
    // Allocate IP if not already set
    if state.bind_ip.is_empty() {
        state.bind_ip = crate::services::allocate_ip(&format!("ws-{}", ctx.branch));
    }

    // Allocate shared service slots
    if !ctx.config.shared_services.is_empty() {
        let ws_key = format!("ws-{}", ctx.branch);
        for dir_name in &ctx.unique_dirs {
            if let Some(dir) = ctx.config.repos.get(dir_name) {
                if let Some(wt_cfg) = dir.wt() {
                    for sref in &wt_cfg.shared_services {
                        if let Some(svc_def) = ctx.config.shared_services.get(&sref.name) {
                            if let Some(capacity) = svc_def.capacity {
                                let base_port = svc_def.ports.first()
                                    .and_then(|p| p.split(':').next())
                                    .and_then(|p| p.parse::<u16>().ok())
                                    .unwrap_or(6379);
                                crate::services::allocate_slot(&sref.name, &ws_key, capacity, base_port);
                            }
                        }
                    }
                }
            }
        }
    }

    // Create workspace folder
    state.ws_folder = crate::services::ensure_workspace_folder(&ctx.config_dir, &ctx.branch);

    Ok(())
}

// ── Stage 3: Infra ──

fn stage_infra(ctx: &CreateContext, state: &CreateState) -> Result<()> {
    if ctx.config.shared_services.is_empty() {
        return Ok(());
    }

    // Collect needed shared services
    let mut needed: Vec<String> = Vec::new();
    for dir_name in &ctx.unique_dirs {
        if let Some(dir) = ctx.config.repos.get(dir_name) {
            if let Some(wt_cfg) = dir.wt() {
                for sref in &wt_cfg.shared_services {
                    if !needed.contains(&sref.name) { needed.push(sref.name.clone()); }
                }
            }
        }
    }

    // Generate + start shared compose
    crate::services::generate_shared_compose(&ctx.config_dir, &ctx.session, &ctx.config.shared_services);
    let refs: Vec<&str> = needed.iter().map(|s| s.as_str()).collect();
    crate::services::start_shared_services(&ctx.config_dir, &ctx.session, &refs);

    // Create databases for worktree branch
    create_databases(ctx, &state.branch_safe, &ctx.branch);

    // Setup main dirs + collect databases for batch creation
    let mut main_dbs: Vec<String> = Vec::new();
    let mut main_db_host = "localhost";
    let mut main_db_port = 5432u16;
    let mut main_db_user = "postgres";
    let mut main_db_pw = "postgres";
    for (dir_name, dir_path) in &ctx.dir_paths {
        let dir_cfg = ctx.config.repos.get(dir_name);
        let wt_cfg = dir_cfg.and_then(|d| d.wt());
        if let Some(wt) = wt_cfg {
            // Setup main as worktree (compose override + env files)
            if !wt.compose_files.is_empty() || std::path::Path::new(dir_path).join("docker-compose.yml").is_file() {
                let compose_files = if wt.compose_files.is_empty() {
                    vec!["docker-compose.yml".to_string()]
                } else {
                    wt.compose_files.clone()
                };
                let (svc_overrides, shared_hosts) = ctx.shared_overrides.iter()
                    .find(|(d, _, _)| d == dir_name)
                    .map(|(_, ov, h)| (ov.clone(), h.clone()))
                    .unwrap_or_default();
                let main_branch = ctx.dir_branches.iter()
                    .find(|(d, _)| d == dir_name)
                    .map(|(_, b)| b.as_str())
                    .unwrap_or("main");
                let main_ws_key = format!("ws-{}", main_branch.replace('/', "-"));
                crate::services::setup_main_as_worktree(
                    std::path::Path::new(dir_path), &compose_files, &wt.env, main_branch,
                    if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                    &shared_hosts, &main_ws_key,
                );
            }

            // Write env file for main dir
            let main_branch = ctx.dir_branches.iter()
                .find(|(d, _)| d == dir_name)
                .map(|(_, b)| b.as_str())
                .unwrap_or("main");
            let main_branch_safe = crate::services::branch_safe(main_branch);
            let main_ws_key = format!("ws-{}", main_branch.replace('/', "-"));
            let p = std::path::Path::new(dir_path);
            wt.apply_all_env_files(p, "127.0.0.1", main_branch, &main_ws_key);
            let _ = crate::services::write_env_file(p, "127.0.0.1");

            // Collect main DBs for batch creation
            for sref in &wt.shared_services {
                if let Some(db_tpl) = &sref.db_name {
                    let db_name = db_tpl.replace("{{branch_safe}}", &main_branch_safe)
                        .replace("{{branch}}", main_branch);
                    main_dbs.push(db_name);
                    let svc_def = ctx.config.shared_services.get(&sref.name);
                    main_db_host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost");
                    main_db_port = svc_def.and_then(|d| d.ports.first())
                        .and_then(|p| p.split(':').next()).and_then(|p| p.parse().ok()).unwrap_or(5432);
                    main_db_user = svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres");
                    main_db_pw = svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres");
                }
            }
        }
    }

    // Batch create main DBs (single container)
    if !main_dbs.is_empty() {
        crate::services::create_shared_dbs_batch(main_db_host, main_db_port, &main_dbs, main_db_user, main_db_pw);
    }

    Ok(())
}

// ── Stage 4: Source ──

// ── Stage 4: Source (parallel per repo) ──

fn stage_source_parallel(ctx: &CreateContext, state: &mut CreateState) -> Result<()> {
    use std::sync::{Arc, Mutex};
    let results: Arc<Mutex<Vec<Result<(String, PathBuf)>>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();

    for (dir_name, base_branch) in &ctx.dir_branches {
        let dir_path = match ctx.dir_paths.iter().find(|(d, _)| d == dir_name) {
            Some((_, p)) => p.clone(),
            None => continue,
        };
        let dir_name = dir_name.clone();
        let base_branch = base_branch.clone();
        let target_branch = ctx.selected_dirs.as_ref()
            .and_then(|sel| sel.iter().find(|(d, _)| *d == dir_name))
            .map(|(_, b)| b.clone())
            .unwrap_or_else(|| ctx.branch.clone());
        let ws_folder = state.ws_folder.clone();
        let copy_files = ctx.config.repos.get(&dir_name)
            .and_then(|d| d.wt()).map(|wt| wt.copy.clone()).unwrap_or_default();
        let results = Arc::clone(&results);

        handles.push(std::thread::spawn(move || {
            let r = crate::services::create_worktree_from_base(
                std::path::Path::new(&dir_path), &target_branch, &base_branch, &copy_files, Some(&ws_folder),
            ).map(|wt_path| (dir_name.clone(), wt_path))
             .map_err(|e| anyhow::anyhow!("Failed to create worktree for {dir_name}: {e}"));
            results.lock().unwrap().push(r);
        }));
    }
    for h in handles { let _ = h.join(); }

    let results = Arc::try_unwrap(results).unwrap().into_inner().unwrap();
    for r in results {
        match r {
            Ok(pair) => state.wt_dirs.push(pair),
            Err(e) => bail!("{e}"),
        }
    }
    Ok(())
}

// ── Stage 5: Configure (parallel per repo) ──

fn stage_configure_parallel(ctx: &CreateContext, state: &CreateState) -> Result<()> {
    let mut handles = Vec::new();
    for (dir_name, wt_path) in &state.wt_dirs {
        let dir_path = ctx.dir_paths.iter()
            .find(|(d, _)| d == dir_name)
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
        let wt_cfg = ctx.config.repos.get(dir_name).and_then(|d| d.wt()).cloned();
        let (svc_overrides, shared_hosts) = ctx.shared_overrides.iter()
            .find(|(d, _, _)| d == dir_name)
            .map(|(_, ov, h)| (ov.clone(), h.clone()))
            .unwrap_or_default();
        let bind_ip = state.bind_ip.clone();
        let ws_branch = ctx.branch.clone();
        let wt_path = wt_path.clone();

        handles.push(std::thread::spawn(move || {
            if let Some(wt) = wt_cfg {
                let compose_files = wt.compose_files.clone();
                let worktree_env = wt.env.clone();
                let ws_key = format!("ws-{}", ws_branch.replace('/', "-"));

                if !compose_files.is_empty() {
                    crate::services::generate_compose_override(
                        std::path::Path::new(&dir_path), &wt_path, &bind_ip,
                        &compose_files, &worktree_env, &ws_branch, None,
                        if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                        &shared_hosts, &ws_key,
                    );
                }
                let _ = crate::services::write_env_file(&wt_path, &bind_ip);

                wt.apply_all_env_files(&wt_path, &bind_ip, &ws_branch, &ws_key);
            }
        }));
    }
    for h in handles { let _ = h.join(); }

    crate::services::ensure_global_gitignore();
    Ok(())
}

// ── Stage 6: Setup (parallel per repo) ──

fn stage_setup_parallel(ctx: &CreateContext, state: &CreateState) -> Result<()> {
    let mut tmux_windows: Vec<String> = Vec::new();

    // Ensure tmux session exists
    crate::tmux::create_session_if_needed(&ctx.session);

    for (dir_name, wt_path) in &state.wt_dirs {
        let setup = ctx.config.repos.get(dir_name)
            .and_then(|d| d.wt())
            .map(|wt| wt.setup.clone())
            .unwrap_or_default();

        if setup.is_empty() { continue; }

        let alias = ctx.config.repos.get(dir_name)
            .and_then(|d| d.alias.as_deref())
            .unwrap_or(dir_name);
        let branch_safe = crate::services::branch_safe(&ctx.branch);
        let win_name = format!("setup~{alias}~{branch_safe}");

        // Run setup in tmux window with remain-on-exit (stays open after command finishes)
        let combined = setup.join(" && ");
        let cmd = format!("cd '{}' && {combined}", wt_path.display());
        crate::tmux::new_window_autoclose(&ctx.session, &win_name, &cmd);
        // Set remain-on-exit so window stays visible after command finishes
        let _ = std::process::Command::new("tmux")
            .args(["set-option", "-t", &format!("={}:{win_name}", ctx.session), "remain-on-exit", "on"])
            .output();
        tmux_windows.push(win_name);
    }

    // Wait for all setup commands to finish, then kill all setup windows
    if !tmux_windows.is_empty() {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let still_running = tmux_windows.iter().any(|w| {
                // Check if pane is still alive (not dead/exited)
                let output = std::process::Command::new("tmux")
                    .args(["list-panes", "-t", &format!("={}:{w}", ctx.session), "-F", "#{pane_dead}"])
                    .output();
                match output {
                    Ok(o) => {
                        let s = String::from_utf8_lossy(&o.stdout);
                        // pane_dead = 0 means still running, 1 means exited
                        s.trim() == "0"
                    }
                    Err(_) => false, // window gone
                }
            });
            if !still_running { break; }
        }
        // All done — kill all setup windows at once
        for w in &tmux_windows {
            let _ = std::process::Command::new("tmux")
                .args(["kill-window", "-t", &format!("={}:{w}", ctx.session)])
                .output();
        }
    }

    Ok(())
}

// ── Stage 7: Network ──

fn stage_network(ctx: &CreateContext, state: &CreateState) -> Result<()> {
    crate::services::create_docker_network(&state.network_name);

    // Regenerate compose overrides with network attached
    for (dir_name, wt_path) in &state.wt_dirs {
        let dir_path = ctx.dir_paths.iter()
            .find(|(d, _)| d == dir_name)
            .map(|(_, p)| p.clone())
            .unwrap_or_default();

        let wt_cfg = ctx.config.repos.get(dir_name).and_then(|d| d.wt());
        let compose_files = wt_cfg.map(|wt| wt.compose_files.clone()).unwrap_or_default();
        let worktree_env = wt_cfg.map(|wt| wt.env.clone()).unwrap_or_default();
        let (svc_overrides, shared_hosts) = ctx.shared_overrides.iter()
            .find(|(d, _, _)| d == dir_name)
            .map(|(_, ov, h)| (ov.clone(), h.clone()))
            .unwrap_or_default();

        let ws_key = format!("ws-{}", ctx.branch.replace('/', "-"));
        crate::services::generate_compose_override(
            std::path::Path::new(&dir_path), wt_path, &state.bind_ip,
            &compose_files, &worktree_env, &ctx.branch, Some(&state.network_name),
            if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
            &shared_hosts, &ws_key,
        );
    }

    Ok(())
}

// ── Helpers ──

fn create_databases(ctx: &CreateContext, branch_safe: &str, branch: &str) {
    let mut db_names = Vec::new();
    let mut host = "localhost";
    let mut port = 5432u16;
    let mut user = "postgres";
    let mut pw = "postgres";

    for dir_name in &ctx.unique_dirs {
        if let Some(dir) = ctx.config.repos.get(dir_name) {
            if let Some(wt_cfg) = dir.wt() {
                for sref in &wt_cfg.shared_services {
                    if let Some(db_tpl) = &sref.db_name {
                        let db_name = db_tpl.replace("{{branch_safe}}", branch_safe)
                            .replace("{{branch}}", branch);
                        let svc_def = ctx.config.shared_services.get(&sref.name);
                        host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost");
                        port = svc_def.and_then(|d| d.ports.first())
                            .and_then(|p| p.split(':').next())
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(5432);
                        user = svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres");
                        pw = svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres");
                        db_names.push(db_name);
                    }
                }
            }
        }
    }
    if !db_names.is_empty() {
        crate::services::create_shared_dbs_batch(host, port, &db_names, user, pw);
    }
}
