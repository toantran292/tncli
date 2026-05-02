use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::docker::{docker_force_cleanup, docker_project_name};
use super::files::copy_files;

/// List existing git worktrees for a repo.
pub fn list_worktrees(dir: &Path) -> Result<Vec<(String, String)>> {
    let output = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "worktree", "list", "--porcelain"])
        .output()?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut result = Vec::new();
    let mut current_path = String::new();
    let mut current_branch = String::new();

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = path.to_string();
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            current_branch = branch.to_string();
        } else if line.is_empty() && !current_path.is_empty() {
            if !current_branch.is_empty() {
                result.push((
                    std::mem::take(&mut current_path),
                    std::mem::take(&mut current_branch),
                ));
            }
            current_path.clear();
            current_branch.clear();
        }
    }
    if !current_path.is_empty() && !current_branch.is_empty() {
        result.push((current_path, current_branch));
    }

    Ok(result)
}

/// Check if a branch is already checked out in any worktree for this repo.
pub fn is_branch_in_worktree(dir: &Path, branch: &str) -> bool {
    list_worktrees(dir).unwrap_or_default().iter()
        .any(|(_, b)| b == branch)
}

/// Create a git worktree with a NEW branch from a base branch.
pub fn create_worktree_from_base(
    repo_dir: &Path,
    new_branch: &str,
    base_branch: &str,
    copy_files_list: &[String],
    workspace_dir: Option<&Path>,
) -> Result<PathBuf> {
    let repo_name = repo_dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());

    let worktree_dir = if let Some(ws_dir) = workspace_dir {
        ws_dir.join(&repo_name)
    } else {
        let dir_suffix = new_branch.replace('/', "-");
        let parent = repo_dir.parent().unwrap_or(Path::new("."));
        parent.join(format!("{repo_name}--{dir_suffix}"))
    };

    let project = docker_project_name(&worktree_dir);
    docker_force_cleanup(&project);

    if worktree_dir.exists() {
        bail!("worktree directory already exists: {}", worktree_dir.display());
    }

    // Check if branch already exists (local or remote)
    let branch_exists = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "rev-parse", "--verify", new_branch])
        .output()
        .is_ok_and(|o| o.status.success());

    let remote_exists = !branch_exists && Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "rev-parse", "--verify", &format!("origin/{new_branch}")])
        .output()
        .is_ok_and(|o| o.status.success());

    let output = if branch_exists {
        // Checkout existing local branch
        Command::new("git")
            .args([
                "-C", &repo_dir.to_string_lossy(),
                "worktree", "add",
                &worktree_dir.to_string_lossy(),
                new_branch,
            ])
            .output()?
    } else if remote_exists {
        // Checkout existing remote branch
        Command::new("git")
            .args([
                "-C", &repo_dir.to_string_lossy(),
                "worktree", "add",
                "--track", "-b", new_branch,
                &worktree_dir.to_string_lossy(),
                &format!("origin/{new_branch}"),
            ])
            .output()?
    } else {
        // Create new branch from base
        Command::new("git")
            .args([
                "-C", &repo_dir.to_string_lossy(),
                "worktree", "add",
                "-b", new_branch,
                &worktree_dir.to_string_lossy(),
                base_branch,
            ])
            .output()?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree add failed: {stderr}");
    }

    copy_files(repo_dir, &worktree_dir, copy_files_list);
    Ok(worktree_dir)
}

/// Create a git worktree checking out an EXISTING branch.
pub fn create_worktree(repo_dir: &Path, branch: &str, copy_files_list: &[String]) -> Result<PathBuf> {
    let dir_suffix = branch.replace('/', "-");
    let parent = repo_dir.parent().unwrap_or(Path::new("."));
    let repo_name = repo_dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());
    let worktree_dir = parent.join(format!("{repo_name}--{dir_suffix}"));

    if worktree_dir.exists() {
        bail!("worktree directory already exists: {}", worktree_dir.display());
    }

    let output = Command::new("git")
        .args([
            "-C", &repo_dir.to_string_lossy(),
            "worktree", "add",
            &worktree_dir.to_string_lossy(),
            branch,
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("already checked out") || stderr.contains("is already used") {
            let new_branch = format!("wt/{}", dir_suffix);
            let output2 = Command::new("git")
                .args([
                    "-C", &repo_dir.to_string_lossy(),
                    "worktree", "add",
                    &worktree_dir.to_string_lossy(),
                    &new_branch,
                ])
                .output()?;
            if !output2.status.success() {
                let output3 = Command::new("git")
                    .args([
                        "-C", &repo_dir.to_string_lossy(),
                        "worktree", "add",
                        "-b", &new_branch,
                        &worktree_dir.to_string_lossy(),
                        branch,
                    ])
                    .output()?;
                if !output3.status.success() {
                    let stderr3 = String::from_utf8_lossy(&output3.stderr);
                    bail!("git worktree add failed: {stderr3}");
                }
            }
        } else {
            bail!("git worktree add failed: {stderr}");
        }
    }

    copy_files(repo_dir, &worktree_dir, copy_files_list);
    Ok(worktree_dir)
}

/// Remove a git worktree and clean up its branch + leftover files.
pub fn remove_worktree(repo_dir: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    if worktree_path.exists() && worktree_path.join("docker-compose.yml").is_file() {
        let _ = Command::new("docker")
            .args(["compose", "down", "-v", "--remove-orphans", "--timeout", "5"])
            .current_dir(worktree_path)
            .output();
    }

    let project = docker_project_name(worktree_path);
    docker_force_cleanup(&project);

    let _ = Command::new("git")
        .args([
            "-C", &repo_dir.to_string_lossy(),
            "worktree", "remove", "--force",
            &worktree_path.to_string_lossy(),
        ])
        .output();

    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }

    let _ = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "worktree", "prune"])
        .output();

    let _ = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "branch", "-D", branch])
        .output();

    Ok(())
}

// ── Branch operations ──

pub fn current_branch(dir: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", dir, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

pub fn checkout(dir: &str, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["-C", dir, "checkout", branch])
        .output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

pub fn checkout_new_branch(dir: &str, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["-C", dir, "checkout", "-b", branch])
        .output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}
