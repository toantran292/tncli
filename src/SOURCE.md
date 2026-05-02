# src/

Entry point va cac module goc cua tncli.

## Files

| File | Chuc nang |
|---|---|
| `main.rs` | CLI entry point, dinh nghia tat ca subcommand bang clap. Default command la `ui` (mo TUI) |
| `commands.rs` | Handler cho tung CLI command: start, stop, restart, status, attach, logs, list, update, setup, workspace, db, proxy |
| `config.rs` | Load va parse `tncli.yml`. Dinh nghia cac struct: Config, Dir, Service, WorktreeConfig, SharedServiceDef, PresetConfig. Resolve template variables |
| `tmux.rs` | Wrapper goi tmux subprocess: tao session/window, kill, attach, capture pane, send keys, display popup |
| `lock.rs` | Quan ly lock file tai `/tmp/tncli/` — danh dau service dang chay (acquire/release) |
| `popup.rs` | Cac popup dialog chay doc lap (Input, WsSelect, Confirm). Ket qua ghi vao `/tmp/tncli-popup-result` |

## Modules

| Module | Chuc nang |
|---|---|
| `pipeline/` | Pipeline tao/xoa workspace theo stage |
| `services/` | Infrastructure layer: docker, git, IP, DNS, compose, proxy, env files |
| `tui/` | Giao dien terminal tuong tac (ratatui) |

## Luong chinh

```
main.rs (clap parse) 
  ├── Command::Ui → tui::run_tui()
  ├── Command::Start/Stop/Restart → commands::cmd_start/stop/restart()
  ├── Command::Workspace → commands::cmd_workspace_create/delete/list()
  ├── Command::Proxy → commands::cmd_proxy_*()
  ├── Command::Db → commands::cmd_db_reset()
  ├── Command::Setup → commands::cmd_setup()
  └── Command::Popup → popup::run_input/ws_select/confirm()
```
