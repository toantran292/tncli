# Architecture

## Overview

Go single-binary CLI+TUI for managing multi-repo dev environments via tmux.

```
┌─────────────────────────────────────────────────────────────────┐
│                         tncli.yml                               │
│  session, repos, services, shared_services, combinations        │
└──────────────────────────┬──────────────────────────────────────┘
                           │
              ┌────────────▼────────────┐
              │    cmd/tncli/root.go     │
              │  cobra dispatch + config │
              │  load + InitNetwork()    │
              └────────────┬────────────┘
                           │
         ┌─────────────────┼─────────────────┐
         │                 │                  │
    ┌────▼────┐      ┌─────▼─────┐     ┌─────▼─────┐
    │   CLI    │      │    TUI    │     │ Pipeline  │
    │commands/ │      │   tui/    │     │ pipeline/ │
    │          │      │bubbletea  │     │ 7 create  │
    │ start    │      │           │     │ 5 delete  │
    │ stop     │      │ model.go  │     │           │
    │ restart  │      │ actions   │     │ runner.go │
    │ status   │      │ render    │     │ stages    │
    │ attach   │      │ tree      │     │ context   │
    │ logs     │      │ popups    │     │           │
    │ workspace│      │ split     │     │           │
    │ db       │      │           │     │           │
    │ setup    │      │           │     │           │
    │ migrate  │      │           │     │           │
    │ update   │      │           │     │           │
    └────┬─────┘      └─────┬─────┘     └─────┬─────┘
         │                  │                  │
         └──────────────────┼──────────────────┘
                            │
              ┌─────────────▼─────────────┐
              │     internal/services/     │
              │   (all side effects)       │
              ├────────────────────────────┤
              │ network.go   port alloc    │
              │ compose.go   docker-compose│
              │ docker.go    networks      │
              │ git.go       worktrees     │
              │ files.go     env files     │
              │ workspace.go shared svcs   │
              │ registry.go  project reg   │
              │ services.go  templates     │
              └─────────────┬─────────────┘
                            │
              ┌─────────────▼─────────────┐
              │      internal/tmux/        │
              │   (subprocess wrapper)     │
              │ session, window, pane      │
              │ capture, attach, send-keys │
              └─────────────┬─────────────┘
                            │
              ┌─────────────▼─────────────┐
              │     internal/paths/        │
              │   XDG state directory      │
              │ ~/.local/state/tncli/      │
              └───────────────────────────┘
```

## Data Flow

```
tncli.yml parse
     │
     ▼
InitNetwork()
  ├─ ClaimSessionSlot() → ~/.local/state/tncli/slots.json
  ├─ Build service_map  → .tncli/network.json
  └─ Build shared_map   → .tncli/network.json
     │
     ▼
Template Resolution
  {{host:NAME}}  → service name (via /etc/hosts + extra_hosts)
  {{port:NAME}}  → SharedPort() or proxy_port
  {{url:NAME}}   → http://host:port
  {{conn:NAME}}  → user:pass@host:port
  {{db:N}}       → session_branchsafe
  {{slot:NAME}}  → Redis DB index
  {{bind_ip}}    → 127.0.0.1 (.env.local) / host.docker.internal (compose)
     │
     ▼
Output
  ├─ .env.local                    → host processes
  ├─ docker-compose.override.yml   → Docker containers (127.0.0.1→host.docker.internal)
  └─ docker-compose.shared.yml     → shared services (dynamic ports)
```

## Port Allocation

No hardcoded ports, no loopback IPs, no sudo for port setup.

```
40000─49999 (10,000 ports)
├── Slot 0: 40000─44999
│   ├── Workspace blocks: 40000─44799 (48 × 100)
│   │   ├── ws-main:       40000─40099
│   │   ├── ws-feat-login: 40100─40199
│   │   └── ...
│   └── Shared services:  44800─44999 (200 ports)
│       ├── postgres:   44800 → :5432
│       ├── redis:      44801 → :6379
│       ├── minio:      44802 → :9000, 44803 → :9001
│       └── opensearch: 44804 → :9200, 44805 → :9600
└── Slot 1: 45000─49999 (same layout)
```

Formulas:
- Workspace service: `PoolStart + slot×SlotSize + blockIdx×BlockSize + svcIdx`
- Shared service: `PoolStart + slot×SlotSize + SlotSize - SharedReserve + offset`
- Multi-port: consecutive offsets per service
- Multi-instance (capacity): `SharedPort(name) + instanceIdx`

## Networking

```
Browser / Host process          Docker container (api via dip)
        │                               │
        │ resolve "postgres"            │ resolve "postgres"
        ▼                               ▼
   /etc/hosts                    extra_hosts
   127.0.0.1 postgres            postgres:host-gateway
        │                               │
        └───────── both reach ──────────┘
                      │
                127.0.0.1:44800
                      │
              ┌───────▼───────┐
              │ Docker host   │
              │ port mapping  │
              │ 44800 → 5432  │
              └───────┬───────┘
                      │
              ┌───────▼───────┐
              │   postgres    │
              │   container   │
              │   :5432       │
              └───────────────┘
```

For host-side services (comm-service, ai-mainframe):
```
Docker container → host.docker.internal:17002 → host → comm-service (tmux)
```

Same URL works from browser, host processes, and Docker containers (macOS + Linux).

## Interfaces (testable)

```
tmux.Runner           → tmux.ExecRunner (default)   → exec.Command("tmux", ...)
services.GitRunner    → ExecGitRunner (default)      → exec.Command("git", ...)
services.DockerRunner → ExecDockerRunner (default)   → exec.Command("docker", ...)
```

Package functions delegate to `Default` runner. Tests swap with mocks.

## State Files

```
~/.local/state/tncli/              (XDG_STATE_HOME/tncli/)
├── slots.json                     session slot leases (max 2)
├── shared_slots.json              capacity slot allocations (Redis DB indexes)
├── registry.json                  session → project dir mapping
├── node-bind-host.js              Node.js BIND_IP patch
├── collapse-{session}.json        TUI collapse state
├── pipeline-{branch}.json         pipeline resume state
└── active/{branch}                active pipeline markers

{project}/.tncli/
└── network.json                   slot, blocks, service_map, shared_map
```

## Workspace Lifecycle

### Create (7 stages)

```
1. Validate      → no-op (reserved)
2. Provision     → allocate shared service slots, create workspace folder
3. Infra         → generate shared compose, start containers, create databases
4. Source    ║   → git worktree add (parallel per repo)
5. Configure ║   → .env.tncli + .env.local + compose override (parallel)
6. Setup     ║   → run setup commands in tmux windows (parallel)
7. Network       → docker network create
```

### Delete (5 stages)

```
1. Stop          → no-op (caller handles)
2. Release       → free shared service slots
3. Cleanup       → run pre_delete commands
4. Remove        → git worktree remove, drop databases, release port block
5. Finalize      → docker network rm, delete workspace folder
```

### Migrate (8 steps)

```
1. XDG           → ~/.tncli/ → ~/.local/state/tncli/ (symlink left)
2. Clean         → Caddy, proxy, loopback script, stale pipelines
3. Network       → reset session slots, re-init with dynamic ports
4. Slots         → remove stale Redis slot allocations
5. System        → /etc/resolver, /etc/hosts old entries, loopback aliases (sudo)
6. /etc/hosts    → add shared service names (sudo)
7. Regenerate    → shared compose + all workspace envs + compose overrides
8. Restart       → docker compose down + up (new dynamic ports)
```

## Dependency Graph

Services support optional `depends_on` for ordered startup:

```yaml
services:
  api:
    cmd: dip server
    depends_on: [worker]
  worker:
    cmd: dip sidekiq
```

Kahn's toposort with cycle detection. Start: dependencies first. Stop: dependents first.
Transitive: requesting `api` auto-starts `worker` if needed.
