package tui

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tmux"
)

// ── Messages ──

type tickMsg time.Time

func tickCmd() tea.Cmd {
	return tea.Tick(time.Second, func(t time.Time) tea.Msg {
		return tickMsg(t)
	})
}

// ── Init / Update / View ──

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
		// Ensure split pane
		if m.TuiWindowID != "" {
			m.ensureSplit()
		}
		// Clear old messages
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
		item := m.CurrentItem()
		if item != nil && (item.Kind == KindInstanceDir || item.Kind == KindInstanceService) {
			dir := m.selectedWorkDir()
			if dir != "" {
				tmux.DisplayPopup("90%", "85%", fmt.Sprintf("cd '%s' && exec zsh", dir))
			}
		}
	case "c":
		m.popupShortcuts()
	case "g":
		m.popupGitMenu()
	case "I":
		m.popupSharedInfo()
	case "w", "W":
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
			// Main: create new
			m.wsName = m.FindParentCombo(m.Cursor)
			if m.wsName == "" && len(m.Combos) > 0 {
				m.wsName = m.Combos[0]
			}
			m.wsCreating = true
			m.popupInput("Workspace branch name:", PendingPopup{Kind: PopupNameInput, Context: "workspace"})
		}
	case "d", "D":
		item := m.CurrentItem()
		if item != nil && item.Kind == KindInstance && !item.IsMain {
			m.popupConfirm(fmt.Sprintf("Delete workspace '%s'?", item.Branch), "delete_ws:"+item.Branch)
		}
	case "X":
		m.popupConfirm("Stop ALL services?", "stop_all")
	case "B":
		m.doRecreateDB()
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
	if m.SwapPending {
		m.SwapPending = false
		m.swapDisplayService()
	}
	return m, nil
}

func (m *Model) handleMouse(msg tea.MouseMsg) {
	switch msg.Button {
	case tea.MouseButtonLeft:
		if msg.Action == tea.MouseActionPress {
			idx := int(msg.Y)
			if idx >= 0 && idx < len(m.ComboItems) {
				m.Cursor = idx
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

func (m *Model) View() string {
	if m.Width == 0 {
		return "Loading..."
	}

	// Build left panel (workspace tree)
	var lines []string
	for i, item := range m.ComboItems {
		isCur := i == m.Cursor
		line := m.renderItem(item, isCur)
		lines = append(lines, line)
	}

	// Fill remaining height
	for len(lines) < m.Height-2 {
		lines = append(lines, "")
	}

	// Message bar at bottom
	msgLine := ""
	if m.Message != "" {
		msgStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("11")).Bold(true)
		msgLine = msgStyle.Render(" " + m.Message)
	}

	// Pipeline progress
	for _, p := range m.ActivePipelines {
		status := fmt.Sprintf(" [%d/%d] %s: %s", p.CurrentStage+1, p.TotalStages, p.Operation, p.StageName)
		if p.Failed != nil {
			status += fmt.Sprintf(" FAILED: %s", p.Failed.Error)
		}
		pStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("33"))
		msgLine = pStyle.Render(status)
	}

	content := strings.Join(lines[:m.Height-2], "\n")
	if msgLine != "" {
		content += "\n" + msgLine
	}

	return content
}

func (m *Model) renderItem(item ComboItem, isCur bool) string {
	curMark := "  "
	if isCur {
		curMark = "> "
	}

	switch item.Kind {
	case KindCombo:
		style := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("15"))
		if isCur {
			style = style.Background(lipgloss.Color("236"))
		}
		return style.Render(curMark + "▾ " + item.Name)

	case KindInstance:
		icon := "◆"
		color := "39" // blue
		label := item.Branch
		if item.IsMain {
			label = item.Branch + " (main)"
			color = "35" // magenta
		}
		if m.CreatingWs[item.Branch] {
			icon = "⏳"
			color = "11"
		}
		if m.DeletingWs[item.Branch] {
			icon = "🗑"
			color = "9"
		}
		style := lipgloss.NewStyle().Foreground(lipgloss.Color(color))
		if isCur {
			style = style.Background(lipgloss.Color("236"))
		}
		return style.Render(fmt.Sprintf("%s  %s %s", curMark, icon, label))

	case KindInstanceDir:
		if strings.HasPrefix(item.Dir, "_global:") {
			svcName := strings.TrimPrefix(item.Dir, "_global:")
			style := lipgloss.NewStyle().Foreground(lipgloss.Color("244"))
			if isCur {
				style = style.Background(lipgloss.Color("236"))
			}
			return style.Render(fmt.Sprintf("%s    ◇ %s", curMark, svcName))
		}
		alias := item.Dir
		if dir, ok := m.Config.Repos[item.Dir]; ok && dir.Alias != "" {
			alias = dir.Alias
		}
		style := lipgloss.NewStyle().Foreground(lipgloss.Color("252"))
		if isCur {
			style = style.Background(lipgloss.Color("236")).Bold(true)
		}
		return style.Render(fmt.Sprintf("%s    ▸ %s", curMark, alias))

	case KindInstanceService:
		icon := "○"
		color := "244" // dim
		if m.RunningWindows[item.TmuxName] {
			icon = "●"
			color = "35" // green
		}
		if m.Stopping[item.TmuxName] {
			icon = "◌"
			color = "11" // yellow
		}
		if m.Starting[item.TmuxName] {
			icon = "◉"
			color = "33" // blue
		}
		style := lipgloss.NewStyle().Foreground(lipgloss.Color(color))
		if isCur {
			style = style.Background(lipgloss.Color("236"))
		}
		return style.Render(fmt.Sprintf("%s      %s %s", curMark, icon, item.Svc))
	}
	return ""
}

// ── Actions ──

func (m *Model) doToggle() {
	item := m.CurrentItem()
	if item == nil {
		return
	}
	switch item.Kind {
	case KindCombo, KindInstance, KindInstanceDir:
		m.ToggleCollapse()
	case KindInstanceService:
		if m.IsRunning(item.TmuxName) {
			m.doStop()
		} else {
			m.doStart()
		}
	}
}

func (m *Model) doStart() {
	item := m.CurrentItem()
	if item == nil {
		return
	}
	switch item.Kind {
	case KindInstanceService:
		if item.IsMain {
			m.startMainService(item.Dir, item.Svc)
		} else {
			m.startWtService(item.Dir, item.Svc, item.WtKey, item.TmuxName)
		}
	case KindInstance:
		m.startInstance(item.Branch, item.IsMain)
	case KindInstanceDir:
		if strings.HasPrefix(item.Dir, "_global:") {
			svcName := strings.TrimPrefix(item.Dir, "_global:")
			m.startGlobalService(svcName, item.Branch, item.IsMain)
		} else {
			m.startDir(item.Dir, item.Branch, item.WtKey, item.IsMain)
		}
	}
}

func (m *Model) doStop() {
	item := m.CurrentItem()
	if item == nil {
		return
	}
	switch item.Kind {
	case KindInstanceService:
		if !m.IsRunning(item.TmuxName) {
			m.SetMessage("nothing to stop")
			return
		}
		m.Stopping[item.TmuxName] = true
		svcSession := m.SvcSession()
		name := item.TmuxName
		go tmux.GracefulStop(svcSession, name)
		m.SetMessage(fmt.Sprintf("stopping: %s...", item.TmuxName))
	case KindInstance:
		m.stopInstance(item.Branch, item.IsMain)
	case KindInstanceDir:
		m.stopDir(item.Dir, item.Branch, item.IsMain)
	}
}

func (m *Model) doRestart() {
	item := m.CurrentItem()
	if item == nil {
		return
	}
	if item.Kind == KindInstanceService {
		if m.IsRunning(item.TmuxName) {
			svcSess := m.SvcSession()
			tmux.GracefulStop(svcSess, item.TmuxName)
			delete(m.RunningWindows, item.TmuxName)
		}
		m.doStart()
		m.SwapPending = true
		m.SetMessage(fmt.Sprintf("restarting: %s", item.TmuxName))
	}
}

func (m *Model) startMainService(dirName, svcName string) {
	alias := dirName
	if dir, ok := m.Config.Repos[dirName]; ok && dir.Alias != "" {
		alias = dir.Alias
	}
	tmuxName := fmt.Sprintf("%s~%s", alias, svcName)
	if tmux.WindowExists(m.SvcSession(), tmuxName) {
		m.SetMessage(tmuxName + " already running")
		return
	}

	dirPath := m.DirPath(dirName)
	dir := m.Config.Repos[dirName]
	if dir == nil {
		return
	}
	svc := dir.Services[svcName]
	if svc == nil || svc.Cmd == "" {
		return
	}

	cmd := fmt.Sprintf("cd '%s'", dirPath)
	if svc.PreStart != "" {
		cmd += " && " + svc.PreStart
	} else if dir.PreStart != "" {
		cmd += " && " + dir.PreStart
	}
	cmd += fmt.Sprintf(" && export BIND_IP=%s", m.MainBindIP)
	cmd += " && " + svc.Cmd
	if svc.Env != "" {
		cmd = svc.Env + " " + cmd
	} else if dir.Env != "" {
		cmd = dir.Env + " " + cmd
	}

	m.Starting[tmuxName] = true
	svcSession := m.SvcSession()
	go func() {
		tmux.CreateSessionIfNeeded(svcSession)
		tmux.NewWindow(svcSession, tmuxName, cmd)
	}()
	m.SetMessage(fmt.Sprintf("starting: %s...", tmuxName))
}

func (m *Model) startWtService(dirName, svcName, wtKey, tmuxName string) {
	wt, ok := m.Worktrees[wtKey]
	if !ok {
		m.SetMessage("worktree not found")
		return
	}
	if tmux.WindowExists(m.SvcSession(), tmuxName) {
		m.SetMessage(tmuxName + " already running")
		return
	}

	dir := m.Config.Repos[dirName]
	if dir == nil {
		return
	}
	svc := dir.Services[svcName]
	if svc == nil || svc.Cmd == "" {
		return
	}

	cmd := fmt.Sprintf("cd '%s'", wt.Path)
	if svc.PreStart != "" {
		cmd += " && " + svc.PreStart
	} else if dir.PreStart != "" {
		cmd += " && " + dir.PreStart
	}
	cmd += fmt.Sprintf(" && export BIND_IP=%s", wt.BindIP)
	cmd += " && " + svc.Cmd
	if svc.Env != "" {
		cmd = svc.Env + " " + cmd
	} else if dir.Env != "" {
		cmd = dir.Env + " " + cmd
	}

	m.Starting[tmuxName] = true
	svcSession := m.SvcSession()
	go func() {
		tmux.CreateSessionIfNeeded(svcSession)
		tmux.NewWindow(svcSession, tmuxName, cmd)
	}()
	m.SetMessage(fmt.Sprintf("starting: %s...", tmuxName))
}

func (m *Model) startInstance(branch string, isMain bool) {
	started := 0
	allWs := m.Config.AllWorkspaces()
	comboName := m.FindParentCombo(m.Cursor)
	entries := allWs[comboName]

	for _, entry := range entries {
		d, s, ok := m.Config.FindServiceEntryQuiet(entry)
		if !ok {
			continue
		}
		alias := d
		if dir, ok := m.Config.Repos[d]; ok && dir.Alias != "" {
			alias = dir.Alias
		}
		var tmuxName string
		if isMain {
			tmuxName = fmt.Sprintf("%s~%s", alias, s)
		} else {
			tmuxName = m.WtTmuxName(d, s, branch)
		}
		if !m.IsRunning(tmuxName) {
			if isMain {
				m.startMainService(d, s)
			} else {
				// Find wt_key for this dir
				for wtKey, wt := range m.Worktrees {
					if wt.ParentDir == d && WorkspaceBranch(wt) == branch {
						m.startWtService(d, s, wtKey, tmuxName)
						break
					}
				}
			}
			started++
		}
	}
	m.SetMessage(fmt.Sprintf("started %d services", started))
}

func (m *Model) startDir(dirName, branch, wtKey string, isMain bool) {
	dir := m.Config.Repos[dirName]
	if dir == nil {
		return
	}
	started := 0
	for _, svcName := range dir.ServiceOrder {
		if isMain {
			m.startMainService(dirName, svcName)
		} else {
			tmuxName := m.WtTmuxName(dirName, svcName, branch)
			m.startWtService(dirName, svcName, wtKey, tmuxName)
		}
		started++
	}
	m.SetMessage(fmt.Sprintf("started %d services for %s", started, dirName))
}

func (m *Model) startGlobalService(svcName, branch string, isMain bool) {
	gs, ok := m.Config.GlobalServices[svcName]
	if !ok {
		return
	}
	var tmuxName string
	if isMain {
		tmuxName = fmt.Sprintf("_global~%s", svcName)
	} else {
		tmuxName = fmt.Sprintf("_global~%s~%s", svcName, services.BranchSafe(branch))
	}
	if tmux.WindowExists(m.SvcSession(), tmuxName) {
		m.SetMessage(svcName + " already running")
		return
	}

	var wsDir string
	if isMain {
		wsDir = m.DirPath(m.DirNames[0]) + "/.."
	} else {
		configDir := filepath.Dir(m.ConfigPath)
		wsDir = filepath.Join(configDir, "workspace--"+branch)
	}

	cmd := fmt.Sprintf("cd '%s' && %s", wsDir, gs.Cmd)
	svcSession := m.SvcSession()
	go func() {
		tmux.CreateSessionIfNeeded(svcSession)
		tmux.NewWindowAutoclose(svcSession, tmuxName, cmd)
	}()
	m.Starting[tmuxName] = true
	m.SetMessage(fmt.Sprintf("starting: %s", svcName))
}

func (m *Model) stopInstance(branch string, isMain bool) {
	allWs := m.Config.AllWorkspaces()
	comboName := m.FindParentCombo(m.Cursor)
	entries := allWs[comboName]

	var svcs []string
	for _, entry := range entries {
		d, s, ok := m.Config.FindServiceEntryQuiet(entry)
		if !ok {
			continue
		}
		alias := d
		if dir, ok := m.Config.Repos[d]; ok && dir.Alias != "" {
			alias = dir.Alias
		}
		var tmuxName string
		if isMain {
			tmuxName = fmt.Sprintf("%s~%s", alias, s)
		} else {
			tmuxName = m.WtTmuxName(d, s, branch)
		}
		if m.IsRunning(tmuxName) {
			svcs = append(svcs, tmuxName)
		}
	}

	if len(svcs) == 0 {
		m.SetMessage("nothing to stop")
		return
	}
	for _, s := range svcs {
		m.Stopping[s] = true
	}
	svcSession := m.SvcSession()
	go func() {
		for _, svc := range svcs {
			tmux.GracefulStop(svcSession, svc)
		}
	}()
	m.SetMessage(fmt.Sprintf("stopping %d services...", len(svcs)))
}

func (m *Model) stopDir(dirName, branch string, isMain bool) {
	dir := m.Config.Repos[dirName]
	if dir == nil {
		return
	}
	alias := dirName
	if dir.Alias != "" {
		alias = dir.Alias
	}
	var svcs []string
	for _, svcName := range dir.ServiceOrder {
		var tmuxName string
		if isMain {
			tmuxName = fmt.Sprintf("%s~%s", alias, svcName)
		} else {
			tmuxName = m.WtTmuxName(dirName, svcName, branch)
		}
		if m.IsRunning(tmuxName) {
			svcs = append(svcs, tmuxName)
		}
	}
	if len(svcs) == 0 {
		m.SetMessage("nothing to stop")
		return
	}
	for _, s := range svcs {
		m.Stopping[s] = true
	}
	svcSession := m.SvcSession()
	go func() {
		for _, svc := range svcs {
			tmux.GracefulStop(svcSession, svc)
		}
	}()
	m.SetMessage(fmt.Sprintf("stopping %d services...", len(svcs)))
}

func (m *Model) openEditor() {
	item := m.CurrentItem()
	if item == nil {
		return
	}
	path := m.selectedWorkDir()
	if path == "" {
		m.SetMessage("no selection")
		return
	}
	if err := runCmd("zed", path); err == nil {
		m.SetMessage("opened in zed")
	} else if err := runCmd("code", path); err == nil {
		m.SetMessage("opened in code")
	} else {
		m.SetMessage("no editor found")
	}
}

func (m *Model) reloadConfig() {
	cfg, err := config.Load(m.ConfigPath)
	if err != nil {
		m.SetMessage(fmt.Sprintf("reload failed: %v", err))
		return
	}
	m.Config = cfg
	m.Session = cfg.Session
	m.DirNames = cfg.RepoOrder
	m.Combos = nil
	for name := range cfg.AllWorkspaces() {
		m.Combos = append(m.Combos, name)
	}
	m.RebuildComboTree()
	m.ClampCursor()
	m.SetMessage("config reloaded")
}

func (m *Model) selectedWorkDir() string {
	item := m.CurrentItem()
	if item == nil {
		return ""
	}
	switch item.Kind {
	case KindInstanceDir, KindInstanceService:
		if item.IsMain {
			return m.DirPath(item.Dir)
		}
		if wt, ok := m.Worktrees[item.WtKey]; ok {
			return wt.Path
		}
	case KindInstance:
		if item.IsMain {
			configDir := filepath.Dir(m.ConfigPath)
			return filepath.Join(configDir, "workspace--"+m.Config.GlobalDefaultBranch())
		}
		configDir := filepath.Dir(m.ConfigPath)
		return filepath.Join(configDir, "workspace--"+item.Branch)
	}
	return ""
}

func (m *Model) cycleComboLog(delta int) {
	// Cycle through running services visible at cursor
	item := m.CurrentItem()
	if item == nil {
		return
	}
	var svcs []string
	switch item.Kind {
	case KindInstanceDir:
		dir := m.Config.Repos[item.Dir]
		if dir == nil {
			return
		}
		for _, s := range dir.ServiceOrder {
			var tn string
			if item.IsMain {
				alias := item.Dir
				if dir.Alias != "" {
					alias = dir.Alias
				}
				tn = fmt.Sprintf("%s~%s", alias, s)
			} else {
				tn = m.WtTmuxName(item.Dir, s, item.Branch)
			}
			if m.IsRunning(tn) {
				svcs = append(svcs, tn)
			}
		}
	case KindInstance:
		// All running svcs in this instance
		allWs := m.Config.AllWorkspaces()
		comboName := m.FindParentCombo(m.Cursor)
		entries := allWs[comboName]
		for _, entry := range entries {
			d, s, ok := m.Config.FindServiceEntryQuiet(entry)
			if !ok {
				continue
			}
			alias := d
			if dir, ok := m.Config.Repos[d]; ok && dir.Alias != "" {
				alias = dir.Alias
			}
			var tn string
			if item.IsMain {
				tn = fmt.Sprintf("%s~%s", alias, s)
			} else {
				tn = m.WtTmuxName(d, s, item.Branch)
			}
			if m.IsRunning(tn) {
				svcs = append(svcs, tn)
			}
		}
	default:
		return
	}

	if len(svcs) == 0 {
		return
	}
	m.ComboLogIdx = (m.ComboLogIdx + delta + len(svcs)) % len(svcs)
	m.SwapPending = true
}

func (m *Model) logServiceName() string {
	item := m.CurrentItem()
	if item == nil {
		return ""
	}
	switch item.Kind {
	case KindInstanceService:
		return item.TmuxName
	case KindInstanceDir, KindInstance:
		// Find nth running service via ComboLogIdx
		var svcs []string
		if item.Kind == KindInstanceDir {
			dir := m.Config.Repos[item.Dir]
			if dir == nil {
				return ""
			}
			for _, s := range dir.ServiceOrder {
				var tn string
				if item.IsMain {
					alias := item.Dir
					if dir.Alias != "" {
						alias = dir.Alias
					}
					tn = fmt.Sprintf("%s~%s", alias, s)
				} else {
					tn = m.WtTmuxName(item.Dir, s, item.Branch)
				}
				if m.IsRunning(tn) {
					svcs = append(svcs, tn)
				}
			}
		}
		if len(svcs) > 0 {
			idx := m.ComboLogIdx % len(svcs)
			return svcs[idx]
		}
	}
	return ""
}

// ── Split Pane Management ──

func (m *Model) SetupSplit() {
	if m.TuiWindowID == "" {
		return
	}
	m.TuiPaneID = tmux.CurrentPaneID()
	placeholder := "echo; echo '  Select a running service'; echo; echo '  j/k navigate · Tab focus · n/N cycle'; stty -echo; tail -f /dev/null"
	tmux.SplitWindowRight(75, placeholder)
	allPanes := tmux.ListPaneIDs(m.TuiWindowID)
	for _, p := range allPanes {
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

	// Swap old back
	if m.JoinedSvc != "" && m.RightPaneID != "" {
		if tmux.WindowExists(svcSess, m.JoinedSvc) {
			_ = tmux.SwapPane(svcSess, m.JoinedSvc, m.RightPaneID)
			m.redetectRightPane()
		}
		m.JoinedSvc = ""
	}

	// Swap new in
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

func (m *Model) doRecreateDB() {
	item := m.CurrentItem()
	if item == nil {
		return
	}
	wsBranch := item.Branch
	if item.IsMain {
		wsBranch = m.Config.GlobalDefaultBranch()
	}
	branchSafe := services.BranchSafe(wsBranch)

	var dbNames []string
	for _, dir := range m.Config.Repos {
		if dir.WT() == nil {
			continue
		}
		for _, sref := range dir.WT().SharedServices {
			if sref.DBName != "" {
				dbName := strings.ReplaceAll(sref.DBName, "{{branch_safe}}", branchSafe)
				dbName = strings.ReplaceAll(dbName, "{{branch}}", wsBranch)
				dbNames = append(dbNames, dbName)
			}
		}
		for _, dbTpl := range dir.WT().Databases {
			dbName := strings.ReplaceAll(dbTpl, "{{branch_safe}}", branchSafe)
			dbName = strings.ReplaceAll(dbName, "{{branch}}", wsBranch)
			dbNames = append(dbNames, m.Config.Session+"_"+dbName)
		}
	}
	if len(dbNames) == 0 {
		m.SetMessage("no databases configured")
		return
	}

	host, port, user, pw := "localhost", uint16(5432), "postgres", "postgres"
	for _, svc := range m.Config.SharedServices {
		if svc.DBUser != "" {
			if svc.Host != "" {
				host = svc.Host
			}
			port = services.FirstPortFromList(svc.Ports)
			if port == 0 {
				port = 5432
			}
			user = svc.DBUser
			pw = svc.DBPassword
			break
		}
	}

	count := len(dbNames)
	go func() {
		services.DropSharedDBsBatch(host, port, dbNames, user, pw)
		services.CreateSharedDBsBatch(host, port, dbNames, user, pw)
	}()
	m.SetMessage(fmt.Sprintf("recreating %d databases for %s...", count, wsBranch))
}

func (m *Model) doOpenURL() {
	item := m.CurrentItem()
	if item == nil || (item.Kind != KindInstanceService && item.Kind != KindInstanceDir) {
		m.SetMessage("select a service to open")
		return
	}

	dir := m.Config.Repos[item.Dir]
	if dir == nil {
		return
	}
	var port *uint16
	if item.Svc != "" {
		if svc, ok := dir.Services[item.Svc]; ok {
			port = svc.ProxyPort
		}
	}
	if port == nil {
		port = dir.ProxyPort
	}
	if port == nil {
		m.SetMessage("no proxy_port configured")
		return
	}

	bindIP := m.MainBindIP
	if !item.IsMain {
		wsKey := "ws-" + strings.ReplaceAll(item.Branch, "/", "-")
		allocs := services.LoadIPAllocations()
		if ip, ok := allocs[wsKey]; ok {
			bindIP = ip
		}
	}

	url := fmt.Sprintf("http://%s:%d", bindIP, *port)
	_ = exec.Command("open", url).Start()
	m.SetMessage(fmt.Sprintf("opening %s", url))
}

func (m *Model) ensureSplit() {
	if m.TuiWindowID == "" {
		return
	}
	panes := tmux.ListPaneIDs(m.TuiWindowID)
	if len(panes) < 2 {
		if m.JoinedSvc != "" && strings.HasPrefix(m.JoinedSvc, "_global~") {
			tmux.KillWindow(m.SvcSession(), m.JoinedSvc)
		}
		placeholder := "echo; echo '  Select a running service'; echo; echo '  j/k navigate · Tab focus · n/N cycle'; stty -echo; tail -f /dev/null"
		tmux.SplitWindowRight(75, placeholder)
		allPanes := tmux.ListPaneIDs(m.TuiWindowID)
		for _, p := range allPanes {
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

func runCmd(name string, args ...string) error {
	return exec.Command(name, args...).Start()
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
