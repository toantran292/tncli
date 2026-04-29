use std::collections::HashSet;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::config::{Config, ServiceOverride};

/// All data needed to run the create workspace pipeline.
/// Decoupled from App — can be built from Config alone (for CLI).
pub struct CreateContext {
    pub workspace_name: String,
    pub branch: String,
    pub config: Config,
    pub config_dir: PathBuf,
    pub session: String,
    pub unique_dirs: Vec<String>,
    pub dir_paths: Vec<(String, String)>,
    pub dir_branches: Vec<(String, String)>,
    pub shared_overrides: Vec<(String, IndexMap<String, ServiceOverride>, Vec<String>)>,
    pub bind_ip: String,
    pub skip_stages: HashSet<usize>,
    /// Per-repo target branches (dir_name → branch). If set, overrides ctx.branch for each dir.
    pub selected_dirs: Option<Vec<(String, String)>>,
}

impl CreateContext {
    /// Build context from Config (for CLI — no App needed).
    pub fn from_config(
        config: &Config,
        config_path: &Path,
        ws_name: &str,
        branch: &str,
        skip_stages: HashSet<usize>,
    ) -> anyhow::Result<Self> {
        let workspaces = config.all_workspaces();
        let entries = workspaces.get(ws_name)
            .ok_or_else(|| anyhow::anyhow!("workspace '{}' not found", ws_name))?;

        let mut unique_dirs = Vec::new();
        for entry in entries {
            if let Ok((dir, _)) = config.find_service_entry(entry) {
                if !unique_dirs.contains(&dir) {
                    unique_dirs.push(dir);
                }
            }
        }
        if unique_dirs.is_empty() {
            anyhow::bail!("no dirs found in workspace '{}'", ws_name);
        }

        let config_dir = config_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let default_branch = config.global_default_branch();

        // Resolve dir paths (through main workspace folder)
        let dir_paths: Vec<(String, String)> = unique_dirs.iter()
            .map(|d| {
                let p = Path::new(d);
                let resolved = if p.is_absolute() {
                    d.to_string()
                } else {
                    // Try workspace folder first, fallback to config dir
                    let ws_path = config_dir.join(format!("workspace--{default_branch}")).join(d);
                    if ws_path.exists() {
                        ws_path.to_string_lossy().into_owned()
                    } else {
                        config_dir.join(d).to_string_lossy().into_owned()
                    }
                };
                (d.clone(), resolved)
            })
            .collect();

        // Resolve dir branches (current git branch for each dir)
        let dir_branches: Vec<(String, String)> = unique_dirs.iter()
            .map(|d| {
                let dir_path = dir_paths.iter()
                    .find(|(name, _)| name == d)
                    .map(|(_, p)| p.as_str())
                    .unwrap_or(".");
                let branch = git_branch(dir_path).unwrap_or_else(|| "main".to_string());
                (d.clone(), branch)
            })
            .collect();

        // Resolve shared overrides for each dir
        let shared_overrides: Vec<(String, IndexMap<String, ServiceOverride>, Vec<String>)> =
            unique_dirs.iter()
                .map(|d| {
                    let (ov, hosts) = resolve_shared_overrides(config, d);
                    (d.clone(), ov, hosts)
                })
                .collect();

        // IP will be allocated in Provision stage if not provided
        let bind_ip = String::new();

        Ok(Self {
            workspace_name: ws_name.to_string(),
            branch: branch.to_string(),
            config: config.clone(),
            config_dir,
            session: config.session.clone(),
            unique_dirs,
            dir_paths,
            dir_branches,
            shared_overrides,
            bind_ip,
            skip_stages,
            selected_dirs: None,
        })
    }

    /// Build context with specific repo selection (from TUI checklist).
    #[allow(dead_code)]
    pub fn from_config_with_selection(
        config: &Config,
        config_path: &Path,
        ws_name: &str,
        branch: &str,
        selected: Vec<(String, String)>, // (dir_name, target_branch)
    ) -> anyhow::Result<Self> {
        let mut ctx = Self::from_config(config, config_path, ws_name, branch, HashSet::new())?;
        // Filter unique_dirs to only selected ones
        let selected_dir_names: Vec<String> = selected.iter().map(|(d, _)| d.clone()).collect();
        ctx.unique_dirs.retain(|d| selected_dir_names.contains(d));
        ctx.dir_paths.retain(|(d, _)| selected_dir_names.contains(d));
        ctx.dir_branches.retain(|(d, _)| selected_dir_names.contains(d));
        ctx.shared_overrides.retain(|(d, _, _)| selected_dir_names.contains(d));
        ctx.selected_dirs = Some(selected);
        Ok(ctx)
    }
}

/// All data needed to run the delete workspace pipeline.
#[allow(dead_code)]
pub struct DeleteContext {
    pub branch: String,
    pub config: Config,
    pub config_dir: PathBuf,
    pub session: String,
    pub wt_keys: Vec<String>,
    pub cleanup_items: Vec<CleanupItem>,
    pub dbs_to_drop: Vec<DbDropItem>,
    pub network: String,
    pub skip_stages: HashSet<usize>,
}

pub struct CleanupItem {
    pub dir_path: String,
    pub wt_path: PathBuf,
    pub wt_branch: String,
    pub pre_delete: Vec<String>,
}

pub struct DbDropItem {
    pub host: String,
    pub port: u16,
    pub db_name: String,
    pub user: String,
    pub password: String,
}

// ── Standalone helpers (extracted from App) ──

/// Resolve shared service overrides for a dir.
/// Returns (service_overrides merged with shared disabled profiles, shared hostnames).
pub fn resolve_shared_overrides(
    config: &Config,
    dir_name: &str,
) -> (IndexMap<String, ServiceOverride>, Vec<String>) {
    let dir = match config.repos.get(dir_name) {
        Some(d) => d,
        None => return (Default::default(), Vec::new()),
    };
    let wt_cfg = match dir.wt() {
        Some(wt) => wt,
        None => return (Default::default(), Vec::new()),
    };
    let mut overrides = wt_cfg.service_overrides.clone();
    let mut hosts: Vec<String> = Vec::new();

    for sref in &wt_cfg.shared_services {
        if !overrides.contains_key(&sref.name) {
            overrides.insert(sref.name.clone(), ServiceOverride {
                environment: IndexMap::new(),
                profiles: vec!["disabled".to_string()],
                mem_limit: None,
            });
        }
        if let Some(svc_def) = config.shared_services.get(&sref.name) {
            if let Some(host) = &svc_def.host {
                if !hosts.contains(host) {
                    hosts.push(host.clone());
                }
            }
        }
    }
    // Add proxy hostnames for all repos with proxy_port (so Docker can reach them via host-gateway)
    for (_, repo) in &config.repos {
        if let Some(port) = repo.proxy_port {
            if let Some(alias) = &repo.alias {
                // Proxy hostname will be resolved by the proxy on 127.0.0.1
                // Docker containers reach it via host-gateway → 127.0.0.1:port → proxy → bind_ip:port
                let _ = port; // port used for route registration, not for extra_hosts
                let hostname = format!("{alias}.tncli.test");
                if !hosts.contains(&hostname) {
                    hosts.push(hostname);
                }
            }
        }
    }

    (overrides, hosts)
}

/// Get current git branch for a directory path.
fn git_branch(dir_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["-C", dir_path, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}
