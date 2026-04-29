use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

// ── Constants ──

const LOOPBACK_STATE_FILE: &str = ".tncli/loopback.json";
const SUBNET_STATE_FILE: &str = ".tncli/subnets.json";

/// Number of subnets pre-created by `tncli setup`.
/// Each subnet = 127.0.{slot}.{2..254} (253 IPs).
pub const SETUP_SUBNET_COUNT: u8 = 10;

// ── Helpers ──

fn home_path(rel: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(rel)
}

fn state_path() -> PathBuf { home_path(LOOPBACK_STATE_FILE) }
fn subnet_path() -> PathBuf { home_path(SUBNET_STATE_FILE) }

// ── File Lock ──

/// File-lock protected operation (safe for concurrent pipelines).
pub(crate) fn with_ip_lock<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    use std::fs::OpenOptions;
    use std::io::Write;

    let lock_path = home_path(".tncli/loopback.lock");
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

// ── Subnet Allocation ──

/// Load subnet allocations: session name → subnet slot (1-based).
pub fn load_subnets() -> HashMap<String, u8> {
    let path = subnet_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_subnets(subnets: &HashMap<String, u8>) {
    let path = subnet_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(subnets) {
        let _ = std::fs::write(&path, json);
    }
}

/// Allocate a subnet slot for a session. Returns existing or next available (1-based).
/// Thread-safe via file lock. No sudo — purely file-based.
pub fn allocate_subnet(session: &str) -> u8 {
    with_ip_lock(|| {
        let mut subnets = load_subnets();

        if let Some(&slot) = subnets.get(session) {
            return slot;
        }

        let used: HashSet<u8> = subnets.values().copied().collect();
        let slot = (1..=254u8).find(|n| !used.contains(n)).unwrap_or(254);
        subnets.insert(session.to_string(), slot);
        save_subnets(&subnets);
        slot
    })
}

/// Release a subnet slot for a session.
#[allow(dead_code)]
pub fn release_subnet(session: &str) {
    with_ip_lock(|| {
        let mut subnets = load_subnets();
        subnets.remove(session);
        save_subnets(&subnets);
    });
}

// ── Loopback IP Allocation ──

/// Load IP allocations from disk.
pub fn load_ip_allocations() -> HashMap<String, String> {
    let path = state_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_ip_allocations(allocs: &HashMap<String, String>) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(allocs) {
        let _ = std::fs::write(&path, json);
    }
}

/// Get the allocated IP for the main workspace. Allocates one if needed.
pub fn main_ip(session: &str, default_branch: &str) -> String {
    let key = format!("ws-{}", default_branch.replace('/', "-"));
    allocate_ip(session, &key)
}

/// Allocate next available loopback IP within the session's subnet.
/// Format: 127.0.{subnet_slot}.{2..254}
/// Thread-safe via file lock. No sudo — purely file-based.
pub fn allocate_ip(session: &str, worktree_key: &str) -> String {
    let subnet = allocate_subnet(session);

    with_ip_lock(|| {
        let mut allocs = load_ip_allocations();

        if let Some(ip) = allocs.get(worktree_key) {
            return ip.clone();
        }

        let prefix = format!("127.0.{subnet}.");
        let used: HashSet<&str> = allocs.values().map(|s| s.as_str()).collect();
        let mut n = 2u8;
        loop {
            let ip = format!("{prefix}{n}");
            if !used.contains(ip.as_str()) {
                allocs.insert(worktree_key.to_string(), ip.clone());
                save_ip_allocations(&allocs);
                return ip;
            }
            n += 1;
            if n == 255 {
                let fallback = format!("{prefix}254");
                allocs.insert(worktree_key.to_string(), fallback.clone());
                save_ip_allocations(&allocs);
                return fallback;
            }
        }
    })
}

/// Release an allocated IP. Thread-safe via file lock.
pub fn release_ip(worktree_key: &str) {
    with_ip_lock(|| {
        let mut allocs = load_ip_allocations();
        allocs.remove(worktree_key);
        save_ip_allocations(&allocs);
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
