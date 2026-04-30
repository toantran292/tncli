use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

// ── Constants ──

const NETWORK_STATE_FILE: &str = ".tncli/network.json";
const CURRENT_VERSION: u8 = 2;

/// Number of subnets pre-created by `tncli setup`.
pub const SETUP_SUBNET_COUNT: u8 = 10;
/// Max host IPs per subnet created by setup (2..=SETUP_HOST_MAX).
pub const SETUP_HOST_MAX: u8 = 51;

// ── Unified Network State ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkState {
    #[serde(default = "default_version")]
    pub version: u8,
    /// Session name → subnet slot (1-based).
    #[serde(default)]
    pub subnets: HashMap<String, u8>,
    /// Worktree key → allocated IP.
    #[serde(default)]
    pub allocations: HashMap<String, String>,
}

fn default_version() -> u8 { 1 }

impl Default for NetworkState {
    fn default() -> Self {
        Self { version: CURRENT_VERSION, subnets: HashMap::new(), allocations: HashMap::new() }
    }
}

// ── Helpers ──

fn home_path(rel: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(rel)
}

fn state_path() -> PathBuf { home_path(NETWORK_STATE_FILE) }

pub fn load_network_state() -> NetworkState {
    let path = state_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_network_state(state: &NetworkState) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(&path, json);
    }
}

/// Compat wrapper — callers that read allocations get them from NetworkState.
pub fn load_ip_allocations() -> HashMap<String, String> {
    load_network_state().allocations
}

// ── File Lock ──

/// File-lock protected operation (safe for concurrent pipelines).
pub(crate) fn with_ip_lock<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    use std::fs::OpenOptions;
    use std::io::Write;

    let lock_path = home_path(".tncli/network.lock");
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Spin-lock on file creation (atomic on POSIX)
    let lockfile = loop {
        match OpenOptions::new().write(true).create_new(true).open(&lock_path) {
            Ok(mut f) => { let _ = write!(f, "{}", std::process::id()); break f; }
            Err(_) => {
                // Check if lock is stale (older than 10s)
                if let Ok(meta) = std::fs::metadata(&lock_path) {
                    if let Ok(modified) = meta.modified() {
                        if modified.elapsed().unwrap_or_default() > std::time::Duration::from_secs(10) {
                            let _ = std::fs::remove_file(&lock_path);
                            continue;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    };

    let result = f();
    drop(lockfile);
    let _ = std::fs::remove_file(&lock_path);
    result
}

// ── Legacy Migration ──

/// One-time migration from v1 (separate files) to v2 (unified network.json).
/// Also clears old 127.0.0.x entries and proxy routes.
/// Idempotent — skipped if network.json already at version 2.
pub fn migrate_legacy_ips() {
    let state = load_network_state();
    if state.version >= CURRENT_VERSION {
        return;
    }

    with_ip_lock(|| {
        let mut state = load_network_state();
        if state.version >= CURRENT_VERSION {
            return; // Another thread migrated
        }

        // Import from old separate files
        let old_loopback = home_path(".tncli/loopback.json");
        if old_loopback.exists() {
            if let Ok(s) = std::fs::read_to_string(&old_loopback) {
                if let Ok(allocs) = serde_json::from_str::<HashMap<String, String>>(&s) {
                    for (k, v) in allocs {
                        if !v.starts_with("127.0.0.") {
                            state.allocations.insert(k, v);
                        }
                    }
                }
            }
            let _ = std::fs::remove_file(&old_loopback);
        }

        let old_subnets = home_path(".tncli/subnets.json");
        if old_subnets.exists() {
            if let Ok(s) = std::fs::read_to_string(&old_subnets) {
                if let Ok(subs) = serde_json::from_str::<HashMap<String, u8>>(&s) {
                    state.subnets = subs;
                }
            }
            let _ = std::fs::remove_file(&old_subnets);
        }

        // Clear legacy 127.0.0.x from allocations
        state.allocations.retain(|_, ip| !ip.starts_with("127.0.0."));

        // Clear proxy routes with old targets
        let mut routes = super::proxy::load_routes();
        let before = routes.routes.len();
        routes.routes.retain(|_, target| !target.starts_with("127.0.0."));
        if routes.routes.len() != before {
            let mut ports: Vec<u16> = routes.routes.keys()
                .filter_map(|k| k.rsplit(':').next()?.parse().ok())
                .collect();
            ports.sort();
            ports.dedup();
            routes.listen_ports = ports;
            super::proxy::save_routes_pub(&routes);
        }

        state.version = CURRENT_VERSION;
        save_network_state(&state);

        // Clean up old migration flag
        let _ = std::fs::remove_file(home_path(".tncli/.migrated-subnet"));
    });
}

// ── Subnet Allocation ──

/// Release a subnet slot for a session.
#[allow(dead_code)]
pub fn release_subnet(session: &str) {
    with_ip_lock(|| {
        let mut state = load_network_state();
        state.subnets.remove(session);
        save_network_state(&state);
    });
}

// ── Loopback IP Allocation ──

/// Get the allocated IP for the main workspace. Allocates one if needed.
pub fn main_ip(session: &str, default_branch: &str) -> String {
    let key = format!("ws-{}", default_branch.replace('/', "-"));
    allocate_ip(session, &key)
}

/// Allocate next available loopback IP within the session's subnet.
/// Format: 127.0.{subnet_slot}.{2..254}
/// Thread-safe via file lock. No sudo — purely file-based.
pub fn allocate_ip(session: &str, worktree_key: &str) -> String {
    // Single lock for both subnet + IP allocation (avoid double-lock deadlock)
    with_ip_lock(|| {
        let mut state = load_network_state();

        // Allocate subnet if needed
        let subnet = if let Some(&slot) = state.subnets.get(session) {
            slot
        } else {
            let used: HashSet<u8> = state.subnets.values().copied().collect();
            let slot = (1..=254u8).find(|n| !used.contains(n)).unwrap_or(254);
            state.subnets.insert(session.to_string(), slot);
            slot
        };

        if let Some(ip) = state.allocations.get(worktree_key) {
            return ip.clone();
        }

        let prefix = format!("127.0.{subnet}.");
        let used: HashSet<&str> = state.allocations.values().map(|s| s.as_str()).collect();
        let mut n = 2u8;
        loop {
            let ip = format!("{prefix}{n}");
            if !used.contains(ip.as_str()) {
                state.allocations.insert(worktree_key.to_string(), ip.clone());
                save_network_state(&state);
                return ip;
            }
            n += 1;
            if n == 255 {
                let fallback = format!("{prefix}254");
                state.allocations.insert(worktree_key.to_string(), fallback.clone());
                save_network_state(&state);
                return fallback;
            }
        }
    })
}

/// Release an allocated IP. Thread-safe via file lock.
pub fn release_ip(worktree_key: &str) {
    with_ip_lock(|| {
        let mut state = load_network_state();
        state.allocations.remove(worktree_key);
        save_network_state(&state);
    });
}

/// Create loopback alias (requires sudo — setup only).
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

// ── /etc/hosts ──

pub fn check_etc_hosts(hostnames: &[&str]) -> Vec<String> {
    let content = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
    hostnames.iter()
        .filter(|h| !content.contains(*h))
        .map(|h| h.to_string())
        .collect()
}

/// Add hostnames to /etc/hosts pointing to 127.0.0.1 (setup only — uses sudo).
#[allow(dead_code)]
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
