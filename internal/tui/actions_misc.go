package tui

import (
	"fmt"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tmux"
)

func (m *Model) openEditor() {
	path := m.selectedWorkDir()
	if path == "" {
		tmux.DisplayMessage(" no selection")
		return
	}
	if runCmd("zed", path) == nil {
		tmux.DisplayMessage(" opened in zed")
	} else if runCmd("code", path) == nil {
		tmux.DisplayMessage(" opened in code")
	} else {
		tmux.DisplayMessage(" no editor found")
	}
}

func (m *Model) reloadConfig() {
	cfg, err := config.Load(m.ConfigPath)
	if err != nil {
		tmux.DisplayMessage(fmt.Sprintf(" reload failed: %v", err))
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
	tmux.DisplayMessage(" config reloaded")
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
		configDir := filepath.Dir(m.ConfigPath)
		if item.IsMain {
			return filepath.Join(configDir, "workspace--"+m.Config.GlobalDefaultBranch())
		}
		return filepath.Join(configDir, "workspace--"+item.Branch)
	}
	return ""
}

func (m *Model) cycleComboLog(delta int) {
	item := m.CurrentItem()
	if item == nil {
		return
	}
	svcs := m.runningServicesForItem(item)
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
	if item.Kind == KindInstanceService {
		return item.TmuxName
	}
	svcs := m.runningServicesForItem(item)
	if len(svcs) > 0 {
		return svcs[m.ComboLogIdx%len(svcs)]
	}
	return ""
}

func (m *Model) runningServicesForItem(item *ComboItem) []string {
	var svcs []string
	switch item.Kind {
	case KindInstanceDir:
		dir := m.Config.Repos[item.Dir]
		if dir == nil {
			return nil
		}
		for _, s := range dir.ServiceOrder {
			tn := fmt.Sprintf("%s~%s", m.aliasFor(item.Dir), s)
			if !item.IsMain {
				tn = m.WtTmuxName(item.Dir, s, item.Branch)
			}
			if m.IsRunning(tn) {
				svcs = append(svcs, tn)
			}
		}
	case KindInstance:
		entries := m.Config.AllWorkspaces()[m.FindParentCombo(m.Cursor)]
		for _, entry := range entries {
			d, s, ok := m.Config.FindServiceEntryQuiet(entry)
			if !ok {
				continue
			}
			tn := fmt.Sprintf("%s~%s", m.aliasFor(d), s)
			if !item.IsMain {
				tn = m.WtTmuxName(d, s, item.Branch)
			}
			if m.IsRunning(tn) {
				svcs = append(svcs, tn)
			}
		}
	}
	return svcs
}

func (m *Model) aliasFor(dirName string) string {
	if dir, ok := m.Config.Repos[dirName]; ok && dir.Alias != "" {
		return dir.Alias
	}
	return dirName
}

func (m *Model) popupDBMenu() {
	item := m.CurrentItem()
	if item == nil {
		return
	}
	m.popupMenu("Database", []string{"create", "recreate (drop+create)", "drop"},
		PendingPopup{Kind: PopupDBMenu, Branch: item.Branch, IsMain: item.IsMain})
}

func (m *Model) resolveDBInfo() (dbNames []string, host string, port uint16, user, pw, wsBranch string) {
	item := m.CurrentItem()
	if item == nil {
		return
	}
	wsBranch = item.Branch
	if item.IsMain {
		wsBranch = m.Config.GlobalDefaultBranch()
	}
	branchSafe := services.BranchSafe(wsBranch)

	for _, dir := range m.Config.Repos {
		if !dir.HasWorktreeConfig() {
			continue
		}
		for _, sref := range dir.SharedSvcRefs {
			if sref.DBName != "" {
				dbName := strings.ReplaceAll(sref.DBName, "{{branch_safe}}", branchSafe)
				dbName = strings.ReplaceAll(dbName, "{{branch}}", wsBranch)
				dbNames = append(dbNames, dbName)
			}
		}
		for _, dbTpl := range dir.Databases {
			dbName := strings.ReplaceAll(dbTpl, "{{branch_safe}}", branchSafe)
			dbName = strings.ReplaceAll(dbName, "{{branch}}", wsBranch)
			dbNames = append(dbNames, m.Config.Session+"_"+dbName)
		}
	}

	host, user, pw = "localhost", "postgres", "postgres"
	port = uint16(services.SharedPort("postgres"))
	if port == 0 {
		port = 5432
	}
	for _, svc := range m.Config.SharedServices {
		if svc.DBUser != "" {
			if svc.Host != "" {
				host = svc.Host
			}
			user, pw = svc.DBUser, svc.DBPassword
			break
		}
	}
	return
}

func (m *Model) doDBAction(action string) {
	dbNames, host, port, user, pw, wsBranch := m.resolveDBInfo()
	if len(dbNames) == 0 {
		tmux.DisplayMessage(" no databases configured")
		return
	}
	count := len(dbNames)
	switch action {
	case "create":
		go func() {
			services.CreateSharedDBsBatch(host, port, dbNames, user, pw)
		}()
		tmux.DisplayMessage(fmt.Sprintf(" creating %d databases for %s...", count, wsBranch))
	case "recreate (drop+create)":
		go func() {
			services.DropSharedDBsBatch(host, port, dbNames, user, pw)
			services.CreateSharedDBsBatch(host, port, dbNames, user, pw)
		}()
		tmux.DisplayMessage(fmt.Sprintf(" recreating %d databases for %s...", count, wsBranch))
	case "drop":
		go func() {
			services.DropSharedDBsBatch(host, port, dbNames, user, pw)
		}()
		tmux.DisplayMessage(fmt.Sprintf(" dropping %d databases for %s...", count, wsBranch))
	}
}

func (m *Model) doOpenURL() {
	item := m.CurrentItem()
	if item == nil || (item.Kind != KindInstanceService && item.Kind != KindInstanceDir) {
		tmux.DisplayMessage(" select a service to open")
		return
	}

	dir := m.Config.Repos[item.Dir]
	if dir == nil {
		return
	}
	alias := item.Dir
	if dir.Alias != "" {
		alias = dir.Alias
	}

	svcName := item.Svc
	if svcName == "" && len(dir.ServiceOrder) > 0 {
		svcName = dir.ServiceOrder[0]
	}
	if svcName == "" {
		tmux.DisplayMessage(" no service to open")
		return
	}

	configDir := filepath.Dir(m.ConfigPath)
	wsKey := "ws-" + m.Config.GlobalDefaultBranch()
	if !item.IsMain {
		wsKey = "ws-" + strings.ReplaceAll(item.Branch, "/", "-")
	}
	svcKey := alias + "~" + svcName
	port := services.Port(configDir, wsKey, svcKey)
	if port == 0 {
		tmux.DisplayMessage(" no port allocated")
		return
	}

	url := fmt.Sprintf("http://localhost:%d", port)
	_ = exec.Command("open", url).Start()
	tmux.DisplayMessage(fmt.Sprintf(" opening %s ", url))
}

func (m *Model) doStopAll() {
	for svc := range m.RunningWindows {
		m.Stopping[svc] = true
	}
	tmux.DisplayMessage(" stopping all services...")
	exe, _ := exec.LookPath("tncli")
	if exe == "" {
		return
	}
	go func() {
		_ = exec.Command(exe, "stop").Run()
	}()
}

func runCmd(name string, args ...string) error {
	return exec.Command(name, args...).Start()
}

func (m *Model) popupShortcutsAction() {
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

	m.popupShortcuts()
}
