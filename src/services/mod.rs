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

/// Resolve template variables in env values.
pub fn resolve_env_templates(
    env: &indexmap::IndexMap<String, String>,
    bind_ip: &str,
    branch_safe: &str,
    branch: &str,
) -> Vec<(String, String)> {
    env.iter()
        .map(|(k, v)| {
            let val = v.replace("{{bind_ip}}", bind_ip)
                .replace("{{branch_safe}}", branch_safe)
                .replace("{{branch}}", branch);
            (k.clone(), val)
        })
        .collect()
}

// ── Re-exports (only items used outside this module) ──

// git
pub use git::{list_branches, list_worktrees, create_worktree, create_worktree_from_base, remove_worktree};

// files
pub use files::{apply_env_overrides, write_env_file, ensure_global_gitignore};

// compose
pub use compose::{generate_compose_override, setup_main_as_worktree};

// shared services
pub use workspace::{
    generate_shared_compose, start_shared_services,
    create_shared_db, drop_shared_db,
    allocate_slot, release_slot,
};

// ip
pub use ip::{allocate_ip, release_ip, load_ip_allocations, check_etc_hosts};

// docker
pub use docker::{
    create_docker_network, remove_docker_network,
    ensure_workspace_folder, delete_workspace_folder,
};
