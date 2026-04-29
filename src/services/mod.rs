mod compose;
mod docker;
mod files;
mod git;
mod ip;
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

/// Resolve template variables in env values.
/// `ws_key` is used for `{{slot:SERVICE}}` lookup (e.g. "ws-main", "ws-feat-123").
pub fn resolve_env_templates(
    env: &indexmap::IndexMap<String, String>,
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
            (k.clone(), val)
        })
        .collect()
}

// ── Re-exports (only items used outside this module) ──

// git
pub use git::{list_branches, list_worktrees, create_worktree, create_worktree_from_base, remove_worktree};

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
pub use ip::{allocate_ip, release_ip, load_ip_allocations, check_etc_hosts};

// docker
pub use docker::{
    create_docker_network, remove_docker_network,
    ensure_workspace_folder, delete_workspace_folder,
    ensure_main_workspace,
};
