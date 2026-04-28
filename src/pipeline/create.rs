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
        CreateStage::Source => stage_source(ctx, state),
        CreateStage::Configure => stage_configure(ctx, state),
        CreateStage::Setup => stage_setup(ctx, state),
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

    // Setup main dirs + create databases for main
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
                crate::services::setup_main_as_worktree(
                    std::path::Path::new(dir_path), &compose_files, &wt.env, main_branch,
                    if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
                    &shared_hosts,
                );
            }

            // Write env file for main dir
            let main_branch = ctx.dir_branches.iter()
                .find(|(d, _)| d == dir_name)
                .map(|(_, b)| b.as_str())
                .unwrap_or("main");
            let main_branch_safe = crate::services::branch_safe(main_branch);
            let resolved = crate::services::resolve_env_templates(&wt.env, "127.0.0.1", &main_branch_safe, main_branch);
            let env_file = wt.env_file.as_deref().unwrap_or(".env.local");
            let p = std::path::Path::new(dir_path);
            crate::services::apply_env_overrides(p, &resolved, env_file);
            let _ = crate::services::write_env_file(p, "127.0.0.1");

            // Create DB for main dir
            for sref in &wt.shared_services {
                if let Some(db_tpl) = &sref.db_name {
                    let db_name = db_tpl.replace("{{branch_safe}}", &main_branch_safe)
                        .replace("{{branch}}", main_branch);
                    create_single_db(&ctx.config, &sref.name, &db_name);
                }
            }
        }
    }

    Ok(())
}

// ── Stage 4: Source ──

fn stage_source(ctx: &CreateContext, state: &mut CreateState) -> Result<()> {
    for (dir_name, base_branch) in &ctx.dir_branches {
        let dir_path = match ctx.dir_paths.iter().find(|(d, _)| d == dir_name) {
            Some((_, p)) => p.clone(),
            None => continue,
        };
        let wt_cfg = ctx.config.repos.get(dir_name).and_then(|d| d.wt());
        let copy_files = wt_cfg.map(|wt| wt.copy.clone()).unwrap_or_default();

        match crate::services::create_worktree_from_base(
            std::path::Path::new(&dir_path), &ctx.branch, base_branch, &copy_files, Some(&state.ws_folder),
        ) {
            Ok(wt_path) => {
                state.wt_dirs.push((dir_name.clone(), wt_path));
            }
            Err(e) => {
                bail!("Failed to create worktree for {dir_name}: {e}");
            }
        }
    }
    Ok(())
}

// ── Stage 5: Configure ──

fn stage_configure(ctx: &CreateContext, state: &CreateState) -> Result<()> {
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

        // Generate compose override (without network — network added in Stage 7)
        crate::services::generate_compose_override(
            std::path::Path::new(&dir_path), wt_path, &state.bind_ip,
            &compose_files, &worktree_env, &ctx.branch, None,
            if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
            &shared_hosts,
        );
        let _ = crate::services::write_env_file(wt_path, &state.bind_ip);

        // Write .env.local
        let resolved = crate::services::resolve_env_templates(&worktree_env, &state.bind_ip, &state.branch_safe, &ctx.branch);
        let env_file = wt_cfg.and_then(|wt| wt.env_file.as_deref()).unwrap_or(".env.local");
        crate::services::apply_env_overrides(wt_path, &resolved, env_file);
    }

    crate::services::ensure_global_gitignore();
    Ok(())
}

// ── Stage 6: Setup ──

fn stage_setup(ctx: &CreateContext, state: &CreateState) -> Result<()> {
    for (dir_name, wt_path) in &state.wt_dirs {
        let setup = ctx.config.repos.get(dir_name)
            .and_then(|d| d.wt())
            .map(|wt| wt.setup.clone())
            .unwrap_or_default();

        if !setup.is_empty() {
            let combined = setup.join(" && ");
            let status = std::process::Command::new("zsh")
                .args(["-lc", &combined])
                .current_dir(wt_path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();

            if let Err(e) = status {
                bail!("Setup failed for {dir_name}: {e}");
            }
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

        crate::services::generate_compose_override(
            std::path::Path::new(&dir_path), wt_path, &state.bind_ip,
            &compose_files, &worktree_env, &ctx.branch, Some(&state.network_name),
            if svc_overrides.is_empty() { None } else { Some(&svc_overrides) },
            &shared_hosts,
        );
    }

    Ok(())
}

// ── Helpers ──

fn create_databases(ctx: &CreateContext, branch_safe: &str, branch: &str) {
    for dir_name in &ctx.unique_dirs {
        if let Some(dir) = ctx.config.repos.get(dir_name) {
            if let Some(wt_cfg) = dir.wt() {
                for sref in &wt_cfg.shared_services {
                    if let Some(db_tpl) = &sref.db_name {
                        let db_name = db_tpl.replace("{{branch_safe}}", branch_safe)
                            .replace("{{branch}}", branch);
                        create_single_db(&ctx.config, &sref.name, &db_name);
                    }
                }
            }
        }
    }
}

fn create_single_db(config: &crate::config::Config, svc_name: &str, db_name: &str) {
    let svc_def = config.shared_services.get(svc_name);
    let host = svc_def.and_then(|d| d.host.as_deref()).unwrap_or("localhost");
    let port = svc_def.and_then(|d| d.ports.first())
        .and_then(|p| p.split(':').next())
        .and_then(|p| p.parse().ok())
        .unwrap_or(5432);
    let user = svc_def.and_then(|d| d.db_user.as_deref()).unwrap_or("postgres");
    let pw = svc_def.and_then(|d| d.db_password.as_deref()).unwrap_or("postgres");
    crate::services::create_shared_db(host, port, db_name, user, pw);
}
