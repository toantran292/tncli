# ui/

Rendering layer — ve giao dien TUI bang ratatui.

## Files

| File | Chuc nang |
|---|---|
| `mod.rs` | Ham `draw()` chinh: chia layout thanh 2 panel (trai: tree, phai: log). Dieu phoi render tung phan: workspace tree, log panel, status bar, overlays |
| `workspace_ui.rs` | Render cay workspace: ve tung ComboItem (combo/instance/dir/service) voi icon trang thai (running/stopped/partial), mau sac, indent theo cap |
| `panel.rs` | Layout va styling cho panel: border, title, focus highlight. Tinh toan kich thuoc panel trai/phai |
| `overlays.rs` | Render popup overlays: cheat sheet (`?`), search bar (`/`), message bar, pipeline progress bar |

## Render Flow

```
draw() (mod.rs)
  ├── Chia layout: [left_panel | right_panel]
  ├── workspace_ui: ve tree items vao left_panel
  ├── logs (screens/logs.rs): ve log content vao right_panel
  ├── panel: ve border + title cho ca 2 panel
  └── overlays: ve popup/search/message len tren cung
```

## Visual Elements

- **Tree items**: icon trang thai + ten, indent theo cap (combo > instance > dir > service)
- **Log panel**: ANSI color preserved, line numbers khi search, highlight search matches
- **Status bar**: ten service dang chon, mode hien tai
- **Overlays**: search input, cheat sheet, pipeline progress, message notifications
