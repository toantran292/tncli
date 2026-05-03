package tui

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tmux"
)

const popupResultFile = "/tmp/tncli-popup-result"

// PendingPopup types
type PopupKind int

const (
	PopupNone PopupKind = iota
	PopupBranchPicker
	PopupShortcut
	PopupGitMenu
	PopupGitPullAll
	PopupWsEdit
	PopupWsAdd
	PopupWsRemove
	PopupWsRepoSelect
	PopupWsBranchPick
	PopupNameInput
	PopupConfirm
)

type PendingPopup struct {
	Kind      PopupKind
	Dir       string
	Path      string
	Branch    string
	IsMain    bool
	WsName    string
	WsBranch  string
	ItemsData string
	Idx       int
	Context   string
	Checkout  bool
	Action    string // "delete_ws:branch" or "stop_all"
}

// ── Popup Launchers ──

func (m *Model) requireTool(name, install string) bool {
	if _, err := exec.LookPath(name); err != nil {
		m.SetMessage(fmt.Sprintf("%s not found — install: %s", name, install))
		return false
	}
	return true
}

func (m *Model) popupMenu(title string, options []string, popup PendingPopup) {
	if !m.requireTool("fzf", "brew install fzf") {
		return
	}
	_ = os.Remove(popupResultFile)
	items := strings.Join(options, "\n")
	cmd := fmt.Sprintf(
		"printf '%s' | fzf --prompt='%s > ' --no-info --reverse > %s",
		escSh(items), escSh(title), popupResultFile)
	tmux.DisplayPopup("50%", "40%", cmd)
	m.pendingPopup = &popup
}

func (m *Model) popupInput(title string, popup PendingPopup) {
	_ = os.Remove(popupResultFile)
	exe, _ := os.Executable()
	cmd := fmt.Sprintf("%s popup --type input", exe)
	t := " " + title + " "
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "40", Height: "5", Title: t,
		BorderStyle: "fg=green", BorderLines: "rounded",
	}, cmd)
	m.pendingPopup = &popup
}

func (m *Model) popupConfirm(msg string, action string) {
	_ = os.Remove(popupResultFile)
	exe, _ := os.Executable()
	cmd := fmt.Sprintf("%s popup --type confirm", exe)
	t := " " + msg + " "
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "40%", Height: "6", Title: t,
		BorderStyle: "fg=red", BorderLines: "rounded",
	}, cmd)
	m.pendingPopup = &PendingPopup{Kind: PopupConfirm, Action: action}
}

func (m *Model) popupCheatsheet() {
	content := `
  Left Panel
  j/k          Navigate up/down
  Enter/Space  Toggle start/stop or collapse
  s            Start service/instance
  x            Stop service/instance
  X            Stop all (confirm)
  r            Restart
  c            Shortcuts popup
  e            Open in editor
  g            Git: checkout/pull/diff
  w            Create workspace / worktree menu
  d            Delete workspace (confirm)
  t            Shell in popup
  I            Shared services info
  R            Reload config
  Tab/l        Focus service pane
  n/N          Cycle running services

  Global
  ?            This cheat-sheet
  q            Quit
`
	cmd := fmt.Sprintf("echo '%s' | less -R --prompt='Keybindings (q to close)'", escSh(content))
	tmux.DisplayPopup("50%", "70%", cmd)
}

func (m *Model) popupSharedInfo() {
	if len(m.Config.SharedServices) == 0 {
		m.SetMessage("no shared services configured")
		return
	}
	project := m.Session + "-shared"
	configDir := filepath.Dir(m.ConfigPath)
	composeFile := filepath.Join(configDir, "docker-compose.shared.yml")

	// Try lazydocker first (full Docker TUI)
	if ldPath, err := exec.LookPath("lazydocker"); err == nil {
		cmd := fmt.Sprintf("COMPOSE_PROJECT_NAME=%s COMPOSE_FILE=%s %s", project, composeFile, ldPath)
		tmux.DisplayPopupStyled(tmux.PopupOptions{
			Width: "90%", Height: "90%",
			Title:       " Shared Services ",
			BorderStyle: "fg=cyan",
			BorderLines: "rounded",
		}, cmd)
		return
	}

	m.SetMessage("lazydocker not found — install: brew install lazydocker")
}

func (m *Model) popupShortcuts() {
	item := m.CurrentItem()
	if item == nil || (item.Kind != KindInstanceDir && item.Kind != KindInstanceService) {
		m.SetMessage("no shortcuts for this item")
		return
	}
	dir := m.Config.Repos[item.Dir]
	if dir == nil {
		return
	}

	shortcuts := dir.Shortcuts
	if item.Kind == KindInstanceService {
		if svc, ok := dir.Services[item.Svc]; ok {
			shortcuts = append(shortcuts, svc.Shortcuts...)
		}
	}
	if len(shortcuts) == 0 {
		m.SetMessage("no shortcuts")
		return
	}

	if !m.requireTool("fzf", "brew install fzf") {
		return
	}
	_ = os.Remove(popupResultFile)
	var lines []string
	for i, s := range shortcuts {
		lines = append(lines, fmt.Sprintf("%d\t%s -> %s", i, s.Desc, s.Cmd))
	}
	input := strings.Join(lines, "\n")
	cmd := fmt.Sprintf("echo '%s' | fzf --prompt='Shortcut> ' --with-nth=2.. --delimiter='\t' | cut -f1 > %s",
		escSh(input), popupResultFile)
	tmux.DisplayPopup("70%", "50%", cmd)
	m.pendingPopup = &PendingPopup{Kind: PopupShortcut}
	m.shortcutItems = shortcuts
}

func (m *Model) popupGitMenu() {
	item := m.CurrentItem()
	if item == nil {
		m.SetMessage("select a dir first")
		return
	}

	switch item.Kind {
	case KindInstance:
		label := "main"
		if !item.IsMain {
			label = item.Branch
		}
		m.popupMenu(fmt.Sprintf("Git (%s)", label), []string{"pull all repos"},
			PendingPopup{Kind: PopupGitPullAll, Branch: item.Branch, IsMain: item.IsMain})
	case KindInstanceDir, KindInstanceService:
		path := m.selectedWorkDir()
		if path == "" {
			m.SetMessage("dir not found")
			return
		}
		if item.IsMain {
			m.popupMenu("Git (main)", []string{"pull origin", "diff view"},
				PendingPopup{Kind: PopupGitMenu, Dir: item.Dir, Path: path})
		} else {
			m.popupMenu("Git", []string{"checkout branch", "pull origin", "diff view"},
				PendingPopup{Kind: PopupGitMenu, Dir: item.Dir, Path: path})
		}
	default:
		m.SetMessage("select a dir first")
	}
}

func (m *Model) popupBranchPicker(dirName string, checkoutMode bool) {
	if !m.requireTool("fzf", "brew install fzf") {
		return
	}
	dirPath := m.selectedWorkDir()
	if dirPath == "" {
		dirPath = m.DirPath(dirName)
	}
	_ = os.Remove(popupResultFile)
	cmd := fmt.Sprintf(
		"git -C '%s' branch -a --sort=-committerdate | sed 's/^[* ]*//' | sed 's|remotes/origin/||' | sort -u | fzf --prompt='Branch> ' > %s",
		dirPath, popupResultFile)
	tmux.DisplayPopup("70%", "60%", cmd)
	m.pendingPopup = &PendingPopup{Kind: PopupBranchPicker, Dir: dirName, Checkout: checkoutMode}
}

// ── Helpers ──

func escSh(s string) string {
	return strings.ReplaceAll(s, "'", "'\\''")
}

func containsStr2(ss []string, s string) bool {
	return services.ContainsStr(ss, s)
}

func min(a, b int) int {
	return services.Min(a, b)
}
