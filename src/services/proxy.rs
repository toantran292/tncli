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

/// Build proxy hostname: {alias}.ws-{branch_safe}.tncli.test
pub fn proxy_hostname(alias: &str, branch_safe: &str) -> String {
    format!("{alias}.ws-{branch_safe}.tncli.test")
}

/// Register routes for a workspace. Called when workspace is created or services start.
/// `services` is a list of (alias, proxy_port, bind_ip) tuples.
/// Also ensures proxy hostnames are in /etc/hosts → 127.0.0.1.
pub fn register_routes(branch_safe: &str, services: &[(&str, u16, &str)]) {
    super::ip::with_ip_lock(|| {
        let mut routes = load_routes();
        for &(alias, port, bind_ip) in services {
            let hostname = proxy_hostname(alias, branch_safe);
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

// ── TCP Proxy Server ──

/// Run the proxy server (blocking — meant to be called from daemon/subcommand).
pub fn run_proxy_server() -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        proxy_main().await
    })
}

async fn spawn_listener(
    port: u16,
    route_map: &std::sync::Arc<std::sync::RwLock<HashMap<String, String>>>,
    active_ports: &std::sync::Arc<std::sync::RwLock<std::collections::HashSet<u16>>>,
) {
    use tokio::net::TcpListener;

    {
        let current = active_ports.read().unwrap();
        if current.contains(&port) {
            return;
        }
    }

    let addr = format!("127.0.0.1:{port}");
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[proxy] failed to bind {addr}: {e}");
            return;
        }
    };
    eprintln!("[proxy] listening on {addr}");

    {
        let mut ports = active_ports.write().unwrap();
        ports.insert(port);
    }

    let map = route_map.clone();
    tokio::spawn(async move {
        loop {
            let (stream, _peer) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => continue,
            };
            let map = map.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, port, &map).await {
                    eprintln!("[proxy] connection error: {e}");
                }
            });
        }
    });
}

async fn proxy_main() -> anyhow::Result<()> {
    use tokio::signal;

    save_pid(std::process::id());

    let routes = load_routes();
    let route_map = std::sync::Arc::new(std::sync::RwLock::new(routes.routes.clone()));
    let active_ports: std::sync::Arc<std::sync::RwLock<std::collections::HashSet<u16>>> =
        std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new()));

    // Start listeners for initial ports
    for port in &routes.listen_ports {
        spawn_listener(*port, &route_map, &active_ports).await;
    }

    if routes.listen_ports.is_empty() {
        eprintln!("[proxy] no listen ports yet — waiting for routes...");
    }

    // Poll for route file changes every 5s (picks up new workspaces/ports)
    let map_poll = route_map.clone();
    let ports_poll = active_ports.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let new_routes = load_routes();
            if let Ok(mut w) = map_poll.write() {
                *w = new_routes.routes;
            }
            // Start listeners for any new ports
            let new_ports: Vec<u16> = {
                let current = ports_poll.read().unwrap();
                new_routes.listen_ports.iter().filter(|p| !current.contains(p)).copied().collect()
            };
            for port in new_ports {
                spawn_listener(port, &map_poll, &ports_poll).await;
            }
        }
    });

    // Reload routes on SIGHUP
    let map_reload = route_map.clone();
    tokio::spawn(async move {
        let mut sighup = signal::unix::signal(signal::unix::SignalKind::hangup())
            .expect("failed to register SIGHUP");
        loop {
            sighup.recv().await;
            let new_routes = load_routes();
            if let Ok(mut w) = map_reload.write() {
                *w = new_routes.routes;
            }
            eprintln!("[proxy] routes reloaded");
        }
    });

    // Wait for SIGTERM/SIGINT
    signal::ctrl_c().await?;
    eprintln!("[proxy] shutting down");
    remove_pid();
    Ok(())
}

async fn handle_connection(
    mut inbound: tokio::net::TcpStream,
    port: u16,
    routes: &std::sync::RwLock<HashMap<String, String>>,
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;

    // Peek at the first bytes to find Host header
    let mut buf = vec![0u8; 8192];
    let n = inbound.peek(&mut buf).await?;
    let header_bytes = &buf[..n];

    let host = extract_host_header(header_bytes)
        .unwrap_or_default()
        .to_lowercase();

    // Strip port from Host header if present, then re-add our listening port
    let host_no_port = host.split(':').next().unwrap_or(&host);
    let route_key = format!("{host_no_port}:{port}");

    let target = {
        let map = routes.read().map_err(|e| anyhow::anyhow!("{e}"))?;
        map.get(&route_key).cloned()
    };

    let target = if let Some(t) = target {
        t
    } else {
        // No route found — try default (first route for this port as fallback)
        let fallback = {
            let map = routes.read().map_err(|e| anyhow::anyhow!("{e}"))?;
            map.iter()
                .find(|(k, _)| k.ends_with(&format!(":{port}")))
                .map(|(_, v)| v.clone())
        }; // lock dropped here
        match fallback {
            Some(t) => t,
            None => {
                let resp = "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = inbound.write_all(resp.as_bytes()).await;
                anyhow::bail!("no route for {route_key}");
            }
        }
    };

    // Connect to target
    let mut outbound = tokio::net::TcpStream::connect(&target).await?;

    // Bidirectional copy
    tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await?;
    Ok(())
}

/// Extract Host header value from raw HTTP bytes.
fn extract_host_header(buf: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(buf).ok()?;
    for line in text.split("\r\n").skip(1) {
        if line.is_empty() {
            break; // End of headers
        }
        if let Some(val) = line.strip_prefix("Host: ").or_else(|| line.strip_prefix("host: ")) {
            return Some(val.trim());
        }
        // Case-insensitive check
        if line.len() > 6 && line[..5].eq_ignore_ascii_case("host:") {
            return Some(line[5..].trim());
        }
    }
    None
}
