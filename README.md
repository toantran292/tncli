# tncli

tmux-based workspace manager for multi-repo projects. Define services, shared infrastructure, and workspace combinations in YAML. Manage everything through an interactive TUI or CLI commands.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/toantran292/tncli/main/install.sh | bash
```

Update to latest:

```bash
tncli update
```

### Supported platforms

| Platform | Architecture | Binary |
|----------|-------------|--------|
| macOS | Apple Silicon (M1/M2/M3/M4) | `tncli-darwin-arm64` |
| macOS | Intel | `tncli-darwin-amd64` |
| Linux | x86_64 | `tncli-linux-amd64` |
| Linux | ARM64 | `tncli-linux-arm64` |

### Build from source

Requires Go 1.26+, tmux, docker.

```bash
make build         # dev build
make release       # optimized + codesign (macOS)
make install       # release + copy to /usr/local/bin
```

### Dependencies

- `tmux` (3.x+)
- `zsh`
- `docker` (for shared services)

```bash
# macOS
brew install tmux

# Ubuntu/Debian
sudo apt install tmux zsh
```

## Quick Start

1. Create `tncli.yml` at your project root
2. Run `tncli setup` (one-time: /etc/hosts for shared services + gitignore)
3. Run `tncli` to open TUI

## Config

`tncli.yml` defines your repos, services, shared infrastructure, and workspace combinations.

```yaml
session: myproject
default_branch: main

shared_services:
  postgres:
    image: postgres:16
    ports: ["5432"]                # container port only — host port is dynamic
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
    volumes: ["shared_postgres:/var/lib/postgresql/data"]
    db_user: postgres
    db_password: postgres

  redis:
    image: redis:7-alpine
    ports: ["6379"]
    capacity: 16                   # auto-scales when slots exhausted

repos:
  my-api:
    alias: api
    default_branch: master
    worktree:
      copy: [.env, .env.secrets]
      compose_files: [docker-compose.yml]
      env_files: ".env.development.local"
      env:
        DATABASE_URL: "postgres://{{conn:postgres}}/myapp_{{branch_safe}}"
        REDIS_URL: "redis://{{host:redis}}:{{port:redis}}/{{slot:redis}}"
      service_overrides:
        local_postgres:
          profiles: ["disabled"]
      shared_services:
        - redis
        - postgres
      databases:
        - "{{branch_safe}}"
      setup:
        - bundle install
        - rake db:migrate
      pre_delete:
        - docker compose down -v
    shortcuts:
      - cmd: bundle install
        desc: Install dependencies
      - cmd: rake db:migrate
        desc: Migrate database
    services:
      api:
        cmd: bundle exec rails server
      worker:
        cmd: bundle exec sidekiq

  my-client:
    alias: client
    worktree:
      env:
        NEXT_PUBLIC_API_URL: "{{url:my-api}}"
      env_files: ".env.local"
      setup:
        - npm install
    services:
      web:
        cmd: npm run dev
```

### Config Reference

#### Repo fields

| Field | Description |
|-------|-------------|
| `alias` | Short name (used in combinations and TUI display) |
| `default_branch` | Override global default branch for this repo |
| `pre_start` | Command to run before any service (e.g. `nvm use`) |
| `worktree.copy` | Files to copy from repo to worktree (e.g. `.env`) |
| `worktree.compose_files` | Docker compose files for this repo |
| `worktree.env_files` | Files to write env overrides (e.g. `.env.local`) |
| `worktree.env` | Env templates (`{{host:NAME}}`, `{{port:NAME}}`, `{{slot:SERVICE}}`) |
| `worktree.shared_services` | Shared services this repo needs |
| `worktree.service_overrides` | Docker compose service overrides (disable/limit) |
| `worktree.databases` | Database name templates (auto-created per workspace) |
| `worktree.setup` | Commands to run after creating worktree |
| `worktree.pre_delete` | Commands to run before deleting worktree |
| `shortcuts` | Quick commands accessible via `c` key |
| `services` | Named services with `cmd`, optional `env`, `pre_start` |

#### Shared service fields

| Field | Description |
|-------|-------------|
| `image` | Docker image |
| `ports` | Container ports (host ports are dynamically allocated) |
| `environment` | Container environment variables |
| `volumes` | Volume mounts |
| `command` | Override container command |
| `healthcheck` | Health check config (`test`, `interval`, `timeout`, `retries`) |
| `db_user` / `db_password` | Credentials for auto database creation |
| `capacity` | Max slots per instance (auto-scales when exceeded) |

#### Template variables

| Template | Resolves to | Example |
|----------|-------------|---------|
| `{{host:NAME}}` | Shared service name (resolved via /etc/hosts) | `postgres` |
| `{{port:NAME}}` | Dynamic port (shared service or repo proxy_port) | `44800` |
| `{{url:NAME}}` | `http://{host}:{port}` | `http://postgres:44800` |
| `{{conn:NAME}}` | `user:pass@host:port` (from shared service creds) | `postgres:postgres@postgres:44800` |
| `{{db:N}}` | Nth database name (session-prefixed) | `myproject_feat_login` |
| `{{slot:SERVICE}}` | Allocated slot for capacity-limited service | `3` |
| `{{bind_ip}}` | Always `127.0.0.1` | `127.0.0.1` |
| `{{branch_safe}}` | Branch with `/`→`_`, `-`→`_` | `feat_login` |
| `{{branch}}` | Raw branch name | `feat/login` |

## CLI Usage

```bash
tncli                                   # open TUI (default)
tncli start <service|combo>             # start services
tncli stop [service|combo]              # stop (no arg = stop all)
tncli restart <service|combo>           # restart
tncli status                            # show running services
tncli list                              # list services and workspaces
tncli attach [service]                  # attach to tmux session
tncli logs <service>                    # show recent output
tncli setup                             # one-time: /etc/hosts + gitignore
tncli migrate                           # migrate from old IP-based system
tncli workspace create <combo> <branch> # create workspace
tncli workspace delete <branch>         # delete workspace
tncli workspace list                    # list workspaces with details
tncli db reset <branch>                 # drop + recreate databases
tncli update                            # update to latest release
```

## TUI

Interactive terminal interface. Left panel shows workspaces, right panel shows logs.

```
┌─ myproject ───────┬─ logs: api~api [1/2] ────────────────┐
│▾● main       2/5  │ => Booting Puma                       │
│ ├ ● api      2/2  │ * Listening on tcp://127.0.0.1:3000   │
│ │ ├ ● api         │ Started GET "/api/v1/..."             │
│ │ └ ● worker      │ Completed 200 OK in 12ms              │
│ └ ○ client   0/1  │                                       │
│   └ ○ web         │                                       │
│▾● feat-123   3/3  │                                       │
│ ├ ● api      2/2  │                                       │
│ └ ● client   1/1  │                                       │
│▸○ fix-456    0/3  │                                       │
└───────────────────┴───────────────────────────────────────┘
 s start  x stop  c cmds  e edit  b branch  w wt/ws  ? help
```

### Keyboard

**Left panel:** `j/k` navigate, `Enter/Space` toggle, `s` start, `x` stop, `X` stop all, `r` restart, `c` shortcuts, `e` editor, `b` branch, `w` workspace, `d` delete, `t` shell, `I` shared info, `R` reload, `Tab` focus log

**Right panel:** `j/k` scroll, `G/g` bottom/top, `/` search, `n/N` next/prev, `i` interactive, `y` copy mode, `Tab` back

**Global:** `a` attach tmux, `?` help, `q` quit

## Port Allocation

No hardcoded ports, no loopback IPs, no sudo for port setup.

```
Pool: 40000-49999 (10,000 ports)
├── Slot 0 (session A): 40000-44999
│   ├── Workspace blocks: 40000-44799 (48 blocks × 100 ports)
│   └── Shared services:  44800-44999 (200 ports)
└── Slot 1 (session B): 45000-49999
```

Each workspace gets a 100-port block. Each service gets a stable index within its block. Shared services get dynamic ports from the top of the slot.

Templates like `{{port:postgres}}` resolve to the allocated port automatically. Same URL works everywhere: browser, host process, Docker container.

## Workspaces

Workspaces let you run multiple copies of your project simultaneously, each on its own git branch with isolated databases and ports.

### How it works

1. **Create**: `w` on main/combo row → enter branch name → repo selection
2. Pipeline runs 7 stages: validate → provision → infra → source → configure → setup → network
3. Stages 4-6 run per-repo in **parallel**
4. After creation: `w` on workspace instance → add/remove repos

### Port & Database Isolation

Each workspace gets its own port block and databases:

```
Shared Infrastructure (dynamic ports)
┌──────────────────────────────┐
│ postgres :44800              │
│ redis    :44801              │  one instance,
│ minio    :44802              │  many databases/slots
└──────────────────────────────┘
         │
         ├─── main workspace
         │    DB: myproject_main
         │    Redis: /0
         │    Ports: 40000-40099
         │
         ├─── feat-123 workspace
         │    DB: myproject_feat_123
         │    Redis: /1
         │    Ports: 40100-40199
         │
         └─── fix-456 workspace
              DB: myproject_fix_456
              Redis: /2
              Ports: 40200-40299
```

### Setup (one-time)

```bash
tncli setup
```

Adds shared service names to `/etc/hosts` (requires sudo) and configures global gitignore.

### Migrating from old system

```bash
tncli migrate
```

Cleans old state files (Caddy, loopback IPs, proxy routes), re-initializes network state with dynamic ports, regenerates env files and compose overrides for all existing workspaces.

## Architecture

Go single-binary CLI+TUI. Each service runs in a tmux window within a shared session.

```
cmd/tncli/                 CLI entry (cobra dispatch)
internal/
  config/                  YAML parsing, service resolution
  commands/                CLI command implementations
  services/                Infrastructure (docker, git, network, compose, files)
  pipeline/                Workspace lifecycle (7 create stages, 5 delete stages)
  tmux/                    tmux subprocess wrapper
  tui/                     Terminal UI (bubbletea)
  popup/                   Popup dialogs
  lock/                    Lock file management
```

## Release

```bash
make patch         # 0.5.0 → 0.5.1
make minor         # 0.5.0 → 0.6.0
make major         # 0.5.0 → 1.0.0
```

Bumps version, commits, tags, pushes. GitHub Actions builds all platforms automatically.
