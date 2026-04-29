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
default_branch: main    # global default (used for workspace folder name)

# Shared infrastructure (single instances reused across all workspaces)
shared_services:
  postgres:
    image: postgres:16
    host: postgres.local
    ports: ["19305:5432"]
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
    volumes: ["shared_postgres:/var/lib/postgresql/data"]
    db_user: postgres
    db_password: postgres

  redis:
    image: redis:7-alpine
    host: redis.local
    ports: ["19307:6379"]
    capacity: 16  # auto-scales when slots exhausted

# Repos and their services
repos:
  my-api:
    alias: api
    default_branch: master   # override per repo (if different from global)
    worktree:
      copy: [.env, .env.secrets]
      compose_files: [docker-compose.yml]
      env_file: ".env.development.local"
      env:
        DATABASE_URL: "postgres://postgres:postgres@postgres.local:19305/myapp_{{branch_safe}}"
        REDIS_URL: "redis://redis.local:19307/{{slot:redis}}"
      service_overrides:
        local_postgres:
          profiles: ["disabled"]   # disable local postgres, use shared
      shared_services:
        - redis
        - postgres:
            db_name: "myapp_{{branch_safe}}"
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
        NEXT_PUBLIC_API_URL: "http://{{bind_ip}}:3000"
      env_file: ".env.local"
      setup:
        - npm install
    services:
      web:
        cmd: npm run dev
```

If no `workspaces` or `combinations` are defined, tncli auto-generates one workspace from all repos.

### Config Reference

#### Repo fields

| Field | Description |
|-------|-------------|
| `alias` | Short name (used in combinations and TUI display) |
| `default_branch` | Override global default branch for this repo |
| `pre_start` | Command to run before any service (e.g. `nvm use`) |
| `worktree.copy` | Files to copy from repo to worktree (e.g. `.env`) |
| `worktree.compose_files` | Docker compose files for this repo |
| `worktree.env_file` | File to write env overrides (e.g. `.env.local`) |
| `worktree.env` | Env overrides (`{{bind_ip}}`, `{{branch_safe}}`, `{{slot:SERVICE}}`) |
| `worktree.shared_services` | Shared services this repo needs |
| `worktree.service_overrides` | Docker compose service overrides (disable/limit) |
| `worktree.setup` | Commands to run after creating worktree |
| `worktree.pre_delete` | Commands to run before deleting worktree |
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

| Variable | Resolves to | Example |
|----------|-------------|---------|
| `{{bind_ip}}` | Allocated loopback IP | `127.0.0.4` |
| `{{branch_safe}}` | Branch with `/`→`_`, `-`→`_` | `feat_login` |
| `{{branch}}` | Raw branch name | `feat/login` |
| `{{slot:SERVICE}}` | Allocated slot for a shared service | `3` |

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
- **main**: workspace folder (`workspace--{default_branch}/`) containing your actual repos (moved there on first run)
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
| `b` | Branch: pull (main/instance) or menu (worktree dir) |
| `w` | Create workspace (on combo row) / worktree menu |
| `d` | Delete workspace (with confirm) |
| `t` | Open shell in directory |
| `I` | Shared services info (status, hosts, ports) |
| `R` | Reload config |
| `Tab` / `l` | Focus log panel |

**Right panel (logs):**

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll down/up |
| `G` / `g` | Jump to bottom/top |
| `/` | Search in logs |
| `n` / `N` | Next/prev search match (if searching) or cycle running services |
| `i` | Interactive mode (send keys to pane) |
| `y` | Copy mode (fullscreen, mouse disabled for selection) |
| `Tab` / `h` | Focus back to left panel |

**Global:**

| Key | Action |
|-----|--------|
| `a` | Attach to tmux session |
| `?` | Keybindings cheat-sheet |
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

## How It Works

### Overview

```
                          tncli.yml
                              │
              ┌───────────────┼───────────────┐
              │               │               │
         ┌────▼────┐    ┌────▼────┐    ┌─────▼─────┐
         │  repos   │    │ shared  │    │ workspaces │
         │ configs  │    │services │    │  (combos)  │
         └────┬────┘    └────┬────┘    └─────┬─────┘
              │              │               │
              ▼              ▼               ▼
    ┌──────────────┐  ┌──────────┐  ┌──────────────┐
    │ workspace--  │  │ postgres │  │  workspace-- │
    │  {default}   │  │ redis    │  │  {branch}    │
    │ (main repos) │  │ minio    │  │ (worktrees)  │
    └──────┬───────┘  └─────┬────┘  └──────┬───────┘
           │                │              │
           └────────┬───────┘──────────────┘
                    │
              ┌─────▼─────┐
              │   tmux     │
              │  session   │
              │ (1 window  │
              │ per service│
              └────────────┘
```

### Directory Structure

When tncli starts, it automatically organizes repos into workspace folders:

```
project-root/
├── tncli.yml                          # config (stays here)
├── docker-compose.shared.yml          # generated: shared services
│
├── workspace--main/                   # main workspace (auto-created)
│   ├── my-api/                        # real repo (moved here on first run)
│   │   ├── src/
│   │   ├── docker-compose.yml
│   │   ├── docker-compose.override.yml  ← generated
│   │   └── .env.tncli                   ← generated
│   └── my-client/                     # real repo (moved here)
│       ├── src/
│       └── .env.tncli                   ← generated
│
├── workspace--feat-123/               # branch workspace (git worktrees)
│   ├── my-api/                        # git worktree → own branch
│   │   ├── src/
│   │   ├── docker-compose.override.yml  ← generated (isolated ports)
│   │   └── .env.tncli                   ← generated (unique IP)
│   └── my-client/                     # git worktree → own branch
│       └── .env.tncli
│
└── workspace--fix-456/                # another branch workspace
    ├── my-api/
    └── my-client/
```

On first run, tncli moves repos from `project-root/{repo}/` into `workspace--{default_branch}/{repo}/`. This is a one-time migration. Git worktree references are automatically fixed.

### Service Lifecycle

```
                    ┌──────────┐
                    │ tncli.yml│
                    └────┬─────┘
                         │ parse
                         ▼
                 ┌───────────────┐
                 │ resolve_service│
                 │ (dir + cmd +  │
                 │  env + hooks) │
                 └───────┬───────┘
                         │
              ┌──────────▼──────────┐
              │ ensure_main_ready   │
              │ (background thread) │
              └──────────┬──────────┘
                         │
         ┌───────────────┼───────────────┐
         │               │               │
    ┌────▼────┐    ┌─────▼─────┐   ┌─────▼─────┐
    │ start   │    │ generate  │   │  create   │
    │ shared  │    │ compose   │   │ databases │
    │services │    │ override  │   │ (if new)  │
    └────┬────┘    └─────┬─────┘   └─────┬─────┘
         │               │               │
         └───────────────┼───────────────┘
                         │
                  ┌──────▼──────┐
                  │ tmux window │
                  │ cd $dir &&  │
                  │ $pre_start  │
                  │ && $cmd     │
                  └─────────────┘
```

### Workspace Creation Pipeline

Creating a workspace (pressing `w` on a combo row) runs a 7-stage pipeline:

```
 ┌─────────────────────────────────────────────────────────┐
 │ Stage 1: Validate                                       │
 │   Check /etc/hosts entries for shared service hostnames │
 ├─────────────────────────────────────────────────────────┤
 │ Stage 2: Provision                                      │
 │   Allocate loopback IP (127.0.0.x)                     │
 │   Allocate shared service slots (Redis DB index, etc.) │
 │   Create workspace--{branch}/ folder                    │
 ├─────────────────────────────────────────────────────────┤
 │ Stage 3: Infra                                          │
 │   Start shared services (postgres, redis, minio...)    │
 │   Create per-workspace databases                        │
 │   Setup main workspace (compose override + env)        │
 ├─────────────────────────────────────────────────────────┤
 │ Stage 4: Source                                         │
 │   git worktree add for each repo → workspace folder    │
 │   Copy config files (.env, .env.secrets, etc.)         │
 ├─────────────────────────────────────────────────────────┤
 │ Stage 5: Configure                                      │
 │   Generate docker-compose.override.yml (port binding)  │
 │   Write .env.tncli (BIND_IP)                           │
 │   Write env_file (resolved env templates)              │
 │     ↳ filename from worktree.env_file config           │
 ├─────────────────────────────────────────────────────────┤
 │ Stage 6: Setup                                          │
 │   Run setup commands (npm install, db:migrate, etc.)   │
 ├─────────────────────────────────────────────────────────┤
 │ Stage 7: Network                                        │
 │   Create Docker network for cross-service communication│
 │   Regenerate compose overrides with network attached    │
 └─────────────────────────────────────────────────────────┘
```

### Port & Database Isolation

Each workspace gets a unique loopback IP. All services bind to that IP, so multiple workspaces use the same ports without conflicts:

```
                    Shared Infrastructure
                    ┌─────────────────────┐
                    │ postgres.local:19305│──┐
                    │ redis.local:19307   │  │ one instance,
                    │ minio.local:19309   │  │ many databases/slots
                    └─────────────────────┘  │
                             │               │
          ┌──────────────────┼───────────────┘
          │                  │
   ┌──────▼──────┐   ┌──────▼──────┐   ┌─────────────┐
   │   main      │   │  feat-123   │   │  fix-456    │
   │ 127.0.0.1   │   │ 127.0.0.4   │   │ 127.0.0.5   │
   │             │   │             │   │             │
   │ DB: app_main│   │ DB: app_    │   │ DB: app_    │
   │ Redis: /0   │   │ feat_123    │   │ fix_456     │
   │             │   │ Redis: /1   │   │ Redis: /2   │
   │ :3000 → api │   │ :3000 → api│   │ :3000 → api│
   │ :3001 → web │   │ :3001 → web│   │ :3001 → web│
   └─────────────┘   └─────────────┘   └─────────────┘
```

### Template Variables

Environment values in `tncli.yml` support these templates:

| Template | Resolves to | Example |
|----------|-------------|---------|
| `{{bind_ip}}` | Allocated loopback IP | `127.0.0.4` |
| `{{branch_safe}}` | Branch with `/`→`_`, `-`→`_` | `feat_login_page` |
| `{{branch}}` | Raw branch name | `feat/login-page` |
| `{{slot:SERVICE}}` | Allocated slot index for a shared service | `3` |

Example config:
```yaml
repos:
  my-api:
    worktree:
      env:
        DATABASE_URL: "postgres://postgres:postgres@postgres.local:19305/myapp_{{branch_safe}}"
        REDIS_URL: "redis://redis.local:19307/{{slot:redis}}"
        API_BASE: "http://{{bind_ip}}:3000"
      shared_services:
        - redis
        - postgres:
            db_name: "myapp_{{branch_safe}}"
```

For the `main` workspace, `{{slot:SERVICE}}` resolves to `0` (default). For branch workspaces, slots are auto-allocated during the Provision stage and persisted in `~/.tncli/slots.json`.

### Branch Management

The `b` key behaves differently based on context:

| Context | Action |
|---------|--------|
| **Main instance** (row) | Pull `origin/{default_branch}` for all repos |
| **Main dir/service** | Pull `origin/{default_branch}` for that repo |
| **Branch instance** (row) | Pull `origin/{branch}` for all repos in worktree |
| **Branch dir/service** | Open branch menu (checkout / create / pull) |

## Architecture

Single Rust binary. Each service runs in a tmux window within a shared session.

```
src/
├── main.rs              # CLI entry point (clap)
├── config.rs            # YAML config loading + template resolution
├── commands.rs          # CLI command implementations
├── tmux.rs              # tmux subprocess wrappers
├── lock.rs              # file-based lock management
├── services/            # domain logic (no TUI dependency)
│   ├── mod.rs           # re-exports + env template resolver
│   ├── compose.rs       # docker-compose override generation
│   ├── docker.rs        # Docker network/workspace folder management
│   ├── files.rs         # .env file generation + file copy
│   ├── git.rs           # git worktree create/remove/list
│   ├── ip.rs            # loopback IP allocation + /etc/hosts
│   └── workspace.rs     # slot allocation + shared compose generation
├── pipeline/            # workspace lifecycle (create/delete)
│   ├── mod.rs           # pipeline runner + event types
│   ├── context.rs       # pipeline context (decoupled from App)
│   ├── stages.rs        # stage definitions
│   ├── create.rs        # 7-stage create pipeline
│   └── delete.rs        # 5-stage delete pipeline
└── tui/
    ├── mod.rs           # main loop + panic handler
    ├── app.rs           # application state + path resolution
    ├── event.rs         # event thread + key/mouse handlers
    ├── ui.rs            # ratatui rendering
    ├── ansi.rs          # ANSI escape code parser
    └── screens/         # screen-specific logic
        ├── logs.rs      # log panel navigation + service cycling
        ├── services.rs  # start/stop/restart services
        ├── tree.rs      # workspace tree building + scan
        └── workspace.rs # workspace create/delete from TUI
```

## Release

```bash
make patch         # 0.1.0 → 0.1.1
make minor         # 0.1.0 → 0.2.0
make major         # 0.1.0 → 1.0.0
```

Bumps version, commits, tags, pushes. GitHub Actions builds all platforms automatically.
