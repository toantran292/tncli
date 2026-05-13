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
	PopupNameInput
	PopupConfirm
	PopupDBMenu
	PopupEnvSelect
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
		tmux.DisplayMessage(fmt.Sprintf(" %s not found — install: %s ", name, install))
		return false
	}
	return true
}

func (m *Model) popupMenu(title string, options []string, popup PendingPopup) {
	_ = os.Remove(popupResultFile)
	dataFile := "/tmp/tncli-popup-data"
	_ = os.WriteFile(dataFile, []byte(strings.Join(options, "\n")), 0o644)
	exe, _ := os.Executable()
	cmd := fmt.Sprintf("%s popup --type list --data-file '%s'", exe, dataFile)
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "50%", Height: "40%", Title: " " + title + " ",
		BorderStyle: "fg=cyan", BorderLines: "rounded",
	}, cmd)
	m.pendingPopup = &popup
}

func (m *Model) popupInput(title string, popup PendingPopup) {
	_ = os.Remove(popupResultFile)
	exe, _ := os.Executable()
	cmd := fmt.Sprintf("%s popup --type input", exe)
	t := " " + title + " "
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "70%", Height: "5", Title: t,
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
	exe, _ := os.Executable()
	cmd := fmt.Sprintf("%s popup --type cheatsheet", exe)
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "50%", Height: "80%",
		Title:       " Keybindings ",
		BorderStyle: "fg=cyan",
		BorderLines: "rounded",
		Style:       "bg=colour235",
	}, cmd)
}

func (m *Model) popupSharedInfo() {
	if len(m.Config.SharedServices) == 0 {
		tmux.DisplayMessage(" no shared services configured")
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

	tmux.DisplayMessage(" lazydocker not found — install: brew install lazydocker ")
}

func (m *Model) popupShortcuts() {
	item := m.CurrentItem()
	if item == nil || (item.Kind != KindInstanceDir && item.Kind != KindInstanceService) {
		tmux.DisplayMessage(" no shortcuts for this item")
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
		tmux.DisplayMessage(" no shortcuts")
		return
	}

	var options []string
	for i, s := range shortcuts {
		options = append(options, fmt.Sprintf("%d\t%s", i, s.Desc))
	}
	m.popupMenu("Shortcuts", options, PendingPopup{Kind: PopupShortcut})
	m.shortcutItems = shortcuts
}

func (m *Model) popupGitMenu() {
	item := m.CurrentItem()
	if item == nil {
		tmux.DisplayMessage(" select a dir first")
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
			tmux.DisplayMessage(" dir not found")
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
		tmux.DisplayMessage(" select a dir first")
	}
}

func (m *Model) popupEnvSelect() {
	item := m.CurrentItem()
	if item == nil || item.Branch == "" {
		return
	}
	envNames := m.Config.EnvironmentNames()
	if len(envNames) == 0 {
		tmux.DisplayMessage(" no environments defined in tncli.yml")
		return
	}
	options := append([]string{"local"}, envNames...)

	switch item.Kind {
	case KindInstanceService:
		// Per-service: key = "alias/svc"
		alias := item.Dir
		if dir, ok := m.Config.Repos[item.Dir]; ok && dir.Alias != "" {
			alias = dir.Alias
		}
		key := alias + "/" + item.Svc
		m.popupMenu("Environment ("+key+")", options,
			PendingPopup{Kind: PopupEnvSelect, Branch: item.Branch, Dir: key})
	case KindInstanceDir:
		// All services in repo
		alias := item.Dir
		if dir, ok := m.Config.Repos[item.Dir]; ok && dir.Alias != "" {
			alias = dir.Alias
		}
		m.popupMenu("Environment ("+alias+"/*)", options,
			PendingPopup{Kind: PopupEnvSelect, Branch: item.Branch, Dir: alias})
	default:
		tmux.DisplayMessage(" select a service or repo first")
	}
}

func (m *Model) popupBranchPicker(dirName string, checkoutMode bool) {
	dirPath := m.selectedWorkDir()
	if dirPath == "" {
		dirPath = m.DirPath(dirName)
	}
	_ = os.Remove(popupResultFile)
	dataFile := "/tmp/tncli-popup-data"
	exe, _ := os.Executable()
	cmd := fmt.Sprintf(
		"git -C '%s' branch -a --sort=-committerdate | sed 's/^[* ]*//' | sed 's|remotes/origin/||' | sort -u > '%s' && %s popup --type list --data-file '%s'",
		dirPath, dataFile, exe, dataFile)
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "70%", Height: "60%", Title: " Branch ",
		BorderStyle: "fg=cyan", BorderLines: "rounded",
	}, cmd)
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
