mod compose;
pub(crate) mod dns;
mod docker;
mod files;
pub(crate) mod git;
pub(crate) mod ip;
pub mod proxy;
mod workspace;

use std::path::PathBuf;

// ── Shared types ──

/// Info about a single worktree instance.
#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeInfo {
    pub branch: String,
    pub parent_dir: String,
    pub bind_ip: String,
    pub path: PathBuf,
}

// ── DRY utilities ──

/// Sanitize branch name for safe use in DB names, env vars, etc.
pub fn branch_safe(branch: &str) -> String {
    branch.replace('/', "_").replace('-', "_")
}

/// Resolve `{{slot:SERVICE_NAME}}` templates in a single string using slot allocations.
pub fn resolve_slot_templates(val: &str, ws_key: &str) -> String {
    let mut result = val.to_string();
    let allocs = workspace::load_slot_allocations();
    while let Some(start) = result.find("{{slot:") {
        let Some(end) = result[start..].find("}}").map(|e| start + e + 2) else { break };
        let svc_name = &result[start + 7..end - 2];
        let slot = allocs.get(svc_name)
            .and_then(|svc| svc.slots.get(ws_key))
            .map(|a| a.slot)
            .unwrap_or(0);
        result = format!("{}{}{}", &result[..start], slot, &result[end..]);
    }
    result
}

/// Find a repo by name (key) first, then by alias as fallback.
/// Returns (alias_or_name, proxy_port).
fn find_repo<'a>(config: &'a crate::config::Config, name: &'a str) -> Option<(&'a str, Option<u16>)> {
    // By repo name (key)
    if let Some(dir) = config.repos.get(name) {
        let alias = dir.alias.as_deref().unwrap_or(name);
        return Some((alias, dir.proxy_port));
    }
    // By alias (fallback)
    config.repos.iter()
        .find(|(_, d)| d.alias.as_deref() == Some(name))
        .map(|(_, d)| (name, d.proxy_port))
}

/// Resolve `{{host:NAME}}`, `{{port:NAME}}`, `{{url:NAME}}` templates from Config.
/// Looks up shared_services by name, repos by name then alias.
pub fn resolve_config_templates(val: &str, config: &crate::config::Config, branch_safe: &str) -> String {
    let mut result = val.to_string();

    // {{host:NAME}} → shared: {session}.{name}.tncli.test, repo: {session}.{alias}.ws-{branch_safe}.tncli.test
    while let Some(start) = result.find("{{host:") {
        let Some(end) = result[start..].find("}}").map(|e| start + e + 2) else { break };
        let name = &result[start + 7..end - 2];
        let host = if config.shared_services.contains_key(name) {
            config.shared_host(name)
        } else if let Some((alias, _)) = find_repo(config, name) {
            format!("{}.{alias}.ws-{branch_safe}.tncli.test", config.session)
        } else {
            format!("{}.{name}.ws-{branch_safe}.tncli.test", config.session)
        };
        result = format!("{}{}{}", &result[..start], host, &result[end..]);
    }

    // {{port:NAME}} → shared: first mapped host port, repo: proxy_port
    while let Some(start) = result.find("{{port:") {
        let Some(end) = result[start..].find("}}").map(|e| start + e + 2) else { break };
        let name = &result[start + 7..end - 2];
        let port = if let Some(svc) = config.shared_services.get(name) {
            svc.ports.first()
                .and_then(|p| p.split(':').next())
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(0)
        } else {
            find_repo(config, name).and_then(|(_, p)| p).unwrap_or(0)
        };
        result = format!("{}{}{}", &result[..start], port, &result[end..]);
    }

    // {{url:NAME}} → http://{host}:{port}
    while let Some(start) = result.find("{{url:") {
        let Some(end) = result[start..].find("}}").map(|e| start + e + 2) else { break };
        let name = &result[start + 6..end - 2];
        let host = if config.shared_services.contains_key(name) {
            config.shared_host(name)
        } else if let Some((alias, _)) = find_repo(config, name) {
            format!("{}.{alias}.ws-{branch_safe}.tncli.test", config.session)
        } else {
            format!("{}.{name}.ws-{branch_safe}.tncli.test", config.session)
        };
        let port = if let Some(svc) = config.shared_services.get(name) {
            svc.ports.first()
                .and_then(|p| p.split(':').next())
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(0)
        } else {
            find_repo(config, name).and_then(|(_, p)| p).unwrap_or(0)
        };
        result = format!("{}http://{}:{}{}", &result[..start], host, port, &result[end..]);
    }

    // {{conn:NAME}} → user:password@host:port (from shared_services with db_user/db_password)
    while let Some(start) = result.find("{{conn:") {
        let Some(end) = result[start..].find("}}").map(|e| start + e + 2) else { break };
        let name = &result[start + 7..end - 2];
        let conn = if let Some(svc) = config.shared_services.get(name) {
            let user = svc.db_user.as_deref().unwrap_or("postgres");
            let pw = svc.db_password.as_deref().unwrap_or("postgres");
            // Use hostname — works for both Docker (via extra_hosts) and host (via /etc/hosts).
            let host = config.shared_host(name);
            let port = svc.ports.first()
                .and_then(|p| p.split(':').next())
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(5432);
            format!("{user}:{pw}@{host}:{port}")
        } else {
            String::new()
        };
        result = format!("{}{}{}", &result[..start], conn, &result[end..]);
    }

    result
}

/// Resolve `{{db:INDEX}}` templates using pre-resolved database names.
pub fn resolve_db_templates(val: &str, db_names: &[String]) -> String {
    let mut result = val.to_string();
    while let Some(start) = result.find("{{db:") {
        let Some(end) = result[start..].find("}}").map(|e| start + e + 2) else { break };
        let idx_str = &result[start + 5..end - 2];
        let resolved = idx_str.parse::<usize>().ok()
            .and_then(|i| db_names.get(i))
            .cloned()
            .unwrap_or_default();
        result = format!("{}{}{}", &result[..start], resolved, &result[end..]);
    }
    result
}

/// Resolve template variables in env values.
/// `ws_key` is used for `{{slot:SERVICE}}` lookup (e.g. "ws-main", "ws-feat-123").
pub fn resolve_env_templates(
    env: &indexmap::IndexMap<String, String>,
    config: &crate::config::Config,
    bind_ip: &str,
    branch_safe: &str,
    branch: &str,
    ws_key: &str,
) -> Vec<(String, String)> {
    env.iter()
        .map(|(k, v)| {
            let val = v.replace("{{bind_ip}}", bind_ip)
                .replace("{{branch_safe}}", branch_safe)
                .replace("{{branch}}", branch);
            let val = resolve_slot_templates(&val, ws_key);
            let val = resolve_config_templates(&val, config, branch_safe);
            (k.clone(), val)
        })
        .collect()
}

// ── Re-exports (only items used outside this module) ──

// git
pub use git::{list_worktrees, create_worktree, create_worktree_from_base, remove_worktree};

// files
pub use files::{apply_env_overrides, write_env_file, ensure_global_gitignore, ensure_node_bind_host};

// compose
pub use compose::{generate_compose_override, setup_main_as_worktree};

// shared services
pub use workspace::{
    generate_shared_compose, start_shared_services,
    create_shared_dbs_batch, drop_shared_dbs_batch,
    allocate_slot, release_slot,
};

// ip
pub use ip::{allocate_ip, release_ip, load_ip_allocations, check_etc_hosts, main_ip, migrate_legacy_ips, SETUP_SUBNET_COUNT, SETUP_HOST_MAX};

// docker
pub use docker::{
    create_docker_network, remove_docker_network,
    ensure_workspace_folder, delete_workspace_folder,
    ensure_main_workspace,
};
