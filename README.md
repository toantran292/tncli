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

```bash
make build         # debug build
make release       # optimized release
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
2. Run `tncli setup` (one-time: loopback IPs + /etc/hosts)
3. Run `tncli` to open TUI

## Config

`tncli.yml` defines your repos, services, shared infrastructure, and workspace combinations.

```yaml
session: myproject

# Shared infrastructure (single instances reused across all workspaces)
shared_services:
  postgres:
    image: postgres:16
    host: postgres.local
    ports: ["5432:5432"]
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
    volumes: ["shared_postgres:/var/lib/postgresql/data"]
    db_user: postgres
    db_password: postgres

  redis:
    image: redis:7-alpine
    host: redis.local
    ports: ["6379:6379"]
    capacity: 16  # auto-scales when slots exhausted

# Repos and their services
repos:
  my-api:
    alias: api
    worktree: true
    worktree_copy: [.env, .env.secrets]
    compose_files: [docker-compose.yml]
    worktree_env_file: ".env.development.local"
    worktree_env:
      DATABASE_URL: "postgres://postgres:postgres@postgres.local:5432/myapp_{{branch_safe}}"
      REDIS_URL: "redis://redis.local:6379/0"
    worktree_shared_services:
      - redis
      - postgres:
          db_name: "myapp_{{branch_safe}}"
    worktree_setup:
      - bundle install
      - rake db:migrate
    worktree_pre_delete:
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
    worktree_env:
      NEXT_PUBLIC_API_URL: "http://{{bind_ip}}:3000"
    worktree_env_file: ".env.local"
    worktree_setup:
      - npm install
    services:
      web:
        cmd: npm run dev

# Workspace combinations
combinations:
  fullstack:
    - api: api, worker
    - client: web
```

### Config Reference

#### Repo fields

| Field | Description |
|-------|-------------|
| `alias` | Short name (used in combinations and TUI display) |
| `worktree` | Enable git worktree support (`true`/`false`) |
| `pre_start` | Command to run before any service (e.g. `nvm use`) |
| `compose_files` | Docker compose files for this repo |
| `worktree_copy` | Files to copy from repo to worktree (e.g. `.env`) |
| `worktree_env_file` | File to write env overrides (e.g. `.env.local`) |
| `worktree_env` | Environment overrides per worktree (supports `{{bind_ip}}`, `{{branch_safe}}`) |
| `worktree_shared_services` | Shared services this repo needs |
| `worktree_service_overrides` | Docker compose service overrides (e.g. disable services) |
| `worktree_setup` | Commands to run after creating worktree |
| `worktree_pre_delete` | Commands to run before deleting worktree |
| `shortcuts` | Quick commands accessible via `c` key |
| `services` | Named services with `cmd`, optional `env`, `pre_start` |

#### Shared service fields

| Field | Description |
|-------|-------------|
| `image` | Docker image |
| `host` | Hostname for resolution (added to `/etc/hosts`) |
| `ports` | Port mappings |
| `environment` | Container environment variables |
| `volumes` | Volume mounts |
| `command` | Override container command |
| `healthcheck` | Health check config (`test`, `interval`, `timeout`, `retries`) |
| `db_user` / `db_password` | Credentials for auto database creation |
| `capacity` | Max slots per instance (auto-scales when exceeded) |

#### Template variables

| Variable | Resolves to |
|----------|-------------|
| `{{bind_ip}}` | Allocated loopback IP (e.g. `127.0.0.4`) |
| `{{branch_safe}}` | Branch name with `/` and `-` replaced by `_` |
| `{{branch}}` | Raw branch name |

#### Combination format

```yaml
combinations:
  fullstack:
    - api: api, worker        # alias: svc1, svc2 (compact)
    - client: web
    - api/api                  # alias/service (explicit)
```

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
tncli setup                             # one-time: loopback IPs + /etc/hosts + gitignore
tncli workspace create <combo> <branch> # create workspace
tncli workspace delete <branch>         # delete workspace
tncli workspace list                    # list workspaces with details
tncli update                            # update to latest release
```

## TUI

Interactive terminal interface. Left panel shows workspaces, right panel shows logs.

```
┌─ Workspaces ──────┬─ (main) logs: api ───────────────────┐
│ ○ fullstack  0/3  │ => Booting Puma                       │
│   main       2/3  │ * Listening on tcp://0.0.0.0:3000     │
│   ├─ api 2/2      │ Started GET "/api/v1/..."             │
│   │  ├─ ● api     │ Completed 200 OK in 12ms              │
│   │  └─ ● worker  │                                       │
│   └─ client 0/1   │                                       │
│      └─ ○ web     │                                       │
│   feat-123   3/3  │                                       │
│   ├─ api 2/2      │                                       │
│   │  ├─ ● api     │                                       │
│   │  └─ ● worker  │                                       │
│   └─ client 1/1   │                                       │
│      └─ ● web     │                                       │
└───────────────────┴───────────────────────────────────────┘
 enter toggle  s start  x stop  c cmds  e edit  b branch  q quit
```

### Concepts

- **Combo**: a workspace definition (e.g. "fullstack" = api + client)
- **main**: virtual instance representing your original repo directories
- **Instances**: git worktree-based copies (e.g. "feat-123") with isolated branches, databases, and ports

### Keyboard

**Left panel:**

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `Enter` / `Space` | Toggle start/stop or expand/collapse |
| `s` | Start service/instance |
| `x` | Stop service/instance |
| `X` | Stop all (with confirm) |
| `r` | Restart |
| `c` | Shortcuts popup |
| `e` | Open in editor (zed/vscode) |
| `b` | Branch menu (checkout/create/fetch) |
| `w` | Create workspace (on combo row) / worktree menu |
| `d` | Delete workspace (with confirm) |
| `t` | Open shell in directory |
| `R` | Reload config |
| `Tab` / `l` | Focus log panel |

**Right panel (logs):**

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll down/up |
| `G` / `g` | Jump to bottom/top |
| `/` | Search in logs |
| `n` / `N` | Next/previous match |
| `i` | Interactive mode (send keys to pane) |
| `y` | Copy mode (fullscreen, mouse disabled for selection) |
| `Tab` / `h` | Focus back to left panel |

**Global:**

| Key | Action |
|-----|--------|
| `a` | Attach to tmux session |
| `q` | Quit |

### Mouse

- Click to select items or focus panels
- Scroll to navigate list (left) or scroll logs (right)

### Status icons

| Icon | Meaning |
|------|---------|
| `●` | Running |
| `◐` | Partially running |
| `○` | Stopped |
| `~` | Creating/deleting (background) |

## Workspaces

Workspaces let you run multiple copies of your project simultaneously, each on its own git branch with isolated databases and ports.

### How it works

1. **Create**: `w` on combo row → enter branch name
2. tncli creates git worktrees for each repo in the combo
3. Allocates a unique loopback IP (e.g. `127.0.0.5`)
4. Starts shared services (postgres, redis, etc.)
5. Creates per-workspace databases
6. Runs setup commands (install deps, migrate)
7. Generates docker-compose overrides for port isolation

### Port isolation

Each workspace gets a unique loopback IP. Services bind to that IP, so multiple workspaces can use the same ports without conflict:

- main: `127.0.0.1:3000`
- feat-123: `127.0.0.4:3000`
- fix-456: `127.0.0.5:3000`

### Shared services

Infrastructure (postgres, redis, minio, etc.) runs once and is shared across all workspaces. Each workspace gets its own database on the shared postgres instance.

When a capacity-limited service (e.g. Redis with 16 db indexes) runs out of slots, tncli automatically starts additional instances with incremented ports.

### Setup (one-time)

```bash
tncli setup
```

This command:
- Creates loopback IPs (`127.0.0.2` through `127.0.0.100`)
- Adds shared service hostnames to `/etc/hosts`
- Configures global gitignore for generated files

## Architecture

Single Rust binary. Each service runs in a tmux window within a shared session.

```
src/
├── main.rs          # CLI entry point (clap)
├── config.rs        # YAML config loading
├── commands.rs      # CLI command implementations
├── tmux.rs          # tmux subprocess wrappers
├── worktree.rs      # git worktree + docker compose + loopback management
├── lock.rs          # file-based lock management
└── tui/
    ├── mod.rs       # TUI main loop + panic handler
    ├── app.rs       # application state + workspace logic
    ├── event.rs     # event thread + key/mouse handlers
    ├── ui.rs        # ratatui rendering
    └── ansi.rs      # ANSI escape code parser
```

## Release

```bash
make patch         # 0.1.0 → 0.1.1
make minor         # 0.1.0 → 0.2.0
make major         # 0.1.0 → 1.0.0
```

Bumps version, commits, tags, pushes. GitHub Actions builds all platforms automatically.
