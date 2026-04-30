use std::collections::HashMap;
use std::path::PathBuf;

// ── Route Table ──

const PROXY_ROUTES_FILE: &str = ".tncli/proxy-routes.json";
const PROXY_PID_FILE: &str = ".tncli/proxy.pid";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProxyRoutes {
    /// Ports the proxy should listen on (127.0.0.1:PORT).
    #[serde(default)]
    pub listen_ports: Vec<u16>,
    /// hostname:port → bind_ip:port mapping.
    #[serde(default)]
    pub routes: HashMap<String, String>,
}

impl Default for ProxyRoutes {
    fn default() -> Self {
        Self { listen_ports: Vec::new(), routes: HashMap::new() }
    }
}

fn routes_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(PROXY_ROUTES_FILE)
}

fn pid_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(PROXY_PID_FILE)
}

pub fn load_routes() -> ProxyRoutes {
    let path = routes_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Save routes (public for migration).
pub fn save_routes_pub(routes: &ProxyRoutes) { save_routes(routes) }

fn save_routes(routes: &ProxyRoutes) {
    let path = routes_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(routes) {
        let _ = std::fs::write(&path, json);
    }
}

/// Build proxy hostname: {session}.{alias}.ws-{branch_safe}.tncli.test
pub fn proxy_hostname(session: &str, alias: &str, branch_safe: &str) -> String {
    format!("{session}.{alias}.ws-{branch_safe}.tncli.test")
}

/// Register routes for a workspace. Called when workspace is created or services start.
/// `services` is a list of (alias, proxy_port, bind_ip) tuples.
pub fn register_routes(session: &str, branch_safe: &str, services: &[(&str, u16, &str)]) {
    super::ip::with_ip_lock(|| {
        let mut routes = load_routes();
        for &(alias, port, bind_ip) in services {
            let hostname = proxy_hostname(session, alias, branch_safe);
            let key = format!("{hostname}:{port}");
            let target = format!("{bind_ip}:{port}");
            routes.routes.insert(key, target);
            if !routes.listen_ports.contains(&port) {
                routes.listen_ports.push(port);
            }
        }
        save_routes(&routes);
    });

}

/// Unregister routes for a workspace. Called when workspace is deleted.
pub fn unregister_routes(branch_safe: &str) {
    super::ip::with_ip_lock(|| {
        let mut routes = load_routes();
        let prefix = format!(".ws-{branch_safe}.tncli.test:");
        routes.routes.retain(|k, _| !k.contains(&prefix));
        // Recalculate listen_ports from remaining routes
        let mut ports: Vec<u16> = routes.routes.keys()
            .filter_map(|k| k.rsplit(':').next()?.parse().ok())
            .collect();
        ports.sort();
        ports.dedup();
        routes.listen_ports = ports;
        save_routes(&routes);
    });
}

// ── Proxy PID Management ──

pub fn save_pid(pid: u32) {
    let path = pid_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, pid.to_string());
}

pub fn read_pid() -> Option<u32> {
    std::fs::read_to_string(pid_path()).ok()?.trim().parse().ok()
}

pub fn remove_pid() {
    let _ = std::fs::remove_file(pid_path());
}

pub fn is_proxy_running() -> bool {
    if let Some(pid) = read_pid() {
        // Check if process is alive via kill -0
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .is_ok_and(|o| o.status.success())
    } else {
        false
    }
}

// ── Caddy Proxy Server ──

const CADDYFILE_PATH: &str = ".tncli/Caddyfile";

fn caddyfile_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(CADDYFILE_PATH)
}

/// Generate Caddyfile from proxy routes — grouped by port with host matchers.
pub fn generate_caddyfile() {
    let routes = load_routes();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let log = format!("{home}/.tncli/proxy.log");
    let mut cfg = format!("{{\n  auto_https off\n  log {{\n    output file {log} {{\n      roll_size 1mb\n      roll_keep 1\n    }}\n    level WARN\n  }}\n}}\n\n");

    // Group routes by port: port → [(hostname, target)]
    let mut port_routes: HashMap<u16, Vec<(String, String)>> = HashMap::new();
    for (key, target) in &routes.routes {
        if let Some((hostname, port_str)) = key.rsplit_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                if !target.is_empty() && !target.starts_with(':') {
                    port_routes.entry(port).or_default().push((hostname.to_string(), target.clone()));
                }
            }
        }
    }

    // One listener per port, bind ONLY to 127.0.0.1 (not all interfaces).
    // Services bind to 127.0.1.x — prevents proxy loop when service calls hostname.
    for (port, routes) in &port_routes {
        cfg.push_str(&format!("http://:{port} {{\n"));
        cfg.push_str("  bind 127.0.0.1\n");
        for (i, (hostname, target)) in routes.iter().enumerate() {
            cfg.push_str(&format!("  @r{i} host {hostname}\n"));
            cfg.push_str(&format!("  reverse_proxy @r{i} {target}\n"));
        }
        cfg.push_str("}\n\n");
    }

    let path = caddyfile_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &cfg);
}

/// Run proxy via Caddy (blocking — called from `tncli proxy serve`).
pub fn run_proxy_server() -> anyhow::Result<()> {
    generate_caddyfile();
    save_pid(std::process::id());

    let caddy_path = caddyfile_path();
    let status = std::process::Command::new("caddy")
        .args(["run", "--config", &caddy_path.to_string_lossy()])
        .status()?;

    remove_pid();
    if !status.success() {
        anyhow::bail!("caddy exited with {status}");
    }
    Ok(())
}

/// Reload Caddy config (after routes change).
pub fn reload_caddy() {
    generate_caddyfile();
    let caddy_path = caddyfile_path();
    let _ = std::process::Command::new("caddy")
        .args(["reload", "--config", &caddy_path.to_string_lossy()])
        .output();
}
