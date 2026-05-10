package tui

import (
	"fmt"
	"os/exec"
	"strconv"
	"strings"

	"github.com/toantran292/tncli/internal/tmux"
)

func (m *Model) SetupSplit() {
	if m.TuiWindowID == "" {
		return
	}
	m.TuiPaneID = tmux.CurrentPaneID()
	placeholder := "echo; echo '  Select a running service'; echo; echo '  j/k navigate · Tab focus · n/N cycle'; stty -echo; tail -f /dev/null"
	tmux.SplitWindowRight(m.SplitPct, placeholder)
	for _, p := range tmux.ListPaneIDs(m.TuiWindowID) {
		if p != m.TuiPaneID {
			m.RightPaneID = p
			break
		}
	}
	tmux.SetWindowOption(m.TuiWindowID, "pane-border-style", "fg=colour238")
	tmux.SetWindowOption(m.TuiWindowID, "pane-active-border-style", "fg=colour238")
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
	// Save current split ratio before teardown
	m.saveSplitRatio()
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
	tmux.UnsetWindowOption(m.TuiWindowID, "pane-border-style")
	tmux.UnsetWindowOption(m.TuiWindowID, "pane-active-border-style")
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
		tmux.SplitWindowRight(m.SplitPct, placeholder)
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

func (m *Model) saveSplitRatio() {
	if m.RightPaneID == "" || m.TuiWindowID == "" {
		return
	}
	// Get total window width and right pane width
	out, err := exec.Command("tmux", "display-message", "-t", m.TuiWindowID, "-p", "#{window_width}").Output()
	if err != nil {
		return
	}
	winWidth, _ := strconv.Atoi(strings.TrimSpace(string(out)))
	out, err = exec.Command("tmux", "display-message", "-t", fmt.Sprintf("%%%s", m.RightPaneID), "-p", "#{pane_width}").Output()
	if err != nil {
		return
	}
	paneWidth, _ := strconv.Atoi(strings.TrimSpace(string(out)))
	if winWidth > 0 && paneWidth > 0 {
		pct := paneWidth * 100 / winWidth
		if pct >= 20 && pct <= 90 {
			m.SplitPct = pct
			m.saveCollapseState()
		}
	}
}
