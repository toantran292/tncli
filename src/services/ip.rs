use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

// ── Loopback IP Allocation ──

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

fn save_ip_allocations(allocs: &HashMap<String, String>) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(allocs) {
        let _ = std::fs::write(&path, json);
    }
}

/// File-lock protected IP allocation (safe for concurrent pipelines).
fn with_ip_lock<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    use std::fs::OpenOptions;
    use std::io::Write;

    let lock_path = {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(".tncli/loopback.lock")
    };
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

/// Allocate next available loopback IP (127.0.0.2, 127.0.0.3, ...).
/// Thread-safe via file lock.
pub fn allocate_ip(worktree_key: &str) -> String {
    with_ip_lock(|| {
        let mut allocs = load_ip_allocations();

        if let Some(ip) = allocs.get(worktree_key) {
            return ip.clone();
        }

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
                return "127.0.0.254".to_string();
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

/// Create loopback alias (requires sudo).
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

/// Add hostnames to /etc/hosts pointing to 127.0.0.1 (CLI — uses sudo).
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
