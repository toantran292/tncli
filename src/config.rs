use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_session")]
    pub session: String,
    #[serde(default, alias = "dirs")]
    pub repos: IndexMap<String, Dir>,
    /// Top-level shared service definitions (docker-compose-like).
    #[serde(default)]
    pub shared_services: IndexMap<String, SharedServiceDef>,
    /// Workspaces (groups of services). Alias: "combinations" for backward compat.
    #[serde(default, deserialize_with = "deserialize_workspace_entries")]
    pub workspaces: IndexMap<String, Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_workspace_entries")]
    pub combinations: IndexMap<String, Vec<String>>,
}


fn default_session() -> String {
    "tncli".into()
}

#[derive(Debug, Deserialize, Clone)]
pub struct Dir {
    pub alias: Option<String>,
    pub pre_start: Option<String>,
    pub env: Option<String>,
    #[serde(default)]
    pub worktree: bool,
    #[serde(default)]
    pub worktree_copy: Vec<String>,
    #[serde(default)]
    pub compose_files: Vec<String>,
    #[serde(default)]
    pub worktree_service_overrides: IndexMap<String, ServiceOverride>,
    /// References to top-level shared_services. Each entry is either a string (just name)
    /// or a map with one key (name) and value (overrides like db_name).
    #[serde(default, deserialize_with = "deserialize_shared_refs")]
    pub worktree_shared_services: Vec<SharedServiceRef>,
    #[serde(default)]
    pub worktree_setup: Vec<String>,
    #[serde(default)]
    pub worktree_pre_delete: Vec<String>,
    #[serde(default)]
    pub worktree_env: IndexMap<String, String>,
    /// File to write env overrides to (e.g. ".env.local"). If not set, overrides are exported via shell only.
    pub worktree_env_file: Option<String>,
    #[serde(default)]
    pub shortcuts: Vec<Shortcut>,
    #[serde(default)]
    pub services: IndexMap<String, Service>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Service {
    pub cmd: Option<String>,
    pub env: Option<String>,
    pub pre_start: Option<String>,
    #[serde(default)]
    pub shortcuts: Vec<Shortcut>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceOverride {
    #[serde(default)]
    pub environment: IndexMap<String, String>,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub mem_limit: Option<String>,
}

/// Top-level shared service definition (docker-compose-like).
#[derive(Debug, Deserialize, Clone)]
pub struct SharedServiceDef {
    pub image: String,
    /// Hostname for container→host access (e.g. "minio.local").
    pub host: Option<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub environment: IndexMap<String, String>,
    #[serde(default)]
    pub volumes: Vec<String>,
    pub command: Option<String>,
    #[serde(default)]
    pub healthcheck: Option<HealthCheck>,
    /// DB user (for auto db creation).
    pub db_user: Option<String>,
    /// DB password (for auto db creation).
    pub db_password: Option<String>,
    /// Max slots per instance (e.g. Redis: 16 db indexes). Auto-scales when exceeded.
    /// If not set, service has unlimited capacity (e.g. postgres with CREATE DATABASE).
    pub capacity: Option<u16>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HealthCheck {
    pub test: serde_yaml::Value,
    #[serde(default = "default_interval")]
    pub interval: String,
    #[serde(default = "default_timeout")]
    pub timeout: String,
    #[serde(default = "default_retries")]
    pub retries: u32,
}
fn default_interval() -> String { "10s".into() }
fn default_timeout() -> String { "3s".into() }
fn default_retries() -> u32 { 3 }

/// Per-dir reference to a top-level shared service.
#[derive(Debug, Clone)]
pub struct SharedServiceRef {
    pub name: String,
    /// DB name template for per-worktree database (e.g. "myapp_{{branch_safe}}").
    pub db_name: Option<String>,
    /// Override port for db creation (if different from shared service port).
    #[allow(dead_code)]
    pub port: Option<u16>,
}

/// Custom deserializer: accept list of strings or maps.
/// `- minio` → SharedServiceRef { name: "minio", db_name: None }
/// `- postgres: { db_name: "myapp_{{branch_safe}}" }` → SharedServiceRef { name: "postgres", db_name: Some(...) }
fn deserialize_shared_refs<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Vec<SharedServiceRef>, D::Error> {
    use serde::de;

    #[derive(Deserialize)]
    struct RefOverride {
        db_name: Option<String>,
        port: Option<u16>,
    }

    let values: Vec<serde_yaml::Value> = Vec::deserialize(deserializer)?;
    let mut result = Vec::new();

    for val in values {
        match val {
            serde_yaml::Value::String(name) => {
                result.push(SharedServiceRef { name, db_name: None, port: None });
            }
            serde_yaml::Value::Mapping(map) => {
                for (k, v) in map {
                    let name = k.as_str().ok_or_else(|| de::Error::custom("expected string key"))?.to_string();
                    let ov: RefOverride = serde_yaml::from_value(v).map_err(de::Error::custom)?;
                    result.push(SharedServiceRef { name, db_name: ov.db_name, port: ov.port });
                }
            }
            _ => return Err(de::Error::custom("expected string or map")),
        }
    }
    Ok(result)
}

/// Deserialize workspace/combination entries.
/// Supports both formats:
///   - "alias/service"             (original)
///   - "alias: svc1, svc2, svc3"   (compact — expands to alias/svc1, alias/svc2, alias/svc3)
///   - { alias: "svc1, svc2" }     (map form)
fn deserialize_workspace_entries<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<IndexMap<String, Vec<String>>, D::Error> {
    let raw: IndexMap<String, Vec<serde_yaml::Value>> = IndexMap::deserialize(deserializer)?;
    let mut result = IndexMap::new();

    for (ws_name, entries) in raw {
        let mut expanded = Vec::new();
        for val in entries {
            match val {
                serde_yaml::Value::String(s) => {
                    // "alias/service" or bare service name — pass through
                    expanded.push(s);
                }
                serde_yaml::Value::Mapping(map) => {
                    // { alias: "svc1, svc2" } → alias/svc1, alias/svc2
                    for (k, v) in map {
                        let alias = k.as_str().unwrap_or_default().to_string();
                        let svcs_str = v.as_str().unwrap_or_default();
                        for svc in svcs_str.split(',') {
                            let svc = svc.trim();
                            if !svc.is_empty() {
                                expanded.push(format!("{alias}/{svc}"));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        result.insert(ws_name, expanded);
    }
    Ok(result)
}

#[derive(Debug, Deserialize, Clone)]
pub struct Shortcut {
    pub cmd: String,
    pub desc: String,
}

/// Resolved service with inherited dir properties.
pub struct ResolvedService {
    pub cmd: String,
    pub work_dir: PathBuf,
    pub env: Option<String>,
    pub pre_start: Option<String>,
}

impl Config {
    /// Get merged workspaces (workspaces + combinations for backward compat).
    ///
    /// Clones both maps because the result merges `workspaces` and `combinations`
    /// (legacy alias) into a single owned IndexMap.
    pub fn all_workspaces(&self) -> IndexMap<String, Vec<String>> {
        let mut result = self.workspaces.clone();
        for (k, v) in &self.combinations {
            if !result.contains_key(k) {
                result.insert(k.clone(), v.clone());
            }
        }
        result
    }

    /// Look up a workspace/combination by name without cloning the entire map.
    /// Checks `workspaces` first, then `combinations` (legacy alias).
    fn lookup_workspace(&self, name: &str) -> Option<&Vec<String>> {
        self.workspaces.get(name).or_else(|| self.combinations.get(name))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(config)
    }

    /// Find service by dir/svc or alias/svc or just svc (if unique).
    pub fn find_service_entry(&self, entry: &str) -> Result<(String, String)> {
        if let Some((prefix, svc_name)) = entry.split_once('/') {
            // Try prefix as dir name first, then as alias
            for (dir_name, dir) in &self.repos {
                let matches = dir_name == prefix
                    || dir.alias.as_deref() == Some(prefix);
                if matches && dir.services.contains_key(svc_name) {
                    return Ok((dir_name.clone(), svc_name.to_string()));
                }
            }
            bail!("service '{entry}' not found (no dir/alias '{prefix}' with service '{svc_name}')");
        }

        // Bare service name — find unique match
        let mut matches: Vec<&str> = Vec::new();
        for (dir_name, dir) in &self.repos {
            if dir.services.contains_key(entry) {
                matches.push(dir_name.as_str());
            }
        }
        match matches.len() {
            0 => bail!("service '{}' not found in any dir", entry),
            1 => Ok((matches[0].to_owned(), entry.to_string())),
            _ => bail!(
                "ambiguous service '{}' — found in: {}. Use dir/service format.",
                entry,
                matches.join(", ")
            ),
        }
    }

    /// Resolve a target (combo name, dir/svc, or bare svc) to a list of (dir_name, svc_name).
    pub fn resolve_services(&self, target: &str) -> Result<Vec<(String, String)>> {
        // Check workspaces/combinations first
        if let Some(entries) = self.lookup_workspace(target) {
            let mut result = Vec::new();
            for entry in entries {
                result.push(self.find_service_entry(entry)?);
            }
            return Ok(result);
        }

        // Try as dir name — start all services in that dir
        if let Some(_dir) = self.repos.get(target) {
            let result: Vec<(String, String)> = self.repos[target]
                .services
                .keys()
                .map(|svc| (target.to_string(), svc.clone()))
                .collect();
            if !result.is_empty() {
                return Ok(result);
            }
        }

        // Try as alias — start all services in that dir
        for (dir_name, dir) in &self.repos {
            if dir.alias.as_deref() == Some(target) {
                let result: Vec<(String, String)> = dir.services.keys()
                    .map(|svc| (dir_name.clone(), svc.clone()))
                    .collect();
                if !result.is_empty() {
                    return Ok(result);
                }
            }
        }

        // Try as single service
        let (dir_name, svc_name) = self.find_service_entry(target)?;
        Ok(vec![(dir_name, svc_name)])
    }

    /// Resolve a (dir_name, svc_name) pair to a ResolvedService with inherited properties.
    pub fn resolve_service(&self, config_dir: &Path, dir_name: &str, svc_name: &str) -> Result<ResolvedService> {
        let dir = self.repos.get(dir_name)
            .ok_or_else(|| anyhow::anyhow!("dir '{}' not found", dir_name))?;
        let svc = dir.services.get(svc_name)
            .ok_or_else(|| anyhow::anyhow!("service '{}' not found in dir '{}'", svc_name, dir_name))?;

        let cmd = svc.cmd.clone()
            .ok_or_else(|| anyhow::anyhow!("service '{}/{}' has no 'cmd'", dir_name, svc_name))?;

        // dir_name IS the directory path (relative to config)
        let work_dir = {
            let p = Path::new(dir_name);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                config_dir.join(dir_name)
            }
        };

        // Service env overrides dir env
        let env = svc.env.clone().or_else(|| dir.env.clone());

        // Service pre_start overrides dir pre_start
        let pre_start = svc.pre_start.clone().or_else(|| dir.pre_start.clone());

        Ok(ResolvedService {
            cmd,
            work_dir,
            env,
            pre_start,
        })
    }

    /// Non-error version of find_service_entry. Returns None on failure.
    pub fn find_service_entry_quiet(&self, entry: &str) -> Option<(String, String)> {
        self.find_service_entry(entry).ok()
    }

    /// Get all services as flat list of (dir_name, svc_name).
    pub fn all_services(&self) -> Vec<(String, String)> {
        self.repos
            .iter()
            .flat_map(|(dir_name, dir)| {
                dir.services
                    .keys()
                    .map(move |svc_name| (dir_name.clone(), svc_name.clone()))
            })
            .collect()
    }
}

pub fn find_config() -> Result<PathBuf> {
    let mut dir = env::current_dir()?;
    loop {
        let candidate = dir.join("tncli.yml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !dir.pop() {
            bail!("no tncli.yml found (searched from current directory to /)");
        }
    }
}
