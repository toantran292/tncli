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

### tmux Integration

Each service = one tmux window. Services run via `zsh -ic` (interactive, loads .zshrc for nvm/rvm). `pre_start` hook runs after `cd` but before `cmd`. Pane capture uses `-e` flag for ANSI color preservation.
