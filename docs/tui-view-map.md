# TUI View Navigation Map

Keep this file up-to-date when adding new views, popups, or key bindings.

## Main View

Tree sidebar (left) + service log pane (right). Tick every 1s refreshes state.

## Key Bindings → View Transitions

```
[MAIN TUI]
│
├── Navigation
│   ├── j/k/up/down     → move cursor
│   ├── enter/space      → toggle collapse (combo/instance/dir) or start/stop service
│   ├── n/N              → cycle service log in right pane
│   └── tab/l            → focus right pane
│
├── Service Control
│   ├── s                → doStart() (async, no popup)
│   ├── x                → doStop() (async, no popup)
│   ├── r                → doRestart() (async, no popup)
│   └── X                → [CONFIRM: Stop All] → doStopAll()
│
├── Shortcuts
│   └── c                → [LIST: Shortcuts]
│                           └── result idx → [SHELL: Shortcut Execution]
│
├── Git
│   └── g                → [LIST: Git Menu]
│                           ├── "checkout branch" → [LIST: Branch Picker] → checkout
│                           ├── "pull origin"     → [SHELL: git pull output]
│                           ├── "diff view"       → [SHELL: git diff output]
│                           └── "pull all repos"  → [SHELL: parallel git pull]
│
├── Environment
│   └── E                → [LIST: Env Select] → update .tncli-workspace.json + regen env
│
├── Workspace
│   ├── w/W              → [LIST: WS Edit Menu]
│   │                       ├── "Create new" → [INPUT: Branch Name]
│   │                       │                   └── [MULTI-SELECT: Repos] → async create pipeline
│   │                       ├── "Add repo"   → [LIST: Available Repos] → add worktree
│   │                       └── "Remove repo"→ [LIST: Existing Repos] → remove worktree
│   └── d/D              → [CONFIRM: Delete WS] → async delete pipeline
│
├── Database
│   └── B                → [LIST: DB Menu] → create/recreate/drop (async)
│
├── Utilities
│   ├── e                → open editor (external process)
│   ├── t                → [TMUX SHELL: nested tmux session with mouse+scroll]
│   ├── I                → [LAZYDOCKER: shared services] (interactive)
│   ├── o                → open browser (external)
│   ├── R                → reload config
│   └── ?                → [CHEATSHEET] (display only, scrollable)
│
└── q/ctrl+c             → quit + cleanup
```

## Popup Types

| Type | Engine | Result | Examples |
|------|--------|--------|----------|
| LIST | bubbletea (popup/list.go) | selected item → ResultFile | shortcuts, git menu, env, branch, db, ws edit/add/remove |
| INPUT | bubbletea (popup/popup.go) | text → ResultFile | workspace branch name |
| CONFIRM | bubbletea (popup/popup.go) | "y" → ResultFile | delete ws, stop all |
| MULTI-SELECT | bubbletea (popup/popup.go) | lines → ResultFile | workspace repo select |
| CHEATSHEET | bubbletea (popup/cheatsheet.go) | none | keybindings help |
| SHELL | tmux popup | none | shortcut exec, git pull/diff |
| TMUX SHELL | tmux popup + nested session | none | terminal ('t') |
| LAZYDOCKER | tmux popup | none | shared services ('I') |

## Popup Communication

All bubbletea popups write results to `/tmp/tncli-popup-result`.
Main TUI reads this file every tick (1s) in `pollPopupResult()`.
`m.pendingPopup` tracks which popup is active and its context.

## Popup Chains (multi-step flows)

- `g` → Git Menu → Branch Picker (2 steps)
- `w` → WS Edit → Name Input → Repo Select (3 steps)
- `c` → Shortcut Picker → Shortcut Execution (2 steps)

## Adding a New View

1. Add PopupKind constant in `popups.go`
2. Add launcher method in `popups.go` or `popups_ws.go`
3. Add result handler in `popups_poll.go` (switch case)
4. Add key binding in `tui.go` handleKey
5. Update cheatsheet in `popup/cheatsheet.go`
6. **Update this file**
