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
