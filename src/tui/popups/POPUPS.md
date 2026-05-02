# popups/

Cac popup dialog hien trong TUI, chay qua tmux display-popup overlay.

## Files

| File | Chuc nang |
|---|---|
| `mod.rs` | Re-export va shared utilities cho popup |
| `git.rs` | Git operations popup: `popup_git_menu()` hien menu (pull all, pull origin, diff view, checkout branch). `popup_branch_picker()` chon branch qua fzf (2 mode: checkout va creation). `GitPullAll` handler pull song song tat ca repo |
| `shortcuts.rs` | `popup_shortcuts()` hien danh sach custom commands tu config. Chon command → chay trong tmux pane cua service |
| `workspace.rs` | `popup_shared_info()` hien trang thai shared services (status, host, port). Cac popup quan ly workspace: WsEdit (sua repo), WsAdd (them repo), WsRemove (xoa repo) |

## Co che hoat dong

1. TUI goi `tmux display-popup` voi command `tncli popup --type <Type> --data <json>`
2. Popup chay nhu process doc lap, render bang ratatui trong tmux popup overlay
3. User tuong tac (chon/nhap)
4. Ket qua ghi vao `/tmp/tncli-popup-result`
5. TUI doc file ket qua sau khi popup dong
6. TUI xu ly ket qua (tao worktree, checkout branch, chay command...)
