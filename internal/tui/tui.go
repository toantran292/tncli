package tui

import (
	"fmt"
	"os"
	"time"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/tmux"
)

type tickMsg time.Time

func tickCmd() tea.Cmd {
	return tea.Tick(time.Second, func(t time.Time) tea.Msg { return tickMsg(t) })
}

func (m *Model) Init() tea.Cmd {
	m.RefreshStatus()
	return tickCmd()
}

func (m *Model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		return m.handleKey(msg)
	case tea.MouseMsg:
		m.handleMouse(msg)
		return m, nil
	case tea.WindowSizeMsg:
		m.Width = msg.Width
		m.Height = msg.Height
		return m, nil
	case tickMsg:
		m.RefreshStatus()
		m.pollPopupResult()
		if m.TuiWindowID != "" {
			m.ensureSplit()
		}
		if m.SwapPending {
			m.swapDisplayService()
			if m.JoinedSvc != "" {
				m.SwapPending = false
			}
		}
		if m.Message != "" && time.Since(m.MessageTime) > 5*time.Second {
			m.Message = ""
		}
		return m, tickCmd()
	}
	return m, nil
}

func (m *Model) handleKey(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.String() {
	case "q", "ctrl+c":
		return m, tea.Quit
	case "?":
		m.popupCheatsheet()
	case "j", "down":
		if m.Cursor+1 < len(m.ComboItems) {
			m.Cursor++
		}
		m.ComboLogIdx = 0
		m.SwapPending = true
	case "k", "up":
		if m.Cursor > 0 {
			m.Cursor--
		}
		m.ComboLogIdx = 0
		m.SwapPending = true
	case "enter", " ":
		m.doToggle()
	case "s":
		m.doStart()
	case "x":
		m.doStop()
	case "r":
		m.doRestart()
	case "e":
		m.openEditor()
	case "t":
		if dir := m.selectedWorkDir(); dir != "" {
			tmux.DisplayPopup("90%", "85%", fmt.Sprintf("cd '%s' && exec zsh", dir))
		}
	case "c":
		m.popupShortcuts()
	case "g":
		m.popupGitMenu()
	case "I":
		m.popupSharedInfo()
	case "w", "W":
		m.handleWorkspaceKey()
	case "d", "D":
		item := m.CurrentItem()
		if item != nil && item.Kind == KindInstance && !item.IsMain {
			m.popupConfirm(fmt.Sprintf("Delete workspace '%s'?", item.Branch), "delete_ws:"+item.Branch)
		}
	case "X":
		m.popupConfirm("Stop ALL services?", "stop_all")
	case "B":
		m.popupDBMenu()
	case "o":
		m.doOpenURL()
	case "R":
		m.reloadConfig()
	case "tab", "l":
		if m.RightPaneID != "" {
			tmux.SelectPane(m.RightPaneID)
		}
	case "n":
		m.cycleComboLog(1)
	case "N":
		m.cycleComboLog(-1)
	}

	m.ClampCursor()
	// Immediate swap for navigation keys (window already exists)
	if m.SwapPending && m.JoinedSvc != "" {
		m.swapDisplayService()
		if m.JoinedSvc != "" || m.logServiceName() == "" {
			m.SwapPending = false
		}
	}
	return m, nil
}

func (m *Model) handleWorkspaceKey() {
	item := m.CurrentItem()
	if item != nil && !item.IsMain && (item.Kind == KindInstance || item.Kind == KindInstanceDir || item.Kind == KindInstanceService) {
		m.wsName = m.FindParentCombo(m.Cursor)
		if m.wsName == "" && len(m.Combos) > 0 {
			m.wsName = m.Combos[0]
		}
		m.wsSourceBranch = item.Branch
		m.popupMenu("Workspace", []string{"Create new workspace", "Add repo", "Remove repo"},
			PendingPopup{Kind: PopupWsEdit, Branch: item.Branch})
	} else {
		m.wsName = m.FindParentCombo(m.Cursor)
		if m.wsName == "" && len(m.Combos) > 0 {
			m.wsName = m.Combos[0]
		}
		m.wsCreating = true
		m.popupInput("Workspace branch name:", PendingPopup{Kind: PopupNameInput, Context: "workspace"})
	}
}

func (m *Model) handleMouse(msg tea.MouseMsg) {
	switch msg.Button {
	case tea.MouseButtonLeft:
		if msg.Action == tea.MouseActionPress {
			realIdx := m.visualToRealIdx(int(msg.Y))
			if realIdx >= 0 && realIdx < len(m.ComboItems) {
				if m.Cursor == realIdx {
					m.doToggle()
				} else {
					m.Cursor = realIdx
				}
				m.SwapPending = true
			}
		}
	case tea.MouseButtonWheelUp:
		if m.Cursor > 0 {
			m.Cursor--
			m.SwapPending = true
		}
	case tea.MouseButtonWheelDown:
		if m.Cursor+1 < len(m.ComboItems) {
			m.Cursor++
			m.SwapPending = true
		}
	}
}

// Run starts the TUI.
func Run() error {
	if !tmux.InTmux() {
		cfgPath, err := config.FindConfig()
		if err != nil {
			return err
		}
		cfg, err := config.Load(cfgPath)
		if err != nil {
			return err
		}
		return autoEnterTmux(cfg.Session)
	}

	cfgPath, err := config.FindConfig()
	if err != nil {
		return err
	}
	m, err := NewModel(cfgPath)
	if err != nil {
		return err
	}

	m.TuiWindowID = tmux.CurrentWindowID()
	m.TuiSession = tmux.CurrentSessionName()
	if m.TuiWindowID != "" {
		m.SetupSplit()
	}

	p := tea.NewProgram(m, tea.WithAltScreen(), tea.WithMouseAllMotion())
	_, err = p.Run()

	m.TeardownSplit()
	return err
}

func autoEnterTmux(session string) error {
	exe, _ := os.Executable()
	cwd, _ := os.Getwd()
	tuiSession := "tncli"

	if tmux.WindowExists(tuiSession, session) {
		panes := tmux.ListPaneIDs(fmt.Sprintf("=%s:%s", tuiSession, session))
		if len(panes) > 0 {
			tmux.SendKeys(panes[0], "q")
		}
		for i := 0; i < 20; i++ {
			time.Sleep(100 * time.Millisecond)
			if !tmux.WindowExists(tuiSession, session) {
				break
			}
		}
		if tmux.WindowExists(tuiSession, session) {
			tmux.KillWindow(tuiSession, session)
		}
	}

	cmd := fmt.Sprintf("%s ui; tmux detach-client 2>/dev/null", exe)
	if tmux.SessionExists(tuiSession) {
		tmux.NewWindowInDir(tuiSession, session, cwd, cmd)
	} else {
		tmux.NewSessionInDir(tuiSession, session, cwd, cmd)
	}

	tmux.AttachSession(fmt.Sprintf("=%s:%s", tuiSession, session))
	return nil
}
