# tncli

tmux-based service launcher. Define services and combinations in YAML, manage them with CLI commands or an interactive TUI.

## Install

```bash
./install.sh
```

Or manually:

```bash
make release
cp ./tncli /usr/local/bin/
```

### Dependencies

- `tmux` (3.x+)
- `zsh`

## Config

Place `tncli.yml` at your project root. `tncli` searches upward from the current directory.

```yaml
session: myproject

services:
  api:
    cmd: bundle exec rails server
    dir: api
  worker:
    cmd: bundle exec sidekiq
    dir: api
  client:
    cmd: npm run dev
    dir: client
    pre_start: nvm use

combinations:
  backend:
    - api
    - worker
  full:
    - api
    - worker
    - client
```

### Service fields

| Field | Required | Description |
|-------|----------|-------------|
| `cmd` | yes | Shell command to run |
| `dir` | no | Working directory, relative to config file or absolute |
| `env` | no | Environment variables prefix (e.g. `PORT=3000 DEBUG=1`) |
| `pre_start` | no | Command to run before `cmd` (e.g. `nvm use`) |

## CLI Usage

```bash
tncli start <service|combo>    # start a service or combination
tncli stop [service|combo]     # stop specific (no arg = stop all)
tncli restart <service|combo>  # restart
tncli status                   # show running/stopped services
tncli list                     # list all services and combinations
tncli attach [service]         # attach to tmux session
tncli logs <service>           # show recent output
tncli ui                       # open interactive TUI (default)
tncli help                     # show help
```

## TUI (`tncli ui`)

Interactive terminal interface with two panels:

```
┌─ Services ────────┬─ logs: api ──────────────────────────┐
│ ● api             │ => Booting Puma                       │
│ ● worker          │ * Listening on tcp://0.0.0.0:3000     │
│ ○ client          │ Started GET "/api/v1/..."             │
├─ Combinations ────│ Completed 200 OK in 12ms              │
│ ● backend    2/2  │                                       │
│ ○ full       2/3  │                                       │
└───────────────────┴──────────────────────────────────────┘
 enter toggle  s start  x stop  r restart  q quit
```

### Keyboard

**Left panel (services/combos):**

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `Enter` / `Space` | Toggle start/stop |
| `s` | Start |
| `x` | Stop |
| `X` | Stop all |
| `r` | Restart |
| `Tab` / `l` | Focus log panel |

**Right panel (logs):**

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll down/up |
| `G` / `g` | Jump to bottom / top |
| `/` | Search in logs |
| `n` / `N` | Next / previous match (or cycle combo logs) |
| `i` | Interactive mode (send keys to pane) |
| `y` | Copy mode (fullscreen logs) |
| `Tab` / `h` | Focus back to left panel |

**Global:**

| Key | Action |
|-----|--------|
| `a` | Attach to tmux session |
| `R` | Reload config |
| `q` | Quit |

### Mouse

- Click to select services/combos or focus log panel
- Scroll wheel navigates list (left) or scrolls logs (right)
- Mouse auto-disables on log panel for text selection

### Status icons

| Icon | Meaning |
|------|---------|
| `●` | Running |
| `◐` | Partially running (combo) |
| `○` | Stopped |

## Build

```bash
make build       # debug
make release     # optimized + signed
make notarize    # release + Apple notarization
```

## Architecture

Single Rust binary. Each service runs in a tmux window within a shared session. The TUI uses ratatui with an event-driven architecture (background event thread + channel).

```
src/
├── main.rs          # CLI entry point (clap)
├── config.rs        # YAML config loading
├── commands.rs      # CLI command implementations
├── tmux.rs          # tmux subprocess wrappers
├── lock.rs          # file-based lock management
└── tui/
    ├── mod.rs       # TUI main loop + panic handler
    ├── app.rs       # application state
    ├── event.rs     # event thread + key/mouse handlers
    ├── ui.rs        # ratatui rendering
    └── ansi.rs      # ANSI escape code parser
```
