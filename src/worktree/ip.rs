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

/// Allocate next available loopback IP (127.0.0.2, 127.0.0.3, ...).
pub fn allocate_ip(worktree_key: &str) -> String {
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
}

/// Release an allocated IP.
pub fn release_ip(worktree_key: &str) {
    let mut allocs = load_ip_allocations();
    allocs.remove(worktree_key);
    save_ip_allocations(&allocs);
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
