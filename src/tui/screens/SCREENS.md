# screens/

Logic xu ly cho tung man hinh va hanh dong trong TUI.

## Files

| File | Chuc nang |
|---|---|
| `mod.rs` | Re-export cac screen module |
| `tree.rs` | Xay dung cay workspace: `rebuild_combo_tree()` tao danh sach phang tu combo → instance → dir → service. `build_instance_dirs()` xay cau truc cho tung instance |
| `logs.rs` | Panel log ben phai: capture output tu tmux pane, scroll (j/k/mouse), search (`/`), ANSI parsing voi cache. Adaptive: scroll=0 capture nho (viewport+50), scrolled capture full (3600 dong) |
| `svc_start.rs` | Logic start service: tao tmux window, cd vao repo, set env (BIND_IP, NODE_OPTIONS), chay pre_start hook, chay cmd. Ham `start_service_with_info()` |
| `svc_stop.rs` | Logic stop service: gui Ctrl+C (graceful) → doi 500ms → kill window → xoa lock |
| `svc_actions.rs` | Menu hanh dong cho service: start, stop, restart, attach, logs |
| `workspace.rs` | Tao worktree cho 1 repo don le: `create_worktree()` — cap IP, generate compose override, env file |
| `ws_builders.rs` | Tao workspace day du: chon repo, chon branch, goi pipeline create. Xu ly popup WsRepoSelect va WsBranchPick |
