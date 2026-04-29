use std::path::PathBuf;
use std::process::Command;

/// dnsmasq config line for *.tncli.local → 127.0.0.1
const DNSMASQ_ENTRY: &str = "address=/tncli.local/127.0.0.1";
/// macOS resolver file
const RESOLVER_PATH: &str = "/etc/resolver/tncli.local";

/// Get path to homebrew dnsmasq config (Apple Silicon or Intel).
fn dnsmasq_conf_path() -> PathBuf {
    let arm = PathBuf::from("/opt/homebrew/etc/dnsmasq.conf");
    if arm.exists() {
        return arm;
    }
    // Intel Mac
    let intel = PathBuf::from("/usr/local/etc/dnsmasq.conf");
    if intel.exists() {
        return intel;
    }
    // Fallback to arm path (will be created)
    arm
}

/// Check if dnsmasq is installed via brew.
pub fn is_dnsmasq_installed() -> bool {
    Command::new("brew")
        .args(["list", "dnsmasq"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Check if dnsmasq is configured for tncli.local.
pub fn is_dnsmasq_configured() -> bool {
    let conf = dnsmasq_conf_path();
    std::fs::read_to_string(conf)
        .unwrap_or_default()
        .contains(DNSMASQ_ENTRY)
}

/// Check if macOS resolver is configured.
pub fn is_resolver_configured() -> bool {
    std::path::Path::new(RESOLVER_PATH).exists()
}

/// Check if dnsmasq service is running.
pub fn is_dnsmasq_running() -> bool {
    Command::new("brew")
        .args(["services", "info", "dnsmasq", "--json"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .is_some_and(|s| s.contains("\"running\""))
}

/// Full DNS status check.
pub fn status() -> DnsStatus {
    DnsStatus {
        dnsmasq_installed: is_dnsmasq_installed(),
        dnsmasq_configured: is_dnsmasq_configured(),
        dnsmasq_running: is_dnsmasq_running(),
        resolver_configured: is_resolver_configured(),
    }
}

pub struct DnsStatus {
    pub dnsmasq_installed: bool,
    pub dnsmasq_configured: bool,
    pub dnsmasq_running: bool,
    pub resolver_configured: bool,
}

impl DnsStatus {
    pub fn is_ready(&self) -> bool {
        self.dnsmasq_installed && self.dnsmasq_configured && self.dnsmasq_running && self.resolver_configured
    }
}

/// Setup dnsmasq for *.tncli.local resolution.
/// Returns list of actions taken. Requires sudo for resolver + dnsmasq restart.
pub fn setup_dnsmasq() -> anyhow::Result<Vec<String>> {
    let mut actions = Vec::new();

    // 1. Install dnsmasq if needed (no sudo)
    if !is_dnsmasq_installed() {
        eprintln!("Installing dnsmasq via brew...");
        let s = Command::new("brew").args(["install", "dnsmasq"]).status()?;
        if !s.success() {
            anyhow::bail!("failed to install dnsmasq");
        }
        actions.push("installed dnsmasq".into());
    }

    // 2. Add tncli.local config (no sudo — homebrew-owned dir)
    if !is_dnsmasq_configured() {
        let conf = dnsmasq_conf_path();
        if let Some(parent) = conf.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut content = std::fs::read_to_string(&conf).unwrap_or_default();
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&format!("# tncli proxy\n{DNSMASQ_ENTRY}\n"));
        std::fs::write(&conf, content)?;
        actions.push(format!("added {DNSMASQ_ENTRY} to {}", conf.display()));
    }

    // 3. Create /etc/resolver/tncli.local (SUDO)
    if !is_resolver_configured() {
        let s = Command::new("sudo")
            .args(["mkdir", "-p", "/etc/resolver"])
            .status()?;
        if !s.success() {
            anyhow::bail!("failed to create /etc/resolver (sudo)");
        }
        let s = Command::new("sudo")
            .args(["sh", "-c", &format!("echo 'nameserver 127.0.0.1' > {RESOLVER_PATH}")])
            .status()?;
        if !s.success() {
            anyhow::bail!("failed to create {RESOLVER_PATH} (sudo)");
        }
        actions.push(format!("created {RESOLVER_PATH}"));
    }

    // 4. Start/restart dnsmasq service (SUDO — port 53)
    let restart = if is_dnsmasq_running() {
        Command::new("sudo").args(["brew", "services", "restart", "dnsmasq"]).status()?
    } else {
        Command::new("sudo").args(["brew", "services", "start", "dnsmasq"]).status()?
    };
    if !restart.success() {
        anyhow::bail!("failed to start dnsmasq service (sudo)");
    }
    actions.push("dnsmasq service started".into());

    Ok(actions)
}

/// Verify DNS resolution works for *.tncli.local.
pub fn verify_resolution() -> bool {
    // Use dscacheutil (macOS) to check resolution
    Command::new("dscacheutil")
        .args(["-q", "host", "-a", "name", "test.tncli.local"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .is_some_and(|s| s.contains("127.0.0.1"))
}
