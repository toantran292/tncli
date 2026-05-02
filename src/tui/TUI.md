# tui/

Giao dien terminal tuong tac (ratatui + crossterm). Quan ly service, xem log, tuong tac voi tmux.

## Files

| File | Chuc nang |
|---|---|
| `mod.rs` | Entry point `run_tui()`: khoi tao terminal, main loop (nhan event → xu ly → render). Panic hook ghi crash log tai `~/.tncli/crash.log` |
| `app.rs` | Struct `App` — toan bo state cua TUI: config, worktrees, tree items, scroll, search, mode. Cac ham rebuild tree, quan ly worktree |
| `app_collapse.rs` | Luu/doc trang thai collapse/expand cua tree tai `~/.tncli/collapse-state.json` |
| `app_editor.rs` | Mo editor (Zed/VS Code) cho repo hoac file config |
| `app_split.rs` | Quan ly tmux split layout — tao/xoa split pane cho TUI |
| `app_status.rs` | Tinh toan status cua service/instance/combo (running/stopped/partial) |
| `event.rs` | Xu ly input: tat ca key bindings (j/k/s/x/r/b/w/d/t/i/y/q...), mouse events, resize |

## Modules

| Module | Chuc nang |
|---|---|
| `screens/` | Logic xu ly cho tung man hinh/hanh dong |
| `popups/` | Cac popup dialog (git, shortcuts, workspace info) |
| `ui/` | Rendering layer (ve giao dien) |

## Architecture

```
Background Thread (event.rs)
  │ poll crossterm events
  │ gui qua mpsc channel
  ▼
Main Loop (mod.rs)
  │ nhan batch events (max 64/frame)
  │ xu ly tat ca events
  │ render 1 lan
  ▼
Ratatui Terminal
```

## TUI Modes

| Mode | Mo ta | Phim |
|---|---|---|
| Normal | Dieu huong tree, quan ly service | Mac dinh |
| Interactive | Go truc tiep vao tmux pane | `i` vao, `Esc` ra |
| Copy | Fullscreen log, tat mouse de select text | `y` vao, `Esc` ra |
| Search | Tim kiem trong log buffer | `/` vao, `Esc` huy |

## Key Bindings (Normal mode)

| Phim | Hanh dong |
|---|---|
| `j/k` | Di chuyen len/xuong |
| `Enter/Space` | Toggle start/stop hoac expand/collapse |
| `s` | Start service/instance |
| `x` | Stop service |
| `X` | Stop tat ca (co confirm) |
| `r` | Restart |
| `b` | Git menu |
| `w` | Workspace menu |
| `d` | Xoa workspace |
| `e` | Mo editor |
| `E` | Mo tncli.yml trong editor |
| `t` | Mo shell tai thu muc |
| `c` | Shortcuts popup |
| `I` | Shared services info |
| `R` | Reload config |
| `Tab/l` | Focus log panel |
| `a` | Attach tmux session |
| `?` | Cheat sheet |
| `q` | Thoat |
