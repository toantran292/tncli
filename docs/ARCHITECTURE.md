# tncli Architecture

Rust single-binary CLI+TUI for managing tmux-based development workspaces with isolated networking.

## Source Structure

```
src/
  main.rs              CLI entry point (Clap parser)
  config.rs            YAML config deserialization
  commands.rs          CLI command implementations
  lock.rs              Service lock files (/tmp/tncli)
  tmux.rs              tmux session/window management

  pipeline/
    mod.rs             Pipeline events, state persistence
    stages.rs          CreateStage (7) + DeleteStage (5) enums
    context.rs         CreateContext + DeleteContext
    create.rs          Workspace creation stages
    delete.rs          Workspace deletion stages

  services/
    mod.rs             WorktreeInfo struct, template resolution
    proxy.rs           TCP reverse proxy (tokio-based)
    dns.rs             dnsmasq integration (*.tncli.local)
    docker.rs          Docker network/project management
    compose.rs         docker-compose.override.yml generation
    files.rs           Env file generation, file copy
    git.rs             Git worktree operations
    ip.rs              Loopback IP allocation (127.0.0.2+)
    workspace.rs       Shared service compose, DB ops, slot allocation

  tui/
    mod.rs             Main TUI loop
    app.rs             App state + ComboItem tree
    event.rs           Background event polling (crossterm)
    ui.rs              Ratatui rendering
    screens/           Service, workspace, tree, log screens
```

## CLI Commands

```
tncli ui                              Interactive TUI
tncli start <target>                  Start service/combination
tncli stop [target]                   Stop (no arg = all)
tncli restart <target>                Restart
tncli status                          Show running services
tncli attach [target]                 Attach tmux session
tncli logs <target>                   Show recent output
tncli list                            List services + combos
tncli update                          Self-update from GitHub
tncli setup                           One-time setup (sudo)

tncli workspace create <ws> <branch>  Create workspace
tncli workspace delete <branch>       Delete workspace
tncli workspace list                  List workspaces

tncli db reset <branch>               Drop + recreate DBs

tncli proxy serve                     Run proxy (foreground)
tncli proxy start                     Start proxy daemon
tncli proxy stop                      Stop proxy daemon
tncli proxy status                    Show routes
tncli proxy install                   Install launchd daemon
tncli proxy uninstall                 Remove launchd daemon
```

## Key Data Structures

### Config (tncli.yml)

```
Config
├── session: String              tmux session name
├── default_branch: String       e.g. "main"
├── shared_services: {name → SharedServiceDef}
│   ├── image, host, ports, environment
│   ├── healthcheck, volumes, command
│   ├── db_user, db_password     for auto DB creation
│   └── capacity                 slots per instance (Redis: 16)
└── repos: {name → Dir}
    ├── alias                    short name (e.g. "api")
    ├── proxy_port               reverse proxy port
    ├── services: {name → Service}
    │   └── cmd, env, pre_start, shortcuts
    └── worktree: WorktreeConfig
        ├── copy                 files to copy from main
        ├── compose_files        docker-compose files
        ├── env_files            [".env.local", {file, env}]
        ├── env                  templates: {{bind_ip}}, {{branch_safe}}, etc.
        ├── service_overrides    disable/limit docker services
        ├── shared_services      refs to top-level services
        ├── setup                commands on create
        └── pre_delete           commands before delete
```

### Template Variables

| Variable | Example | Description |
|----------|---------|-------------|
| `{{bind_ip}}` | `127.0.0.2` | Workspace loopback IP |
| `{{branch_safe}}` | `feat_login` | Branch with `/`→`_`, `-`→`_` |
| `{{branch}}` | `feat/login` | Raw branch name |
| `{{slot:SERVICE}}` | `2` | Allocated slot (Redis DB index) |

## State Files (~/.tncli/)

| File | Purpose |
|------|---------|
| `loopback.json` | IP allocations: `{ws-key → "127.0.0.x"}` |
| `proxy-routes.json` | Proxy route table: `{hostname:port → ip:port}` |
| `shared_slots.json` | Service slot allocations (Redis DB indexes) |
| `proxy.pid` | Proxy daemon PID |
| `proxy.log` | Proxy daemon log |
| `node-bind-host.js` | Node.js monkey-patch for BIND_IP |
| `loopback.lock` | File lock for IP allocation |
| `slots.lock` | File lock for slot allocation |
| `active/` | Active pipeline markers |
| `pipeline-*.json` | Pipeline state for resume |

## Pipeline Stages

### Create (7 stages)

```
1. Validate     Check /etc/hosts, config validity
2. Provision    Allocate IP (127.0.0.x), service slots
3. Infra        Start shared services, create databases
4. Source       Create git worktrees (parallel)
5. Configure    Generate compose overrides + env files (parallel)
6. Setup        Run setup commands (parallel, in tmux)
7. Network      Create Docker network, register proxy routes
```

### Delete (5 stages)

```
1. Stop         Stop tmux windows
2. Release      Release IP + slots
3. Cleanup      Run pre_delete commands
4. Remove       Remove worktrees, drop databases
5. Finalize     Remove Docker network, delete folder, unregister proxy routes
```

## Networking Architecture

### IP Allocation

Each workspace gets a unique loopback IP from `127.0.0.2` to `127.0.0.254`. Main workspace also uses an allocated IP (no longer hardcoded `127.0.0.1`). `127.0.0.1` is reserved for the reverse proxy.

```
127.0.0.1    → reverse proxy (listens here)
127.0.0.2    → ws-main
127.0.0.3    → ws-feat-login
127.0.0.4    → ws-fix-bug-123
```

### Reverse Proxy

TCP proxy with HTTP Host header sniffing. Routes requests to the correct workspace.

```
Docker container (boompay-api)
  → http://comm.ws-main.tncli.local:17002
  → DNS: *.tncli.local → 127.0.0.1 (dnsmasq)
  → extra_hosts: comm.tncli.local → host-gateway
  ↓
Proxy (127.0.0.1:17002)
  → Host header: "comm.ws-main.tncli.local"
  → route lookup → 127.0.0.2:17002
  ↓
communication-service (127.0.0.2:17002)
```

**Hostname convention:** `{alias}.ws-{branch_safe}.tncli.local`

**Route table** (`~/.tncli/proxy-routes.json`):
```json
{
  "listen_ports": [17002, 8000],
  "routes": {
    "comm.ws-main.tncli.local:17002": "127.0.0.2:17002",
    "comm.ws-feat_login.tncli.local:17002": "127.0.0.3:17002",
    "ai.ws-main.tncli.local:8000": "127.0.0.2:8000"
  }
}
```

**Features:**
- Supports HTTP, WebSocket (Connection: Upgrade), gRPC
- Polls route file every 5s for new ports/routes
- Reloads on SIGHUP
- Daemon managed via launchd (macOS)

### DNS (dnsmasq)

Wildcard `*.tncli.local → 127.0.0.1` via dnsmasq.

```
Application → *.tncli.local
           ↓
macOS resolver → /etc/resolver/tncli.local → nameserver 127.0.0.1
              ↓
dnsmasq (port 53) → address=/tncli.local/127.0.0.1
              ↓
Returns 127.0.0.1
```

Setup once via `tncli setup` (requires sudo for `/etc/resolver/` and dnsmasq port 53).

### Docker Integration

Each workspace generates `docker-compose.override.yml` per repo:
- Port bindings: `{bind_ip}:{host_port}:{container_port}`
- Extra hosts: shared services (`postgres.local:host-gateway`) + proxy (`comm.tncli.local:host-gateway`)
- Network: workspace Docker network (`tncli-ws-{branch}`)
- Environment: resolved template variables

### Shared Services

Global docker-compose services (postgres, redis, minio, opensearch) shared across workspaces:
- Fixed ports on host (e.g. `19305:5432`)
- Per-workspace databases (`boom_feat_login`, `boom_main`)
- Capacity-based slot allocation (Redis: 16 DB indexes per instance)
- Auto-scale: new instances when capacity exceeded

## Dependencies

| Crate | Purpose |
|-------|---------|
| clap | CLI argument parsing |
| serde + serde_yaml | Config deserialization |
| serde_json | State file JSON |
| indexmap | Ordered maps |
| ratatui + crossterm | Terminal UI |
| tokio | Async runtime (proxy) |
| anyhow | Error handling |
| color-eyre | Panic handler |
