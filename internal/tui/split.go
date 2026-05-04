package tui

import (
	"github.com/toantran292/tncli/internal/tmux"
)

func (m *Model) SetupSplit() {
	if m.TuiWindowID == "" {
		return
	}
	m.TuiPaneID = tmux.CurrentPaneID()
	placeholder := "echo; echo '  Select a running service'; echo; echo '  j/k navigate · Tab focus · n/N cycle'; stty -echo; tail -f /dev/null"
	tmux.SplitWindowRight(75, placeholder)
	for _, p := range tmux.ListPaneIDs(m.TuiWindowID) {
		if p != m.TuiPaneID {
			m.RightPaneID = p
			break
		}
	}
	tmux.SetWindowOption(m.TuiWindowID, "pane-border-status", "top")
	tmux.SetWindowOption(m.TuiWindowID, "pane-border-format",
		" #{?pane_active,#[fg=colour39#,bold],#[fg=colour252]}#{pane_title}#[default] ")
	if m.TuiPaneID != "" {
		tmux.SetPaneTitle(m.TuiPaneID, m.Session)
	}
	if m.RightPaneID != "" {
		tmux.SetPaneTitle(m.RightPaneID, "service")
	}
}

func (m *Model) TeardownSplit() {
	if m.TuiWindowID == "" {
		return
	}
	svcSess := m.SvcSession()
	if m.JoinedSvc != "" {
		if m.RightPaneID != "" {
			if tmux.WindowExists(svcSess, m.JoinedSvc) {
				_ = tmux.SwapPane(svcSess, m.JoinedSvc, m.RightPaneID)
			} else {
				tmux.EnsureSession(svcSess)
				tmux.BreakPaneTo(m.RightPaneID, svcSess, m.JoinedSvc)
			}
		}
		m.JoinedSvc = ""
	}
	if m.TuiPaneID != "" {
		for _, p := range tmux.ListPaneIDs(m.TuiWindowID) {
			if p != m.TuiPaneID {
				tmux.KillPane(p)
			}
		}
	}
	tmux.UnsetWindowOption(m.TuiWindowID, "pane-border-status")
	tmux.UnsetWindowOption(m.TuiWindowID, "pane-border-format")
	m.RightPaneID = ""
}

func (m *Model) swapDisplayService() {
	svcSess := m.SvcSession()
	newSvc := m.logServiceName()
	if newSvc == m.JoinedSvc {
		return
	}
	if m.JoinedSvc != "" && m.RightPaneID != "" {
		if tmux.WindowExists(svcSess, m.JoinedSvc) {
			_ = tmux.SwapPane(svcSess, m.JoinedSvc, m.RightPaneID)
			m.redetectRightPane()
		}
		m.JoinedSvc = ""
	}
	if newSvc != "" && m.RightPaneID != "" {
		if tmux.WindowExists(svcSess, newSvc) {
			if err := tmux.SwapPane(svcSess, newSvc, m.RightPaneID); err == nil {
				m.JoinedSvc = newSvc
				m.redetectRightPane()
				if m.RightPaneID != "" {
					tmux.SetPaneTitle(m.RightPaneID, newSvc)
				}
			}
		}
	} else if m.RightPaneID != "" {
		tmux.SetPaneTitle(m.RightPaneID, "service")
	}
}

func (m *Model) redetectRightPane() {
	if m.TuiWindowID == "" {
		return
	}
	for _, p := range tmux.ListPaneIDs(m.TuiWindowID) {
		if p != m.TuiPaneID {
			m.RightPaneID = p
			return
		}
	}
}

func (m *Model) ensureSplit() {
	if m.TuiWindowID == "" {
		return
	}
	panes := tmux.ListPaneIDs(m.TuiWindowID)
	if len(panes) < 2 {
		placeholder := "echo; echo '  Select a running service'; echo; echo '  j/k navigate · Tab focus · n/N cycle'; stty -echo; tail -f /dev/null"
		tmux.SplitWindowRight(75, placeholder)
		for _, p := range tmux.ListPaneIDs(m.TuiWindowID) {
			if p != m.TuiPaneID {
				m.RightPaneID = p
				break
			}
		}
		if m.RightPaneID != "" {
			tmux.SetPaneTitle(m.RightPaneID, "service")
		}
		m.JoinedSvc = ""
	}
}
