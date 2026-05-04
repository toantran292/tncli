# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
go build -o tncli ./cmd/tncli/    # Build binary
go test ./...                      # Run all tests
go vet ./...                       # Static analysis
make build                         # Same as go build
make release                       # Optimized + codesign (macOS)
```

Requires: `go` (1.26+), `tmux`, `codesign` (macOS)

## Architecture

Go single-binary CLI+TUI for managing tmux services. Config via `tncli.yml` found by walking up from CWD.

**CLI path**: `cmd/tncli/main.go` (dispatch) → `internal/services/` (business logic) → `internal/tmux/` (subprocess)

**TUI path**: `internal/tui/tui.go` (bubbletea Update/View) → `internal/tui/model.go` (state) → `internal/tui/tree.go` (tree builder)

### Go Project Layout

```
cmd/tncli/main.go              — CLI entry, command dispatch, no business logic
internal/
  config/config.go              — YAML parsing, service resolution (pure logic)
  lock/lock.go                  — Lock file management
  tmux/tmux.go                  — tmux subprocess wrapper (thin layer)
  popup/popup.go                — Popup dialogs (bubbletea sub-programs)
  services/                     — Infrastructure layer (all side effects)
    services.go                 — Template resolution, shared types
    network.go                  — Port allocation, session slots, shared ports
    compose.go                  — docker-compose override generation
    docker.go, git.go           — Docker/Git operations
    files.go                    — Env file management
    workspace.go                — Shared services compose, slot allocation
    registry.go                 — Project registry
  pipeline/                     — Workspace lifecycle (staged)
    pipeline.go                 — Events, state persistence
    stages.go, context.go       — Stage definitions, context builders
    create.go, delete.go        — Stage executors
    runner.go                   — Pipeline runner
  tui/                          — Terminal UI (bubbletea)
    model.go                    — App state, worktree scanning
    tree.go                     — Workspace tree builder
    tui.go                      — Update/View/Actions
    popups.go                   — TUI popup handlers
```

### TUI Modes

- **Normal**: navigate services (left panel), view logs (right panel)
- **Interactive** (`i`): forward keystrokes to tmux pane via `send_keys`
- **Copy** (`y`): fullscreen log view, mouse disabled for text selection
- **Search** (`/`): case-insensitive search across log buffer

### Log System

Adaptive capture from tmux: small buffer (viewport+50 lines) when following (scroll=0), full buffer (3600 lines) when scrolled. Parsed ANSI lines are cached and only re-rendered when scroll position, search query, or content changes.

### Key Patterns

- `invalidate_log()` marks tmux cache dirty (re-capture next frame)
- `invalidate_parsed()` marks rendered lines dirty (re-parse ANSI next frame)
- `log_scroll=0` means following (live tail), `>0` means scrolled back into history
- Mouse capture auto-toggles: enabled on left panel, disabled on right panel
- Panic hook restores terminal + writes crash log to `~/.tncli/crash.log`
- Event thread is dropped before tmux attach and recreated after detach

### TUI Threading Rule

**The TUI main goroutine must NEVER block on heavy operations.** It only handles rendering + event dispatch.

All heavy work runs in goroutines:
- Docker compose up/down → `go func(){}`
- Git worktree create/remove → background goroutine
- Setup/pre_delete commands → `zsh -c` (non-interactive)
- Update app state + rebuild tree FIRST, then spawn cleanup goroutine

### Sudo Rule

`sudo` is only allowed in `tncli setup` (one-time global setup). Runtime commands (`start`, `workspace create`, `proxy`, etc.) must NEVER require sudo.

### Workspace Branch vs Git Branch

**Always use workspace branch** (from folder name `workspace--{branch}`) for env resolution, hostnames, database names. Git branch may differ (e.g., workspace `crm-524` but git branch `crm-524-send-confirmation-before-showing`). Use `workspace_branch(wt)` helper, fallback to `wt.branch`.

### Config Templates

- `{{host:name}}` — shared service: service name (e.g. `postgres`), resolved via `/etc/hosts` on host, `extra_hosts` in Docker
- `{{port:name}}` — shared service: dynamic port from `SharedPort()`, repo: `proxy_port`
- `{{url:name}}` — `http://{host}:{port}`
- `{{conn:name}}` — `user:pass@host:port` from shared_services
- `{{db:N}}` — Nth database from repo's `databases:` list (session-prefixed + branch-resolved)
- `{{slot:name}}` — allocated slot index for capacity-limited services
- `{{bind_ip}}` — always `127.0.0.1`
- `{{branch_safe}}` — workspace branch with `/` and `-` → `_`

### Network State

`.tncli/network.json` — per-project state:
- `slot`: current runtime slot (0 or 1)
- `blocks`: wsKey → block index (for workspace port blocks)
- `service_map`: svcKey → index within block (stable)
- `shared_map`: shared service name → offset from sharedBase (stable)

`~/.tncli/slots.json` — global session slot leases
`~/.tncli/shared_slots.json` — capacity-based slot allocations (Redis DB indexes)

### tmux Integration

Each service = one tmux window. Services run via `zsh -ic` (interactive, loads .zshrc for nvm/rvm). `pre_start` hook runs after `cd` but before `cmd`. Pane capture uses `-e` flag for ANSI color preservation.

## Go Code Rules

### Structure & SOLID

- **One package per concern** — `config` parses YAML, `tmux` wraps subprocess, `services` handles infra. No circular imports.
- **`cmd/tncli/main.go` = dispatch only** — no business logic, just parse args → call internal packages → print output.
- **Composition over embedding** — Config/Dir/Service are plain structs. Add methods for resolution. No deep type hierarchies.
- **Interfaces defined by consumer, not provider** — if `pipeline` needs to call tmux, it imports `tmux` directly (small project). Only add interfaces when testing requires mocking.
- **Receiver methods for behavior** — `(c *Config).ResolveService()`, `(m *Model).DoStart()`. Not standalone functions taking config as first arg.

### Error Handling

- **Wrap errors with context** — `fmt.Errorf("git worktree add for %s: %w", dirName, err)`. Never lose the original error.
- **Return errors from subprocess calls** — only use `_ =` for cleanup paths (`os.Remove` on teardown). Never ignore `exec.Command().Run()` in happy paths.
- **`fatal()` only in `main.go`** — internal packages return errors. Never `os.Exit()` from library code.
- **No panic** — use `log.Fatalf()` or return error. Panic only for "impossible" programmer errors.

### Security & Subprocess

- **`exec.Command()` only, NEVER shell interpolation** — pass args as separate strings: `exec.Command("git", "-C", dir, "checkout", branch)`. Never `exec.Command("sh", "-c", "git checkout " + branch)`.
- **Exception: multi-command pipelines** — when chaining `cd && source && export && cmd`, use `exec.Command("zsh", "-ic", fullCmd)` with pre-built string. Never interpolate user input directly — sanitize via `BranchSafe()` first.
- **Validate paths** — reject `..` in branch names before `filepath.Join`. Use `filepath.Clean()` on user-provided paths.
- **File permissions** — dirs: `0o755`, config/state: `0o644`, scripts: `0o755`. Never `0o777`.
- **Sudo only in `tncli setup`** — runtime commands must never require sudo.

### DRY & Templates

- **Template resolution centralized** — `ResolveEnvTemplates`, `ResolveConfigTemplates`, `ResolveDBTemplates`, `ResolveSlotTemplates` in `services/services.go`. Never hand-roll `strings.ReplaceAll("{{bind_ip}}", ...)` outside these functions.
- **Extract on 3rd occurrence** — two similar blocks = ok. Third = extract function.
- **`BranchSafe()` for all branch→filename/dbname conversions** — never inline `strings.ReplaceAll(branch, "/", "_")`.

### Concurrency

- **TUI main goroutine never blocks** — all heavy work (docker, git, tmux start/stop) in `go func(){}()`.
- **Pipeline events via channel** — `RunCreatePipeline(ctx, ch)` sends `Event` structs. Consumer (CLI or TUI) reads channel.
- **File locks for shared state** — `WithProjectLock()` for `network.json`, `withSlotLock()` for `shared_slots.json`. Never read-modify-write without lock.
- **`sync.WaitGroup` for parallel stages** — `stageSourceParallel`, `stageConfigureParallel`. Collect errors via mutex.

### TUI (bubbletea)

- **Elm architecture** — `Init() → Update(msg) → View()`. No direct terminal writes.
- **Side effects only via `tea.Cmd`** — or goroutines started in `Update`. Never in `View()`.
- **`SetMessage()` for user feedback** — never `fmt.Print` from TUI code. Messages auto-clear after 5s.
- **`unjoinIfDisplayed()` before stopping** — swap service out of right pane before `GracefulStop`. Otherwise ghost pane appears.
- **Mouse click: first click = select, second click = toggle** — consistent with keyboard Enter behavior.
