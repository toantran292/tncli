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
    pub services: IndexMap<String, Service>,
    #[serde(default)]
    pub combinations: IndexMap<String, Vec<String>>,
}

fn default_session() -> String {
    "tncli".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct Service {
    pub cmd: Option<String>,
    pub dir: Option<String>,
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

    pub fn resolve_services(&self, target: &str) -> Result<Vec<String>> {
        if let Some(combo) = self.combinations.get(target) {
            return Ok(combo.clone());
        }
        if self.services.contains_key(target) {
            return Ok(vec![target.to_string()]);
        }
        bail!("unknown service or combination: '{}'", target);
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
