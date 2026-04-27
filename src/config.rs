use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_session")]
    pub session: String,
    #[serde(default)]
    pub dirs: IndexMap<String, Dir>,
    #[serde(default)]
    pub combinations: IndexMap<String, Vec<String>>,
}

fn default_session() -> String {
    "tncli".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct Dir {
    pub alias: Option<String>,
    pub pre_start: Option<String>,
    pub env: Option<String>,
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
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(config)
    }

    /// Find service by dir/svc or alias/svc or just svc (if unique).
    fn find_service_entry(&self, entry: &str) -> Result<(String, String)> {
        if let Some((prefix, svc_name)) = entry.split_once('/') {
            // Try prefix as dir name first, then as alias
            for (dir_name, dir) in &self.dirs {
                let matches = dir_name == prefix
                    || dir.alias.as_deref() == Some(prefix);
                if matches && dir.services.contains_key(svc_name) {
                    return Ok((dir_name.clone(), svc_name.to_string()));
                }
            }
            bail!("service '{entry}' not found (no dir/alias '{prefix}' with service '{svc_name}')");
        }

        // Bare service name — find unique match
        let mut matches = Vec::new();
        for (dir_name, dir) in &self.dirs {
            if dir.services.contains_key(entry) {
                matches.push(dir_name.clone());
            }
        }
        match matches.len() {
            0 => bail!("service '{}' not found in any dir", entry),
            1 => Ok((matches[0].clone(), entry.to_string())),
            _ => bail!(
                "ambiguous service '{}' — found in: {}. Use dir/service format.",
                entry,
                matches.join(", ")
            ),
        }
    }

    /// Resolve a target (combo name, dir/svc, or bare svc) to a list of (dir_name, svc_name).
    pub fn resolve_services(&self, target: &str) -> Result<Vec<(String, String)>> {
        // Check combinations first
        if let Some(entries) = self.combinations.get(target) {
            let mut result = Vec::new();
            for entry in entries {
                result.push(self.find_service_entry(entry)?);
            }
            return Ok(result);
        }

        // Try as dir name — start all services in that dir
        if let Some(_dir) = self.dirs.get(target) {
            let result: Vec<(String, String)> = self.dirs[target]
                .services
                .keys()
                .map(|svc| (target.to_string(), svc.clone()))
                .collect();
            if !result.is_empty() {
                return Ok(result);
            }
        }

        // Try as alias — start all services in that dir
        for (dir_name, dir) in &self.dirs {
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
        let dir = self.dirs.get(dir_name)
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
        let mut result = Vec::new();
        for (dir_name, dir) in &self.dirs {
            for svc_name in dir.services.keys() {
                result.push((dir_name.clone(), svc_name.clone()));
            }
        }
        result
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
