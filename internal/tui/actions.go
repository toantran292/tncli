package tui

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tmux"
)

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
		if item.Dir == "_ws" {
			m.startWsService(item.Svc, item.TmuxName, item.Branch, item.IsMain)
		} else if item.IsMain {
			m.startMainService(item.Dir, item.Svc)
		} else {
			m.startWtService(item.Dir, item.Svc, item.WtKey, item.TmuxName)
		}
	case KindInstance:
		m.startInstance(item.Branch, item.IsMain)
	case KindInstanceDir:
		m.startDir(item.Dir, item.Branch, item.WtKey, item.IsMain)
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
		m.unjoinIfDisplayed(item.TmuxName)
		svcSession := m.SvcSession()
		name := item.TmuxName
		go tmux.GracefulStop(svcSession, name)
		tmux.DisplayMessage(fmt.Sprintf(" stopping: %s... ", item.TmuxName))
	case KindInstance:
		m.stopInstance(item.Branch, item.IsMain)
	case KindInstanceDir:
		m.stopDir(item.Dir, item.Branch, item.IsMain)
	}
}

func (m *Model) unjoinIfDisplayed(tmuxName string) {
	if m.JoinedSvc != tmuxName {
		return
	}
	svcSess := m.SvcSession()
	if m.RightPaneID != "" && tmux.WindowExists(svcSess, tmuxName) {
		_ = tmux.SwapPane(svcSess, tmuxName, m.RightPaneID)
		m.JoinedSvc = ""
		m.redetectRightPane()
		if m.RightPaneID != "" {
			tmux.SetPaneTitle(m.RightPaneID, "service")
		}
	}
}

func (m *Model) doRestart() {
	item := m.CurrentItem()
	if item == nil || item.Kind != KindInstanceService {
		return
	}
	if m.IsRunning(item.TmuxName) {
		tmux.GracefulStop(m.SvcSession(), item.TmuxName)
		delete(m.RunningWindows, item.TmuxName)
	}
	m.doStart()
	m.SwapPending = true
	m.SetMessage(fmt.Sprintf("restarting: %s", item.TmuxName))
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

	dir := m.Config.Repos[dirName]
	if dir == nil {
		return
	}
	svc := dir.Services[svcName]
	if svc == nil || svc.Cmd == "" {
		return
	}

	configDir := filepath.Dir(m.ConfigPath)
	defaultBranch := m.Config.GlobalDefaultBranch()
	wsKey := "ws-" + defaultBranch
	services.ClaimBlock(configDir, wsKey)
	port := 0
	if svc.HasPort() {
		port = services.Port(configDir, wsKey, alias+"~"+svcName)
	}
	cmd := buildServiceCmd(m.DirPath(dirName), dir, svc, port)
	m.Starting[tmuxName] = true
	m.SwapPending = true
	tmux.DisplayMessage(fmt.Sprintf(" starting: %s... ", tmuxName))
	svcSession := m.SvcSession()
	cfg := m.Config
	go func() {
		services.RegenerateWorkspaceEnv(configDir, cfg, defaultBranch)
		tmux.CreateSessionIfNeeded(svcSession)
		tmux.NewWindow(svcSession, tmuxName, cmd)
	}()
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

	alias := dirName
	if dir.Alias != "" {
		alias = dir.Alias
	}
	configDir := filepath.Dir(m.ConfigPath)
	branch := wt.Branch
	wsKey := "ws-" + strings.ReplaceAll(branch, "/", "-")
	services.ClaimBlock(configDir, wsKey)
	port := 0
	if svc.HasPort() {
		port = services.Port(configDir, wsKey, alias+"~"+svcName)
	}
	cmd := buildServiceCmd(wt.Path, dir, svc, port)
	m.Starting[tmuxName] = true
	m.SwapPending = true
	tmux.DisplayMessage(fmt.Sprintf(" starting: %s... ", tmuxName))
	svcSession := m.SvcSession()
	cfg := m.Config
	go func() {
		services.RegenerateWorkspaceEnv(configDir, cfg, branch)
		tmux.CreateSessionIfNeeded(svcSession)
		tmux.NewWindow(svcSession, tmuxName, cmd)
	}()
}

func (m *Model) startInstance(branch string, isMain bool) {
	started := 0
	entries := m.Config.AllWorkspaces()[m.FindParentCombo(m.Cursor)]
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
		if isMain {
			tn = fmt.Sprintf("%s~%s", alias, s)
		} else {
			tn = m.WtTmuxName(d, s, branch)
		}
		if !m.IsRunning(tn) {
			if isMain {
				m.startMainService(d, s)
			} else {
				for wtKey, wt := range m.Worktrees {
					if wt.ParentDir == d && WorkspaceBranch(wt) == branch {
						m.startWtService(d, s, wtKey, tn)
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
			m.startWtService(dirName, svcName, wtKey, m.WtTmuxName(dirName, svcName, branch))
		}
		started++
	}
	m.SetMessage(fmt.Sprintf("started %d services for %s", started, dirName))
}

func (m *Model) startWsService(svcName, tmuxName, branch string, isMain bool) {
	if tmux.WindowExists(m.SvcSession(), tmuxName) {
		tmux.DisplayMessage(fmt.Sprintf(" %s already running ", svcName))
		return
	}
	svc, ok := m.Config.WsServices[svcName]
	if !ok || svc.Cmd == "" {
		return
	}

	configDir := filepath.Dir(m.ConfigPath)
	var wsDir string
	if isMain {
		wsDir = filepath.Join(configDir, "workspace--"+m.Config.GlobalDefaultBranch())
	} else {
		wsDir = filepath.Join(configDir, "workspace--"+branch)
	}

	cmd := fmt.Sprintf("cd '%s' && %s", wsDir, svc.Cmd)
	m.Starting[tmuxName] = true
	m.SwapPending = true
	tmux.DisplayMessage(fmt.Sprintf(" starting: %s... ", svcName))
	svcSession := m.SvcSession()
	go func() {
		tmux.CreateSessionIfNeeded(svcSession)
		tmux.NewWindow(svcSession, tmuxName, cmd)
	}()
}

func (m *Model) stopInstance(branch string, isMain bool) {
	entries := m.Config.AllWorkspaces()[m.FindParentCombo(m.Cursor)]
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
		tn := fmt.Sprintf("%s~%s", alias, s)
		if !isMain {
			tn = m.WtTmuxName(d, s, branch)
		}
		if m.IsRunning(tn) {
			svcs = append(svcs, tn)
		}
	}
	m.stopServices(svcs)
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
		tn := fmt.Sprintf("%s~%s", alias, svcName)
		if !isMain {
			tn = m.WtTmuxName(dirName, svcName, branch)
		}
		if m.IsRunning(tn) {
			svcs = append(svcs, tn)
		}
	}
	m.stopServices(svcs)
}

func (m *Model) stopServices(svcs []string) {
	if len(svcs) == 0 {
		m.SetMessage("nothing to stop")
		return
	}
	for _, s := range svcs {
		m.Stopping[s] = true
		m.unjoinIfDisplayed(s)
	}
	svcSession := m.SvcSession()
	go func() {
		for _, svc := range svcs {
			tmux.GracefulStop(svcSession, svc)
		}
	}()
	m.SetMessage(fmt.Sprintf("stopping %d services...", len(svcs)))
}

// buildServiceCmd constructs the full shell command for starting a service.
func buildServiceCmd(workDir string, dir *config.Dir, svc *config.Service, port int) string {
	cmd := fmt.Sprintf("cd '%s'", workDir)
	if svc.PreStart != "" {
		cmd += " && " + svc.PreStart
	} else if dir.PreStart != "" {
		cmd += " && " + dir.PreStart
	}
	cmd += " && set -a && source .env.local 2>/dev/null; set +a"
	cmd += " && export BIND_IP=localhost"
	if port > 0 {
		cmd += fmt.Sprintf(" && export PORT=%d", port)
	}
	cmd += " && " + svc.Cmd
	if svc.Env != "" {
		cmd = svc.Env + " " + cmd
	} else if dir.ShellEnv != "" {
		cmd = dir.ShellEnv + " " + cmd
	}
	return cmd
}
