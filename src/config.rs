use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_session")]
    pub session: String,
    /// Default git branch for main worktree (e.g. "master", "main").
    /// Per-repo override via Dir.default_branch.
    pub default_branch: Option<String>,
    #[serde(default, alias = "dirs")]
    pub repos: IndexMap<String, Dir>,
    /// Global env vars inherited by all repos. Per-repo env overrides these.
    #[serde(default)]
    pub env: IndexMap<String, String>,
    /// Reusable worktree presets (setup, pre_delete, shortcuts).
    #[serde(default)]
    pub presets: IndexMap<String, PresetConfig>,
    /// Top-level shared service definitions (docker-compose-like).
    #[serde(default)]
    pub shared_services: IndexMap<String, SharedServiceDef>,
    /// Workspaces (groups of services). Legacy — all repos = one workspace now.
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
    /// Override default_branch for this repo.
    pub default_branch: Option<String>,
    /// Worktree configuration. If present, worktree support is enabled for this dir.
    #[serde(default)]
    pub worktree: Option<WorktreeConfig>,
    #[serde(default)]
    pub shortcuts: Vec<Shortcut>,
    #[serde(default)]
    pub services: IndexMap<String, Service>,
    /// Port to reverse-proxy for inter-service communication.
    /// Proxy listens on 127.0.0.1:PORT and routes by Host header to the correct workspace bind_ip.
    pub proxy_port: Option<u16>,
}

/// Worktree configuration block. Presence of this block enables worktree support.
#[derive(Debug, Deserialize, Clone)]
pub struct WorktreeConfig {
    #[serde(default)]
    pub copy: Vec<String>,
    #[serde(default)]
    pub compose_files: Vec<String>,
    /// File(s) to write env overrides to. Supports:
    /// - `env_files: ".env.local"` (single string)
    /// - `env_files: [".env.development.local", ".env.test.local"]` (list of strings)
    /// - `env_files: [{file: ".env.test.local", env: {KEY: val}}]` (per-file env overrides)
    #[serde(default, alias = "env_file", deserialize_with = "deserialize_env_files")]
    pub env_files: Vec<EnvFileEntry>,
    #[serde(default)]
    pub env: IndexMap<String, String>,
    #[serde(default)]
    pub service_overrides: IndexMap<String, ServiceOverride>,
    /// Compose services to disable (profiles: ["disabled"]).
    /// Shorthand for service_overrides with just profiles: ["disabled"].
    #[serde(default)]
    pub disable: Vec<String>,
    /// References to top-level shared_services. Each entry is either a string (just name)
    /// or a map with one key (name) and value (overrides like db_name).
    #[serde(default, deserialize_with = "deserialize_shared_refs")]
    pub shared_services: Vec<SharedServiceRef>,
    /// Database names to create on shared postgres. Auto-prefixed with `{session}_`.
    /// Example: `["{{branch_safe}}", "transaction_{{branch_safe}}"]`
    /// → creates `boom_main`, `boom_transaction_main` (session=boom, branch=main)
    #[serde(default)]
    pub databases: Vec<String>,
    /// Preset name to inherit setup, pre_delete, shortcuts from.
    pub preset: Option<String>,
    #[serde(default)]
    pub setup: Vec<String>,
    #[serde(default)]
    pub pre_delete: Vec<String>,
}

/// Reusable preset for worktree setup/pre_delete/shortcuts.
#[derive(Debug, Deserialize, Clone)]
pub struct PresetConfig {
    #[serde(default)]
    pub setup: Vec<String>,
    #[serde(default)]
    pub pre_delete: Vec<String>,
    #[serde(default)]
    pub shortcuts: Vec<Shortcut>,
}

/// An env file target with optional per-file env overrides.
#[derive(Debug, Clone)]
pub struct EnvFileEntry {
    pub file: String,
    /// Per-file env overrides (merged on top of global `env`).
    pub env: IndexMap<String, String>,
}

/// Default env file entry when none configured.
static DEFAULT_ENV_FILE: std::sync::LazyLock<EnvFileEntry> = std::sync::LazyLock::new(|| {
    EnvFileEntry { file: ".env.local".into(), env: IndexMap::new() }
});

impl WorktreeConfig {
    /// Get env file entries. Falls back to [".env.local"] if empty.
    pub fn env_file_entries(&self) -> Vec<&EnvFileEntry> {
        if self.env_files.is_empty() {
            vec![&DEFAULT_ENV_FILE]
        } else {
            self.env_files.iter().collect()
        }
    }

    /// Apply env overrides for all configured env files.
    /// For each file, merges global `env` with per-file `env` (per-file wins),
    /// resolves templates, then writes.
    pub fn apply_all_env_files(&self, dir: &std::path::Path, config: &crate::config::Config, bind_ip: &str, branch: &str, ws_key: &str) {
        let branch_safe = crate::services::branch_safe(branch);
        // Pre-resolve database names for {{db:N}} templates
        let db_names: Vec<String> = self.databases.iter()
            .map(|tpl| {
                let name = tpl.replace("{{branch_safe}}", &branch_safe).replace("{{branch}}", branch);
                format!("{}_{name}", config.session)
            })
            .collect();
        // Merge: global env → worktree env (worktree wins)
        let mut base_env = config.env.clone();
        for (k, v) in &self.env {
            base_env.insert(k.clone(), v.clone());
        }
        for entry in self.env_file_entries() {
            let env_src = if entry.env.is_empty() {
                base_env.clone()
            } else {
                let mut merged = base_env.clone();
                for (k, v) in &entry.env {
                    merged.insert(k.clone(), v.clone());
                }
                merged
            };
            let mut resolved = crate::services::resolve_env_templates(&env_src, config, bind_ip, &branch_safe, branch, ws_key);
            // Resolve {{db:N}} with this repo's databases
            for (_, v) in resolved.iter_mut() {
                *v = crate::services::resolve_db_templates(v, &db_names);
            }
            crate::services::apply_env_overrides(dir, &resolved, &entry.file);
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Service {
    pub cmd: Option<String>,
    /// Env prefix string prepended to command (e.g. "RAILS_ENV=production").
    pub env: Option<String>,
    /// Per-service env vars (template-resolved). Merged on top of worktree env.
    #[serde(default)]
    pub env_vars: IndexMap<String, String>,
    pub pre_start: Option<String>,
    /// Per-service proxy port. Registers route: {session}.{svc_name}.ws-{branch}.tncli.test:{port}
    pub proxy_port: Option<u16>,
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
    /// Hostname for container→host access.
    /// If omitted, auto-generated as `{name}.{session}.tncli.test`.
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

/// Custom deserializer for env_files. Accepts:
/// - `".env.local"` → single entry, no per-file env
/// - `[".env.local", ".env.test.local"]` → multiple entries, no per-file env
/// - `[".env.local", {file: ".env.test.local", env: {KEY: val}}]` → mixed
fn deserialize_env_files<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Vec<EnvFileEntry>, D::Error> {
    let val: serde_yaml::Value = serde_yaml::Value::deserialize(deserializer)?;

    fn parse_entry(v: serde_yaml::Value) -> std::result::Result<EnvFileEntry, String> {
        match v {
            serde_yaml::Value::String(s) => Ok(EnvFileEntry { file: s, env: IndexMap::new() }),
            serde_yaml::Value::Mapping(map) => {
                let file = map.get(&serde_yaml::Value::String("file".into()))
                    .and_then(|v| v.as_str())
                    .ok_or("env_files map entry requires 'file' key")?
                    .to_string();
                let env = map.get(&serde_yaml::Value::String("env".into()))
                    .and_then(|v| v.as_mapping())
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| {
                                Some((k.as_str()?.to_string(), v.as_str()?.to_string()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(EnvFileEntry { file, env })
            }
            _ => Err("env_files entry must be a string or map".into()),
        }
    }

    match val {
        serde_yaml::Value::String(s) => Ok(vec![EnvFileEntry { file: s, env: IndexMap::new() }]),
        serde_yaml::Value::Sequence(seq) => {
            seq.into_iter()
                .map(|v| parse_entry(v).map_err(serde::de::Error::custom))
                .collect()
        }
        serde_yaml::Value::Null => Ok(Vec::new()),
        _ => Err(serde::de::Error::custom("env_files must be a string, list, or null")),
    }
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

impl Dir {
    /// Check if worktree support is enabled.
    pub fn has_worktree(&self) -> bool {
        self.worktree.is_some()
    }

    /// Get worktree config ref (convenience).
    pub fn wt(&self) -> Option<&WorktreeConfig> {
        self.worktree.as_ref()
    }
}

impl Config {
    /// Get global default branch (for workspace folder naming). No per-repo override.
    pub fn global_default_branch(&self) -> &str {
        self.default_branch.as_deref().unwrap_or("main")
    }

    /// Get default branch for a repo (per-repo override → global → "main").
    pub fn default_branch_for(&self, repo_name: &str) -> String {
        self.repos.get(repo_name)
            .and_then(|d| d.default_branch.as_deref())
            .or(self.default_branch.as_deref())
            .unwrap_or("main")
            .to_string()
    }

    /// Resolve shared service hostname.
    /// If `host` is set in config, use it. Otherwise auto-generate `{session}.{name}.tncli.test`.
    pub fn shared_host(&self, service_name: &str) -> String {
        self.shared_services.get(service_name)
            .and_then(|s| s.host.clone())
            .unwrap_or_else(|| format!("{}.{service_name}.tncli.test", self.session))
    }

    /// Get all workspaces. If none defined, auto-generate one from all repos.
    /// All repos in config = one workspace named after the session.
    pub fn all_workspaces(&self) -> IndexMap<String, Vec<String>> {
        // Check explicit workspaces/combinations first
        let mut result = self.workspaces.clone();
        for (k, v) in &self.combinations {
            if !result.contains_key(k) {
                result.insert(k.clone(), v.clone());
            }
        }

        // If none defined, auto-generate: all repos = one workspace
        if result.is_empty() {
            let entries: Vec<String> = self.repos.iter()
                .flat_map(|(_, dir)| {
                    let alias = dir.alias.as_deref().unwrap_or("");
                    dir.services.keys().map(move |svc| {
                        if alias.is_empty() { svc.clone() } else { format!("{alias}/{svc}") }
                    })
                })
                .collect();
            if !entries.is_empty() {
                result.insert(self.session.clone(), entries);
            }
        }

        result
    }

    /// Look up a workspace by name.
    fn lookup_workspace(&self, name: &str) -> Option<Vec<String>> {
        let all = self.all_workspaces();
        all.get(name).cloned()
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        config.apply_presets();
        Ok(config)
    }

    /// Apply presets: merge preset setup/pre_delete/shortcuts into repos that reference them.
    /// Repo-level values take priority (preset provides defaults).
    fn apply_presets(&mut self) {
        for dir in self.repos.values_mut() {
            if let Some(wt) = &mut dir.worktree {
                if let Some(preset_name) = &wt.preset {
                    if let Some(preset) = self.presets.get(preset_name) {
                        if wt.setup.is_empty() {
                            wt.setup = preset.setup.clone();
                        }
                        if wt.pre_delete.is_empty() {
                            wt.pre_delete = preset.pre_delete.clone();
                        }
                        if dir.shortcuts.is_empty() {
                            dir.shortcuts = preset.shortcuts.clone();
                        }
                    }
                }
            }
        }
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
            for entry in &entries {
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

        // Resolve work_dir through main workspace folder
        let work_dir = {
            let p = Path::new(dir_name);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                let ws_path = config_dir.join(format!("workspace--{}", self.global_default_branch())).join(dir_name);
                if ws_path.exists() {
                    ws_path
                } else {
                    config_dir.join(dir_name)
                }
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
    #[allow(dead_code)]
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
