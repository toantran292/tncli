# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
make build         # Debug build + codesign
make release       # Release build (strip+LTO) + codesign
make notarize      # Release + Apple notarization (requires keychain profile "tncli-notarize")
make clean         # Remove artifacts
```

Requires: `rustc`, `cargo`, `tmux`, `codesign` (macOS)

## Architecture

Rust single-binary CLI+TUI for managing tmux services. Config via `tncli.yml` found by walking up from CWD.

**CLI path**: `main.rs` (clap dispatch) -> `commands.rs` -> `tmux.rs` (subprocess calls)

**TUI path**: `main.rs` -> `tui/mod.rs` (terminal setup, main loop) -> `tui/event.rs` (background event thread) -> `tui/ui.rs` (ratatui rendering)

### Event-Driven Architecture

Background thread polls crossterm events and sends them via `mpsc` channel. Main loop receives batched events (up to 64/frame), processes all, then draws once. This prevents touchpad scroll flooding from freezing the UI.

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

**The TUI main thread must NEVER block on heavy operations.** It only handles rendering + event dispatch.

All heavy work runs in background threads:
- Docker compose up/down → `std::thread::spawn`
- Git worktree create/remove → background thread
- Setup/pre_delete commands → `run_setup()` with `zsh -c` (non-interactive), stdin/stdout/stderr null
- Update app state + rebuild tree FIRST, then spawn cleanup thread
- Never use `zsh -ic` (interactive) in background — causes `suspended (tty input)` crash
- Never use `eprintln!` from app logic — corrupts ratatui rendering. Return messages via `set_message()`

### Sudo Rule

`sudo` is only allowed in `tncli setup` (one-time global setup). Runtime commands (`start`, `workspace create`, `proxy`, etc.) must NEVER require sudo.

### Reverse Proxy (Caddy)

Caddy reverse proxy routes by Host header. Generated `~/.tncli/Caddyfile` from `proxy-routes.json`.

- Caddy binds `127.0.0.1` only (prevents proxy loop — services bind to `127.0.1.x`)
- Hostname convention: `{session}.{alias}.ws-{branch_safe}.tncli.test` (workspace), `{session}.{service}.tncli.test` (shared)
- DNS: dnsmasq wildcard `*.tncli.test` → `127.0.0.1`, shared services also in `/etc/hosts`
- `tncli proxy start/restart/stop/status` manages Caddy daemon

### Workspace Branch vs Git Branch

**Always use workspace branch** (from folder name `workspace--{branch}`) for env resolution, hostnames, database names. Git branch may differ (e.g., workspace `crm-524` but git branch `crm-524-send-confirmation-before-showing`). Use `workspace_branch(wt)` helper, fallback to `wt.branch`.

### Config Templates

- `{{host:name}}` — shared: `{session}.{name}.tncli.test`, repo: by alias
- `{{port:name}}` — shared: mapped host port, repo: proxy_port
- `{{url:name}}` — `http://{host}:{port}` (lookup by repo name first, alias fallback)
- `{{conn:name}}` — `user:pass@host:port` from shared_services
- `{{db:N}}` — Nth database from repo's `databases:` list (session-prefixed + branch-resolved)
- `{{slot:name}}` — allocated slot index for capacity-limited services
- `{{bind_ip}}` — workspace loopback IP
- `{{branch_safe}}` — workspace branch with `/` and `-` → `_`

### Network State

`~/.tncli/network.json` — unified state (version 2):
- `subnets`: session → subnet slot (1-based)
- `allocations`: worktree key → IP (`127.0.{subnet}.{host}`)

### tmux Integration

Each service = one tmux window. Services run via `zsh -ic` (interactive, loads .zshrc for nvm/rvm). `pre_start` hook runs after `cd` but before `cmd`. Pane capture uses `-e` flag for ANSI color preservation.
