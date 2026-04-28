use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Info about a single worktree instance.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub branch: String,
    pub parent_dir: String,
    pub bind_ip: String,
    pub path: PathBuf,
}

/// List remote + local branches for a git repo.
pub fn list_branches(dir: &Path) -> Result<Vec<String>> {
    // Skip fetch — too slow for 600+ branches. User can git fetch manually.
    let output = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "branch", "-a", "--format=%(refname:short)"])
        .output()?;

    if !output.status.success() {
        bail!("git branch failed in {}", dir.display());
    }

    let branches: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.contains("HEAD"))
        .map(|l| l.to_string())
        .collect();

    Ok(branches)
}

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
    // Last entry
    if !current_path.is_empty() && !current_branch.is_empty() {
        result.push((current_path, current_branch));
    }

    Ok(result)
}

/// Create a git worktree with a NEW branch from a base branch.
/// `git worktree add -b new_branch path base_branch`
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

    // If workspace_dir provided, create inside it. Otherwise sibling to repo.
    let worktree_dir = if let Some(ws_dir) = workspace_dir {
        ws_dir.join(&repo_name)
    } else {
        let dir_suffix = new_branch.replace('/', "-");
        let parent = repo_dir.parent().unwrap_or(Path::new("."));
        parent.join(format!("{repo_name}--{dir_suffix}"))
    };

    // Cleanup any leftover docker containers from previous worktree with same name
    let project = docker_project_name(&worktree_dir);
    docker_force_cleanup(&project);

    if worktree_dir.exists() {
        bail!("worktree directory already exists: {}", worktree_dir.display());
    }

    // Delete leftover branch if exists (from previous worktree)
    let _ = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "branch", "-D", new_branch])
        .output();

    let output = Command::new("git")
        .args([
            "-C", &repo_dir.to_string_lossy(),
            "worktree", "add",
            "-b", new_branch,
            &worktree_dir.to_string_lossy(),
            base_branch,
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree add failed: {stderr}");
    }

    copy_files(repo_dir, &worktree_dir, copy_files_list);
    Ok(worktree_dir)
}

/// Create a git worktree checking out an EXISTING branch.
/// Returns the path to the new worktree directory.
pub fn create_worktree(repo_dir: &Path, branch: &str, copy_files_list: &[String]) -> Result<PathBuf> {
    // Sanitize branch name for directory: replace / with -
    let dir_suffix = branch.replace('/', "-");
    let parent = repo_dir.parent().unwrap_or(Path::new("."));
    let repo_name = repo_dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());
    let worktree_dir = parent.join(format!("{repo_name}--{dir_suffix}"));

    if worktree_dir.exists() {
        bail!("worktree directory already exists: {}", worktree_dir.display());
    }

    // Try direct checkout first
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
        // If branch is already checked out, try wt/ branch
        if stderr.contains("already checked out") || stderr.contains("is already used") {
            let new_branch = format!("wt/{}", dir_suffix);
            // Try using existing wt/ branch first
            let output2 = Command::new("git")
                .args([
                    "-C", &repo_dir.to_string_lossy(),
                    "worktree", "add",
                    &worktree_dir.to_string_lossy(),
                    &new_branch,
                ])
                .output()?;
            if !output2.status.success() {
                // Branch doesn't exist yet, create it
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

    // Copy user-configured files from parent to worktree
    copy_files(repo_dir, &worktree_dir, copy_files_list);

    Ok(worktree_dir)
}

/// Copy files from parent repo to worktree based on user-configured list.
/// Supports simple glob: `apps/*/.env` matches `apps/portal/.env`, `apps/crm/.env`, etc.
pub fn copy_files(repo_dir: &Path, worktree_dir: &Path, patterns: &[String]) {
    for pattern in patterns {
        if pattern.contains('*') {
            // Glob: find matching files in repo_dir
            let full_pattern = format!("{}/{}", repo_dir.display(), pattern);
            if let Ok(paths) = glob_simple(&full_pattern, repo_dir) {
                for rel_path in &paths {
                    let src = repo_dir.join(rel_path);
                    let dest = worktree_dir.join(rel_path);
                    if src.is_file() && !dest.exists() {
                        if let Some(parent) = dest.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let _ = std::fs::copy(&src, &dest);
                    }
                }
            }
        } else {
            // Exact path
            let src = repo_dir.join(pattern);
            let dest = worktree_dir.join(pattern);
            if src.is_file() && !dest.exists() {
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::copy(&src, &dest);
            }
        }
    }
}

/// Apply env overrides by writing env file (e.g. .env.local) in each dir that has .env files.
/// Scans existing .env* files to find which keys exist, then writes matching overrides.
pub fn apply_env_overrides(worktree_dir: &Path, env_overrides: &[(String, String)], env_file_name: &str) {
    if env_overrides.is_empty() {
        return;
    }

    // Collect dirs containing .env files (top-level + apps/*)
    let mut env_dirs: Vec<PathBuf> = Vec::new();

    // Top-level
    if worktree_dir.join(".env").is_file()
        || worktree_dir.join(".env.development").is_file()
    {
        env_dirs.push(worktree_dir.to_path_buf());
    }

    // Nested apps/*/.env
    if let Ok(entries) = std::fs::read_dir(worktree_dir.join("apps")) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                let app_dir = entry.path();
                if app_dir.join(".env").is_file() {
                    env_dirs.push(app_dir);
                }
            }
        }
    }

    for dir in &env_dirs {
        // Read all .env* files in this dir to find existing keys
        let mut all_keys = HashSet::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with(".env") && name_str != env_file_name
                    && entry.file_type().is_ok_and(|t| t.is_file())
                {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        for line in content.lines() {
                            let line = line.trim();
                            if !line.starts_with('#') && !line.is_empty() {
                                if let Some(key) = line.split('=').next() {
                                    all_keys.insert(key.trim().to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        // Write .env.local with matching overrides (keep *.local hostnames —
        // Docker resolves via extra_hosts, host resolves via /etc/hosts)
        let mut lines: Vec<String> = vec!["# Auto-generated by tncli. Do not commit.".into()];
        for (key, value) in env_overrides {
            if all_keys.contains(key.as_str()) {
                lines.push(format!("{key}={value}"));
            }
        }

        if lines.len() > 1 {
            let local_path = dir.join(env_file_name);
            let _ = std::fs::write(&local_path, lines.join("\n") + "\n");
        }
    }
}

/// Run setup commands in worktree dir (foreground, prints output).
pub fn run_setup_foreground(worktree_dir: &Path, commands: &[String]) {
    if commands.is_empty() {
        return;
    }
    let dir_name = worktree_dir.file_name().unwrap_or_default().to_string_lossy();
    for cmd in commands {
        eprintln!("  [setup] {dir_name}: {cmd}");
        let _ = std::process::Command::new("zsh")
            .args(["-ic", cmd])
            .current_dir(worktree_dir)
            .status();
    }
}

/// Simple glob using `find` command (avoids adding glob crate).
fn glob_simple(pattern: &str, repo_dir: &Path) -> Result<Vec<String>> {
    // Convert glob to find: "apps/*/.env" → find apps -name ".env"
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() != 2 {
        return Ok(Vec::new());
    }
    // Use shell glob expansion
    let output = Command::new("bash")
        .args(["-c", &format!("cd '{}' && ls {} 2>/dev/null", repo_dir.display(), pattern.replace(&format!("{}/", repo_dir.display()), ""))])
        .output()?;
    let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    Ok(files)
}

/// Parse ports from docker-compose files and generate override with BIND_IP.
pub fn generate_compose_override(
    repo_dir: &Path,
    worktree_dir: &Path,
    bind_ip: &str,
    compose_files: &[String],
    worktree_env: &indexmap::IndexMap<String, String>,
    branch: &str,
    network_name: Option<&str>,
    service_overrides: Option<&indexmap::IndexMap<String, crate::config::ServiceOverride>>,
    shared_hosts: &[String],
) {
    // Determine which compose files to parse
    let files_to_parse: Vec<std::path::PathBuf> = if compose_files.is_empty() {
        // Default: docker-compose.yml
        let default = repo_dir.join("docker-compose.yml");
        if default.is_file() { vec![default] } else { return; }
    } else {
        compose_files.iter()
            .map(|f| repo_dir.join(f))
            .filter(|p| p.is_file())
            .collect()
    };

    if files_to_parse.is_empty() {
        return;
    }

    // Parse compose files using serde_yaml for correct service detection
    let mut service_ports: indexmap::IndexMap<String, Vec<String>> = indexmap::IndexMap::new();
    let mut hardcoded_container_names: indexmap::IndexMap<String, String> = indexmap::IndexMap::new();
    let mut service_depends: indexmap::IndexMap<String, Vec<String>> = indexmap::IndexMap::new();
    let mut all_service_names: HashSet<String> = HashSet::new();

    for f in &files_to_parse {
        let content = match std::fs::read_to_string(f) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let yaml: serde_yaml::Value = match serde_yaml::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(services) = yaml.get("services").and_then(|s| s.as_mapping()) {
            for (name, svc) in services {
                let name = match name.as_str() {
                    Some(n) => n,
                    None => continue,
                };
                all_service_names.insert(name.to_string());
                // Detect hardcoded container_name
                if let Some(cn) = svc.get("container_name").and_then(|c| c.as_str()) {
                    hardcoded_container_names.insert(name.to_string(), cn.to_string());
                }
                // Detect depends_on
                if let Some(deps) = svc.get("depends_on") {
                    let dep_list: Vec<String> = if let Some(seq) = deps.as_sequence() {
                        seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
                    } else if let Some(map) = deps.as_mapping() {
                        map.keys().filter_map(|k| k.as_str().map(|s| s.to_string())).collect()
                    } else {
                        Vec::new()
                    };
                    if !dep_list.is_empty() {
                        service_depends.insert(name.to_string(), dep_list);
                    }
                }
                if let Some(ports) = svc.get("ports").and_then(|p| p.as_sequence()) {
                    for port in ports {
                        let port_str = match port.as_str() {
                            Some(s) => s.to_string(),
                            None => match port.as_i64() {
                                Some(n) => n.to_string(),
                                None => continue,
                            },
                        };
                        let parts: Vec<&str> = port_str.split(':').collect();
                        let new_port = match parts.len() {
                            2 => format!("{bind_ip}:{port_str}"),
                            3 => format!("{bind_ip}:{}:{}", parts[1], parts[2]),
                            _ => continue,
                        };
                        service_ports.entry(name.to_string()).or_default().push(new_port);
                    }
                }
            }
        }
    }

    if service_ports.is_empty() && worktree_env.is_empty() {
        return;
    }

    // Resolve template vars in env values
    let branch_safe = branch.replace('/', "_").replace('-', "_");
    let resolved_env: Vec<(String, String)> = worktree_env.iter()
        .map(|(k, v)| {
            let val = v.replace("{{branch_safe}}", &branch_safe)
                .replace("{{bind_ip}}", bind_ip)
                .replace("{{branch}}", branch);
            (k.clone(), val)
        })
        .collect();

    // Generate docker-compose.override.yml
    let mut output = String::with_capacity(4096);
    output.push_str("# Auto-generated by tncli for worktree. Do not commit.\n");

    // Unique project name to avoid conflict with main
    let dir_name = worktree_dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent_name = worktree_dir.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let project_name = if parent_name.starts_with("workspace--") {
        // Inside workspace folder: use "dirname-branchname"
        let ws_name = parent_name.trim_start_matches("workspace--");
        format!("{dir_name}-{ws_name}")
    } else {
        dir_name.clone()
    };
    let _ = write!(output, "name: {project_name}\n");

    // Network definition (if workspace)
    if let Some(net) = network_name {
        let _ = write!(output, "\nnetworks:\n  {}:\n    external: true\n", net);
    }

    output.push_str("\nservices:\n");

    let merged_overrides: indexmap::IndexMap<String, crate::config::ServiceOverride> =
        service_overrides.cloned().unwrap_or_default();
    let service_overrides = Some(&merged_overrides);

    // Collect service override names (may include services not in compose files)
    let svc_override_names: Vec<String> = service_overrides
        .map(|so| so.keys().cloned().collect())
        .unwrap_or_default();

    // Merge all service names (compose + overrides) preserving order
    let mut all_svc_merged: Vec<String> = all_service_names.iter().cloned().collect();
    for name in &svc_override_names {
        if !all_service_names.contains(name) {
            all_svc_merged.push(name.clone());
        }
    }

    // Collect services disabled by profiles (need to remove from depends_on)
    let disabled_svcs: HashSet<String> = service_overrides
        .map(|so| so.iter()
            .filter(|(_, ov)| !ov.profiles.is_empty())
            .map(|(name, _)| name.clone())
            .collect())
        .unwrap_or_default();

    for svc in &all_svc_merged {
        let has_ports = service_ports.contains_key(svc);
        let needs_env = !resolved_env.is_empty();
        let needs_network = network_name.is_some();
        let has_svc_override = service_overrides
            .and_then(|so| so.get(svc))
            .is_some();

        let has_hardcoded_name = hardcoded_container_names.contains_key(svc);

        // Check if this service depends on a disabled service (explicit or via YAML anchor)
        let needs_depends_override = if !disabled_svcs.is_empty() {
            // If there are disabled services, any non-disabled service might depend on them
            // via YAML anchors (<<: *app) which serde_yaml doesn't resolve for Value parsing.
            // Safe to always override depends_on for non-disabled services.
            !disabled_svcs.contains(svc)
        } else {
            false
        };

        if !has_ports && !needs_env && !needs_network && !has_svc_override && !has_hardcoded_name && !needs_depends_override {
            continue;
        }

        let _ = write!(output, "  {svc}:\n");

        // Override hardcoded container_name to avoid conflicts with main
        if hardcoded_container_names.contains_key(svc) {
            let new_name = format!("{project_name}-{svc}");
            let _ = write!(output, "    container_name: {new_name}\n");
        }

        // Override depends_on to remove disabled services
        if needs_depends_override {
            // Filter out disabled services from known depends_on;
            // for services with anchor-inherited deps, clear all deps
            if let Some(deps) = service_depends.get(svc) {
                let filtered: Vec<&String> = deps.iter()
                    .filter(|d| !disabled_svcs.contains(*d))
                    .collect();
                if filtered.is_empty() {
                    output.push_str("    depends_on: !override []\n");
                } else {
                    output.push_str("    depends_on: !override\n");
                    for d in &filtered {
                        let _ = write!(output, "      - {d}\n");
                    }
                }
            } else {
                // No explicit depends_on found (likely inherited via YAML anchor) — clear all
                output.push_str("    depends_on: !override []\n");
            }
        }

        // Service-specific overrides (profiles, mem_limit, environment)
        if let Some(svc_ov) = service_overrides.and_then(|so| so.get(svc)) {
            if !svc_ov.profiles.is_empty() {
                output.push_str("    profiles:\n");
                for p in &svc_ov.profiles {
                    let _ = write!(output, "      - \"{p}\"\n");
                }
            }
            if let Some(mem) = &svc_ov.mem_limit {
                let _ = write!(output, "    mem_limit: {mem}\n");
            }
        }

        // Port override
        if let Some(ports) = service_ports.get(svc) {
            output.push_str("    ports: !override\n");
            for port in ports {
                let _ = write!(output, "      - \"{port}\"\n");
            }
        }

        // Env override (worktree_env + service_override env merged)
        let mut env_map: Vec<(String, String)> = resolved_env.clone();
        if let Some(svc_ov) = service_overrides.and_then(|so| so.get(svc)) {
            for (k, v) in &svc_ov.environment {
                // Service override env wins over worktree_env
                if let Some(existing) = env_map.iter_mut().find(|(ek, _)| ek == k) {
                    existing.1 = v.clone();
                } else {
                    env_map.push((k.clone(), v.clone()));
                }
            }
        }
        if !env_map.is_empty() {
            output.push_str("    environment:\n");
            for (k, v) in &env_map {
                let _ = write!(output, "      {k}: \"{v}\"\n");
            }
        }

        // Extra hosts: add shared service hostnames → host-gateway
        if !shared_hosts.is_empty() {
            output.push_str("    extra_hosts:\n");
            for host in shared_hosts {
                let _ = write!(output, "      - \"{}:host-gateway\"\n", host);
            }
        }

        // Network
        if let Some(net) = network_name {
            let _ = write!(output, "    networks:\n      - default\n      - {}\n", net);
        }
    }

    let override_path = worktree_dir.join("docker-compose.override.yml");
    let _ = std::fs::write(&override_path, &output);

    // If dip.yml exists, create dip.override.yml to include the compose override
    let dip_path = worktree_dir.join("dip.yml");
    if dip_path.is_file() {
        let dip_override = worktree_dir.join("dip.override.yml");
        let _ = std::fs::write(dip_override,
            "version: '6.1'\n\ncompose:\n  files:\n    - docker-compose.yml\n    - docker-compose.override.yml\n"
        );
    }
}

/// Generate docker-compose.shared.yml from top-level shared_services config.
/// Returns path to the generated file.
pub fn generate_shared_compose(
    config_dir: &Path,
    session: &str,
    shared_services: &indexmap::IndexMap<String, crate::config::SharedServiceDef>,
) -> PathBuf {
    let mut output = String::with_capacity(4096);
    output.push_str("# Auto-generated by tncli. Do not edit.\n");
    let _ = write!(output, "name: {session}-shared\n\n");

    // Collect volume names
    let mut volume_names: Vec<String> = Vec::new();
    let mut volume_set: HashSet<String> = HashSet::new();

    output.push_str("services:\n");
    for (name, svc) in shared_services {
        // Determine how many instances needed (auto-scaled)
        let instances = if svc.capacity.is_some() { max_instance_count(name) } else { 1 };

        for inst in 0..instances {
            // Instance naming: redis (0), redis-2 (1), redis-3 (2)...
            let svc_name = if inst == 0 { name.clone() } else { format!("{name}-{}", inst + 1) };
            let _ = write!(output, "  {svc_name}:\n");
            let _ = write!(output, "    image: {}\n", svc.image);

            if let Some(cmd) = &svc.command {
                let _ = write!(output, "    command: {cmd}\n");
            }

            if !svc.ports.is_empty() {
                output.push_str("    ports:\n");
                for port in &svc.ports {
                    // Increment host port for scaled instances: 17007→17008→17009
                    let adjusted = if inst > 0 {
                        let parts: Vec<&str> = port.split(':').collect();
                        if parts.len() == 2 {
                            if let Ok(host_port) = parts[0].parse::<u16>() {
                                format!("{}:{}", host_port + inst as u16, parts[1])
                            } else { port.clone() }
                        } else { port.clone() }
                    } else { port.clone() };
                    let _ = write!(output, "      - \"{adjusted}\"\n");
                }
            }

            if !svc.environment.is_empty() {
                output.push_str("    environment:\n");
                for (k, v) in &svc.environment {
                    let _ = write!(output, "      {k}: \"{v}\"\n");
                }
            }

            if !svc.volumes.is_empty() {
                output.push_str("    volumes:\n");
                for vol in &svc.volumes {
                    // Each instance gets its own volume
                    let adjusted_vol = if inst > 0 {
                        let parts: Vec<&str> = vol.splitn(2, ':').collect();
                        if parts.len() == 2 && !parts[0].starts_with('.') && !parts[0].starts_with('/') {
                            let vol_name = format!("{}-{}", parts[0], inst + 1);
                            format!("{vol_name}:{}", parts[1])
                        } else { vol.clone() }
                    } else { vol.clone() };
                    let _ = write!(output, "      - {adjusted_vol}\n");
                    if let Some(vol_name) = adjusted_vol.split(':').next() {
                        if !vol_name.starts_with('.') && !vol_name.starts_with('/') && volume_set.insert(vol_name.to_string()) {
                            volume_names.push(vol_name.to_string());
                        }
                    }
                }
            }

            if let Some(hc) = &svc.healthcheck {
                output.push_str("    healthcheck:\n");
                match &hc.test {
                    serde_yaml::Value::String(s) => {
                        let _ = write!(output, "      test: {s}\n");
                    }
                    other => {
                        if let Ok(yaml) = serde_yaml::to_string(other) {
                            let _ = write!(output, "      test: {}", yaml.trim_start_matches("---\n"));
                        }
                    }
                }
                let _ = write!(output, "      interval: {}\n", hc.interval);
                let _ = write!(output, "      timeout: {}\n", hc.timeout);
                let _ = write!(output, "      retries: {}\n", hc.retries);
            }

            output.push_str("    restart: unless-stopped\n");
            output.push('\n');
        }
    }

    // Volume definitions
    if !volume_names.is_empty() {
        output.push_str("volumes:\n");
        for vol in &volume_names {
            let _ = write!(output, "  {vol}:\n    driver: local\n");
        }
    }

    let path = config_dir.join("docker-compose.shared.yml");
    let _ = std::fs::write(&path, &output);
    path
}

/// Start shared services from the generated compose file.
pub fn start_shared_services(
    config_dir: &Path,
    session: &str,
    service_names: &[&str],
) {
    let compose_file = config_dir.join("docker-compose.shared.yml");
    if !compose_file.is_file() {
        return;
    }
    let _ = Command::new("docker")
        .args(["compose", "-f", &compose_file.to_string_lossy(), "-p", &format!("{session}-shared"), "up", "-d"])
        .args(service_names)
        .current_dir(config_dir)
        .output();
}

/// Get workspace folder path. Worktrees are created directly inside this folder.
pub fn workspace_folder_path(config_dir: &Path, name: &str) -> PathBuf {
    config_dir.join(format!("workspace--{name}"))
}

/// Ensure workspace folder exists.
pub fn ensure_workspace_folder(config_dir: &Path, name: &str) -> PathBuf {
    let ws_folder = workspace_folder_path(config_dir, name);
    let _ = std::fs::create_dir_all(&ws_folder);
    ws_folder
}

/// Delete workspace folder (symlinks only, not worktree dirs).
pub fn delete_workspace_folder(config_dir: &Path, name: &str) {
    let ws_folder = config_dir.join(format!("workspace--{name}"));
    if ws_folder.exists() {
        let _ = std::fs::remove_dir_all(&ws_folder);
    }
}

/// Setup main dir as a worktree-like environment with 127.0.0.1 binding.
/// Generates compose override with env, shared hosts, and branch-based namespacing.
pub fn setup_main_as_worktree(
    repo_dir: &Path,
    compose_files: &[String],
    worktree_env: &indexmap::IndexMap<String, String>,
    branch: &str,
    service_overrides: Option<&indexmap::IndexMap<String, crate::config::ServiceOverride>>,
    shared_hosts: &[String],
) {
    generate_compose_override(repo_dir, repo_dir, "127.0.0.1", compose_files, worktree_env, branch, None, service_overrides, shared_hosts);
}


/// Create database for worktree on a shared postgres instance.
/// Returns a status message (does not print — caller decides output).
pub fn create_shared_db(
    host: &str,
    port: u16,
    db_name: &str,
    user: &str,
    password: &str,
) -> String {
    let extra_host = format!("--add-host={host}:host-gateway");
    let conn_url = format!("postgresql://{user}:{password}@{host}:{port}/postgres");
    let create_sql = format!("CREATE DATABASE \"{db_name}\"");

    let output = Command::new("docker")
        .args([
            "run", "--rm",
            &extra_host,
            "postgres:16-alpine",
            "psql", &conn_url,
            "-c", &create_sql,
        ])
        .output();

    match output {
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if o.status.success() {
                format!("created db: {db_name}")
            } else if stderr.contains("already exists") {
                format!("db exists: {db_name}")
            } else {
                format!("warning: {db_name}: {}", stderr.trim())
            }
        }
        Err(e) => format!("error creating DB: {e}"),
    }
}

/// Drop database on shared postgres instance.
pub fn drop_shared_db(host: &str, port: u16, db_name: &str, user: &str, password: &str) {
    let extra_host = format!("--add-host={host}:host-gateway");
    let conn_url = format!("postgresql://{user}:{password}@{host}:{port}/postgres");
    let drop_sql = format!("DROP DATABASE IF EXISTS \"{db_name}\"");

    let _ = Command::new("docker")
        .args([
            "run", "--rm",
            &extra_host,
            "postgres:16-alpine",
            "psql", &conn_url,
            "-c", &drop_sql,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Ensure /etc/hosts has required local hostnames (e.g. minio.local).
/// Returns list of hostnames that need to be added (requires sudo).
// ── Shared Service Slot Allocation ──

const SLOT_STATE_FILE: &str = ".tncli/shared_slots.json";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SlotAllocation {
    pub instance: usize,
    pub slot: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ServiceSlots {
    pub slots: HashMap<String, SlotAllocation>,
    pub instance_count: usize,
}

fn slot_state_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(SLOT_STATE_FILE)
}

pub fn load_slot_allocations() -> HashMap<String, ServiceSlots> {
    let path = slot_state_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_slot_allocations(allocs: &HashMap<String, ServiceSlots>) {
    let path = slot_state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(allocs) {
        let _ = std::fs::write(&path, json);
    }
}

/// Allocate a slot on a shared service instance.
/// Returns (instance_index, slot_index, port).
/// If capacity is exceeded, increments instance_count for auto-scaling.
pub fn allocate_slot(
    service_name: &str,
    worktree_key: &str,
    capacity: u16,
    base_port: u16,
) -> (usize, usize, u16) {
    let mut allocs = load_slot_allocations();
    let svc = allocs.entry(service_name.to_string()).or_insert_with(|| ServiceSlots {
        slots: HashMap::new(),
        instance_count: 1,
    });

    // Already allocated?
    if let Some(existing) = svc.slots.get(worktree_key) {
        let port = base_port + existing.instance as u16;
        return (existing.instance, existing.slot, port);
    }

    // Find instance with free slot
    let cap = capacity as usize;
    for inst in 0..svc.instance_count {
        let used_in_inst = svc.slots.values().filter(|a| a.instance == inst).count();
        if used_in_inst < cap {
            // Find first free slot index in this instance
            let used_slots: HashSet<usize> = svc.slots.values()
                .filter(|a| a.instance == inst)
                .map(|a| a.slot)
                .collect();
            let slot = (0..cap).find(|s| !used_slots.contains(s)).unwrap_or(0);
            svc.slots.insert(worktree_key.to_string(), SlotAllocation { instance: inst, slot });
            save_slot_allocations(&allocs);
            let port = base_port + inst as u16;
            return (inst, slot, port);
        }
    }

    // All instances full → scale up
    let new_inst = svc.instance_count;
    svc.instance_count += 1;
    svc.slots.insert(worktree_key.to_string(), SlotAllocation { instance: new_inst, slot: 0 });
    save_slot_allocations(&allocs);
    let port = base_port + new_inst as u16;
    (new_inst, 0, port)
}

/// Release slot when worktree is deleted.
pub fn release_slot(service_name: &str, worktree_key: &str) {
    let mut allocs = load_slot_allocations();
    if let Some(svc) = allocs.get_mut(service_name) {
        svc.slots.remove(worktree_key);
        // Shrink instance_count if last instance is now empty
        while svc.instance_count > 1 {
            let last = svc.instance_count - 1;
            if svc.slots.values().any(|a| a.instance == last) {
                break;
            }
            svc.instance_count -= 1;
        }
    }
    save_slot_allocations(&allocs);
}

/// Get the maximum instance_count across all services (for compose generation).
pub fn max_instance_count(service_name: &str) -> usize {
    load_slot_allocations()
        .get(service_name)
        .map(|s| s.instance_count)
        .unwrap_or(1)
}

pub fn check_etc_hosts(hostnames: &[&str]) -> Vec<String> {
    let content = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
    hostnames.iter()
        .filter(|h| !content.contains(*h))
        .map(|h| h.to_string())
        .collect()
}

/// Add hostnames to /etc/hosts pointing to 127.0.0.1 (CLI — uses sudo).
pub fn setup_etc_hosts(hostnames: &[String]) -> Result<()> {
    if hostnames.is_empty() {
        return Ok(());
    }
    let entries: Vec<String> = hostnames.iter()
        .map(|h| format!("127.0.0.1 {h}"))
        .collect();
    let cmd = format!("echo '{}' >> /etc/hosts", entries.join("\n"));
    let status = Command::new("sudo")
        .args(["sh", "-c", &cmd])
        .status()?;
    if !status.success() {
        bail!("failed to update /etc/hosts (sudo required)");
    }
    Ok(())
}

/// Create a Docker network for workspace cross-service communication.
#[allow(dead_code)]
pub fn create_docker_network(name: &str) {
    let _ = Command::new("docker")
        .args(["network", "create", name])
        .output();
}

/// Remove a Docker network.
#[allow(dead_code)]
pub fn remove_docker_network(name: &str) {
    let _ = Command::new("docker")
        .args(["network", "rm", name])
        .output();
}

/// Ensure docker-compose.override.yml is in global gitignore.
pub fn ensure_global_gitignore() {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return,
    };
    let gitignore_path = format!("{home}/.gitignore_global");

    // Check if already configured
    let excludes = Command::new("git")
        .args(["config", "--global", "core.excludesfile"])
        .output()
        .ok()
        .and_then(|o| if o.status.success() { String::from_utf8(o.stdout).ok() } else { None })
        .unwrap_or_default()
        .trim()
        .to_string();

    // Set global excludes file if not set
    if excludes.is_empty() {
        let _ = Command::new("git")
            .args(["config", "--global", "core.excludesfile", &gitignore_path])
            .output();
    }

    let target = if excludes.is_empty() { &gitignore_path } else { &excludes };

    // Read existing content
    let content = std::fs::read_to_string(target).unwrap_or_default();

    // Add all tncli-generated files
    let tncli_files = [
        "docker-compose.override.yml",
        "docker-compose.shared.yml",
        "dip.override.yml",
        ".env.tncli",
        ".env.local",
        ".env.*.local",
    ];
    let mut to_add = Vec::new();
    for file in &tncli_files {
        if !content.contains(file) {
            to_add.push(*file);
        }
    }
    if !to_add.is_empty() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(target) {
            if !content.contains("# tncli") {
                let _ = writeln!(f, "\n# tncli worktree");
            }
            for file in to_add {
                let _ = writeln!(f, "{file}");
            }
        }
    }
}

/// Force cleanup all Docker containers + volumes + networks for a project name.
pub fn docker_force_cleanup(project_name: &str) {
    // Kill + remove containers by name pattern AND label (covers both cases)
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
    // Remove volumes
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
    // Remove networks
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

/// Remove a git worktree and clean up its branch + leftover files.
pub fn remove_worktree(repo_dir: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    // Step 1: graceful docker down from worktree dir (uses correct project name from override)
    if worktree_path.exists() && worktree_path.join("docker-compose.yml").is_file() {
        let _ = Command::new("docker")
            .args(["compose", "down", "-v", "--remove-orphans", "--timeout", "5"])
            .current_dir(worktree_path)
            .output();
    }

    // Step 2: force cleanup any remaining containers/volumes/networks
    let project = docker_project_name(worktree_path);
    docker_force_cleanup(&project);

    // Try git worktree remove (may fail if untracked files)
    let _ = Command::new("git")
        .args([
            "-C", &repo_dir.to_string_lossy(),
            "worktree", "remove", "--force",
            &worktree_path.to_string_lossy(),
        ])
        .output();

    // Force remove directory if still exists (untracked files left behind)
    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }

    // Prune stale worktree refs
    let _ = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "worktree", "prune"])
        .output();

    // Delete the branch
    let _ = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "branch", "-D", branch])
        .output();

    Ok(())
}

// ── Loopback Aliasing ──

const LOOPBACK_STATE_FILE: &str = ".tncli/loopback.json";

fn state_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(LOOPBACK_STATE_FILE)
}

/// Load IP allocations from disk.
pub fn load_ip_allocations() -> HashMap<String, String> {
    let path = state_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Save IP allocations to disk.
fn save_ip_allocations(allocs: &HashMap<String, String>) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(allocs) {
        let _ = std::fs::write(&path, json);
    }
}

/// Allocate next available loopback IP (127.0.0.2, 127.0.0.3, ...).
pub fn allocate_ip(worktree_key: &str) -> String {
    let mut allocs = load_ip_allocations();

    // Already allocated?
    if let Some(ip) = allocs.get(worktree_key) {
        return ip.clone();
    }

    // Find next available
    let used: HashSet<&str> = allocs.values().map(|s| s.as_str()).collect();
    let mut n = 2u8;
    loop {
        let ip = format!("127.0.0.{n}");
        if !used.contains(ip.as_str()) {
            allocs.insert(worktree_key.to_string(), ip.clone());
            save_ip_allocations(&allocs);
            return ip;
        }
        n += 1;
        if n == 255 {
            return "127.0.0.254".to_string(); // fallback
        }
    }
}

/// Release an allocated IP.
pub fn release_ip(worktree_key: &str) {
    let mut allocs = load_ip_allocations();
    allocs.remove(worktree_key);
    save_ip_allocations(&allocs);
}

/// Create loopback alias (requires sudo). Call manually, not from TUI.
#[allow(dead_code)]
pub fn setup_loopback(ip: &str) -> Result<()> {
    let status = Command::new("sudo")
        .args(["ifconfig", "lo0", "alias", ip])
        .status()?;
    if !status.success() {
        bail!("failed to create loopback alias {ip} (sudo required)");
    }
    Ok(())
}

/// Remove loopback alias.
#[allow(dead_code)]
pub fn teardown_loopback(ip: &str) -> Result<()> {
    let _ = Command::new("sudo")
        .args(["ifconfig", "lo0", "-alias", ip])
        .status();
    Ok(())
}

/// Write .env.tncli file in worktree dir with BIND_IP.
pub fn write_env_file(worktree_path: &Path, ip: &str) -> Result<()> {
    let env_path = worktree_path.join(".env.tncli");
    std::fs::write(&env_path, format!("BIND_IP={ip}\n"))?;
    Ok(())
}
