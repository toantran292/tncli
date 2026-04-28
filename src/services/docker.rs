use std::path::{Path, PathBuf};
use std::process::Command;

/// Force cleanup all Docker containers + volumes + networks for a project name.
pub fn docker_force_cleanup(project_name: &str) {
    let filters = [
        format!("name={project_name}"),
        format!("label=com.docker.compose.project={project_name}"),
    ];
    for filter in &filters {
        if let Ok(output) = Command::new("docker")
            .args(["ps", "-aq", "--filter", filter])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let ids: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
            if !ids.is_empty() {
                let mut args = vec!["rm", "-f"];
                args.extend(ids);
                let _ = Command::new("docker").args(&args).output();
            }
        }
    }
    if let Ok(output) = Command::new("docker")
        .args(["volume", "ls", "-q", "--filter", &format!("name={project_name}")])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let ids: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
        if !ids.is_empty() {
            let mut args = vec!["volume", "rm", "-f"];
            args.extend(ids);
            let _ = Command::new("docker").args(&args).output();
        }
    }
    let _ = Command::new("docker").args(["network", "rm", &format!("{project_name}_default")]).output();
}

/// Get docker project name for a worktree path.
pub fn docker_project_name(worktree_path: &Path) -> String {
    let dir_name = worktree_path.file_name().unwrap_or_default().to_string_lossy();
    let parent_name = worktree_path.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if parent_name.starts_with("workspace--") {
        let ws = parent_name.trim_start_matches("workspace--");
        format!("{dir_name}-{ws}")
    } else {
        dir_name.into_owned()
    }
}

/// Create a Docker network for workspace cross-service communication.
pub fn create_docker_network(name: &str) {
    let _ = Command::new("docker")
        .args(["network", "create", name])
        .output();
}

/// Remove a Docker network.
pub fn remove_docker_network(name: &str) {
    let _ = Command::new("docker")
        .args(["network", "rm", name])
        .output();
}

/// Get workspace folder path.
pub fn workspace_folder_path(config_dir: &Path, name: &str) -> PathBuf {
    config_dir.join(format!("workspace--{name}"))
}

/// Ensure workspace folder exists.
pub fn ensure_workspace_folder(config_dir: &Path, name: &str) -> PathBuf {
    let ws_folder = workspace_folder_path(config_dir, name);
    let _ = std::fs::create_dir_all(&ws_folder);
    ws_folder
}

/// Delete workspace folder.
pub fn delete_workspace_folder(config_dir: &Path, name: &str) {
    let ws_folder = config_dir.join(format!("workspace--{name}"));
    if ws_folder.exists() {
        let _ = std::fs::remove_dir_all(&ws_folder);
    }
}

/// Ensure main workspace folder exists with repos moved into it.
/// Idempotent — skips repos already in workspace folder.
pub fn ensure_main_workspace(config_dir: &Path, config: &crate::config::Config) -> PathBuf {
    let branch = config.global_default_branch();
    let ws_dir = config_dir.join(format!("workspace--{branch}"));
    let _ = std::fs::create_dir_all(&ws_dir);

    for dir_name in config.repos.keys() {
        let p = Path::new(dir_name);
        // Skip absolute paths — only migrate relative dirs
        if p.is_absolute() {
            continue;
        }
        let src = config_dir.join(dir_name);
        let dst = ws_dir.join(dir_name);
        if src.exists() && src.is_dir() && !dst.exists() {
            if std::fs::rename(&src, &dst).is_ok() {
                fix_worktree_refs_after_move(&dst);
            }
        }
    }

    ws_dir
}

/// After moving a git repo, fix worktree `.git` files that reference the old location.
/// Each worktree has a `.git` file containing `gitdir: /old/path/.git/worktrees/{name}`.
/// We update it to point to the new repo location.
fn fix_worktree_refs_after_move(new_repo_dir: &Path) {
    let git_dir = new_repo_dir.join(".git");
    // Only handle standard git repos (not worktrees themselves)
    if !git_dir.is_dir() {
        return;
    }
    let wt_dir = git_dir.join("worktrees");
    if !wt_dir.is_dir() {
        return;
    }

    let entries = match std::fs::read_dir(&wt_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let gitdir_file = entry.path().join("gitdir");
        if !gitdir_file.is_file() {
            continue;
        }
        // gitdir file contains the path to the worktree's .git file
        let wt_git_path = match std::fs::read_to_string(&gitdir_file) {
            Ok(s) => s.trim().to_string(),
            Err(_) => continue,
        };
        let wt_git_file = Path::new(&wt_git_path);
        if !wt_git_file.is_file() {
            continue;
        }
        // Read the worktree's .git file: `gitdir: /old/path/.git/worktrees/{name}`
        let content = match std::fs::read_to_string(wt_git_file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let prefix = "gitdir: ";
        if !content.starts_with(prefix) {
            continue;
        }
        let old_gitdir = content[prefix.len()..].trim();
        // old_gitdir = /old/path/.git/worktrees/{name}
        // We need to replace with /new/path/.git/worktrees/{name}
        // Extract the worktree name from the old path
        let wt_name = match Path::new(old_gitdir).file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };
        let new_gitdir = git_dir.join("worktrees").join(&wt_name);
        let new_content = format!("gitdir: {}\n", new_gitdir.display());
        let _ = std::fs::write(wt_git_file, new_content);
    }
}
