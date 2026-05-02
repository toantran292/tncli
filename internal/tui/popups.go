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

func (m *Model) popupMenu(title string, options []string, popup PendingPopup) {
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
	var lines []string
	lines = append(lines, fmt.Sprintf("  Shared Services (%s)", project))
	lines = append(lines, "")
	for name, svc := range m.Config.SharedServices {
		host := svc.Host
		if host == "" {
			host = "-"
		}
		var ports []string
		for _, p := range svc.Ports {
			ports = append(ports, strings.SplitN(p, ":", 2)[0])
		}
		cap := ""
		if svc.Capacity != nil {
			cap = fmt.Sprintf(" (cap:%d)", *svc.Capacity)
		}
		lines = append(lines, fmt.Sprintf("  %-16s %-22s :%s%s", name, host, strings.Join(ports, ", "), cap))
	}
	content := strings.Join(lines, "\n")
	cmd := fmt.Sprintf("echo '%s' | less -R --prompt='Shared Services (q to close)'", escSh(content))
	tmux.DisplayPopup("60%", "50%", cmd)
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

// ── Workspace Builders ──

func (m *Model) buildWsSelect(wsBranch string) {
	wsName := m.wsName
	allWs := m.Config.AllWorkspaces()
	entries := allWs[wsName]

	var uniqueDirs []string
	for _, entry := range entries {
		d, _, ok := m.Config.FindServiceEntryQuiet(entry)
		if ok && !containsStr2(uniqueDirs, d) {
			uniqueDirs = append(uniqueDirs, d)
		}
	}

	// Build items for popup
	var itemsStr []string
	for _, dirName := range uniqueDirs {
		alias := dirName
		if dir, ok := m.Config.Repos[dirName]; ok && dir.Alias != "" {
			alias = dir.Alias
		}
		base := m.Config.DefaultBranchFor(dirName)
		path := m.DirPath(dirName)
		itemsStr = append(itemsStr, fmt.Sprintf("%s|%s|%s|%s", alias, base, wsBranch, path))
	}

	exe, _ := os.Executable()
	cmd := fmt.Sprintf("%s popup --type ws-select --data '%s'", exe, escSh(strings.Join(itemsStr, ",")))
	h := fmt.Sprintf("%d", min(len(uniqueDirs)+4, 20))
	title := fmt.Sprintf(" Create workspace: %s ", wsBranch)
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "55", Height: h, Title: title,
		BorderStyle: "fg=green", BorderLines: "rounded",
	}, cmd)
	m.pendingPopup = &PendingPopup{Kind: PopupWsRepoSelect, WsName: wsName, WsBranch: wsBranch}
}

func (m *Model) buildWsAddList(branch string) {
	var existingDirs []string
	for _, wt := range m.Worktrees {
		if WorkspaceBranch(wt) == branch {
			existingDirs = append(existingDirs, wt.ParentDir)
		}
	}

	var available []string
	for _, d := range m.DirNames {
		if !containsStr2(existingDirs, d) {
			alias := d
			if dir, ok := m.Config.Repos[d]; ok && dir.Alias != "" {
				alias = dir.Alias
			}
			available = append(available, fmt.Sprintf("%s\t%s", d, alias))
		}
	}

	if len(available) == 0 {
		m.SetMessage("all repos already in workspace")
		return
	}

	_ = os.Remove(popupResultFile)
	items := strings.Join(available, "\n")
	cmd := fmt.Sprintf("printf '%s' | fzf --prompt='Add repo> ' --with-nth=2 --delimiter='\t' | cut -f1 > %s",
		escSh(items), popupResultFile)
	tmux.DisplayPopup("50%", "40%", cmd)
	m.pendingPopup = &PendingPopup{Kind: PopupWsAdd, Branch: branch}
}

func (m *Model) buildWsRemoveList(branch string) {
	type repoEntry struct{ key, alias string }
	var repos []repoEntry
	for wtKey, wt := range m.Worktrees {
		if WorkspaceBranch(wt) == branch {
			alias := wt.ParentDir
			if dir, ok := m.Config.Repos[wt.ParentDir]; ok && dir.Alias != "" {
				alias = dir.Alias
			}
			repos = append(repos, repoEntry{wtKey, alias})
		}
	}
	if len(repos) == 0 {
		m.SetMessage("no repos to remove")
		return
	}

	_ = os.Remove(popupResultFile)
	var items []string
	for _, r := range repos {
		items = append(items, fmt.Sprintf("%s\t%s", r.key, r.alias))
	}
	input := strings.Join(items, "\n")
	cmd := fmt.Sprintf("printf '%s' | fzf --prompt='Remove repo> ' --with-nth=2 --delimiter='\t' | cut -f1 > %s",
		escSh(input), popupResultFile)
	tmux.DisplayPopup("50%", "40%", cmd)
	m.pendingPopup = &PendingPopup{Kind: PopupWsRemove}
}

// ── Poll Popup Result ──

func (m *Model) pollPopupResult() {
	popup := m.pendingPopup
	if popup == nil {
		return
	}

	data, err := os.ReadFile(popupResultFile)
	if err != nil {
		return // file not ready yet
	}
	_ = os.Remove(popupResultFile)
	m.pendingPopup = nil

	result := strings.TrimSpace(string(data))
	if result == "" {
		return
	}

	switch popup.Kind {
	case PopupBranchPicker:
		if popup.Checkout {
			if err := services.Checkout(m.selectedWorkDir(), result); err == nil {
				m.scanWorktrees()
				m.SetMessage(fmt.Sprintf("checked out %s", result))
			} else {
				m.SetMessage(fmt.Sprintf("checkout failed: %v", err))
			}
		}

	case PopupGitMenu:
		switch result {
		case "checkout branch":
			m.popupBranchPicker(popup.Dir, true)
		case "pull origin":
			branch := services.CurrentBranch(popup.Path)
			if branch == "" {
				branch = "main"
			}
			cmd := fmt.Sprintf("(git -C '%s' pull origin %s) 2>&1 | less -R --mouse +G", popup.Path, branch)
			tmux.DisplayPopup("70%", "50%", cmd)
		case "diff view":
			cmd := fmt.Sprintf("cd '%s' && git diff --color=always | less -R --mouse", popup.Path)
			tmux.DisplayPopup("90%", "90%", cmd)
		}

	case PopupGitPullAll:
		if result == "pull all repos" {
			var script strings.Builder
			script.WriteString("#!/bin/zsh\n")
			i := 0
			for _, dirName := range m.DirNames {
				path := m.DirPath(dirName)
				branch := m.Config.DefaultBranchFor(dirName)
				fmt.Fprintf(&script,
					"( cd '%s' && git pull origin %s > /tmp/tncli-pull-%d.log 2>&1 && echo '\\033[32m✓ %s\\033[0m' || echo '\\033[31m✗ %s\\033[0m'; cat /tmp/tncli-pull-%d.log; rm -f /tmp/tncli-pull-%d.log; echo ) &\n",
					path, branch, i, dirName, dirName, i, i)
				i++
			}
			script.WriteString("wait\necho '\\033[32m[Done]\\033[0m'\n")
			scriptPath := "/tmp/tncli-pull-all.sh"
			_ = os.WriteFile(scriptPath, []byte(script.String()), 0o755)
			log := "/tmp/tncli-pull-all.log"
			run := fmt.Sprintf("%s 2>&1 | tee '%s'; less -R --mouse +G '%s'; rm -f '%s' '%s'",
				scriptPath, log, log, log, scriptPath)
			tmux.DisplayPopup("80%", "80%", run)
		}

	case PopupShortcut:
		var idx int
		if _, err := fmt.Sscanf(result, "%d", &idx); err == nil && idx < len(m.shortcutItems) {
			shortcut := m.shortcutItems[idx]
			dir := m.selectedWorkDir()
			if dir != "" {
				m.runShortcutInPopup(shortcut.Cmd, shortcut.Desc, dir)
			}
		}

	case PopupWsEdit:
		switch result {
		case "Create new workspace":
			m.wsCreating = true
			m.popupInput("Workspace branch name:", PendingPopup{Kind: PopupNameInput, Context: "workspace"})
		case "Add repo":
			m.buildWsAddList(popup.Branch)
		case "Remove repo":
			m.buildWsRemoveList(popup.Branch)
		}

	case PopupWsAdd:
		if result != "" {
			m.addRepoToWorkspace(result, popup.Branch)
		}

	case PopupWsRemove:
		if result != "" {
			m.deleteWorktree(result)
		}

	case PopupWsRepoSelect:
		if strings.HasPrefix(result, "BRANCH_PICK:") {
			rest := strings.TrimPrefix(result, "BRANCH_PICK:")
			if idxStr, itemsData, ok := strings.Cut(rest, ":"); ok {
				var idx int
				fmt.Sscanf(idxStr, "%d", &idx)
				// Extract path for branch picker
				items := strings.Split(itemsData, ",")
				alias, path := "", ""
				if idx < len(items) {
					fields := strings.SplitN(items[idx], "|", 5)
					if len(fields) >= 4 {
						alias, path = fields[0], fields[3]
					}
				}
				m.openWsBranchPicker(popup.WsName, popup.WsBranch, itemsData, idx, alias, path)
			}
			return
		}
		// Parse selected entries
		var selected []services.DirBranch
		for _, line := range strings.Split(result, "\n") {
			line = strings.TrimSpace(line)
			if line == "" {
				continue
			}
			if a, b, ok := strings.Cut(line, ":"); ok {
				selected = append(selected, services.DirBranch{Name: strings.TrimSpace(a), Branch: strings.TrimSpace(b)})
			}
		}
		if len(selected) > 0 {
			m.wsName = popup.WsName
			m.startCreatePipeline(popup.WsName, popup.WsBranch, selected)
		}

	case PopupWsBranchPick:
		if result != "" {
			// Update target branch in items_data
			parts := strings.Split(popup.ItemsData, ",")
			if popup.Idx < len(parts) {
				fields := strings.SplitN(parts[popup.Idx], "|", 5)
				if len(fields) >= 4 {
					sel := "1"
					if len(fields) >= 5 {
						sel = fields[4]
					}
					parts[popup.Idx] = fmt.Sprintf("%s|%s|%s|%s|%s", fields[0], fields[1], result, fields[3], sel)
				}
			}
			m.reopenWsSelect(popup.WsName, popup.WsBranch, strings.Join(parts, ","))
		}

	case PopupNameInput:
		if result != "" {
			if popup.Context == "workspace" {
				m.buildWsSelect(result)
			} else if strings.HasPrefix(popup.Context, "branch:") {
				dir := strings.TrimPrefix(popup.Context, "branch:")
				if err := services.CheckoutNewBranch(m.DirPath(dir), result); err == nil {
					m.scanWorktrees()
					m.SetMessage(fmt.Sprintf("created branch %s in %s", result, dir))
				} else {
					m.SetMessage(fmt.Sprintf("create branch failed: %v", err))
				}
			}
		}

	case PopupConfirm:
		if strings.EqualFold(strings.TrimSpace(result), "y") {
			if strings.HasPrefix(popup.Action, "delete_ws:") {
				branch := strings.TrimPrefix(popup.Action, "delete_ws:")
				m.startDeletePipeline(branch)
			} else if popup.Action == "stop_all" {
				m.doStopAll()
			}
		} else {
			m.SetMessage("cancelled")
		}
	}
}

func (m *Model) openWsBranchPicker(wsName, wsBranch, itemsData string, idx int, alias, path string) {
	_ = os.Remove(popupResultFile)
	title := fmt.Sprintf(" %s — select branch ", alias)
	cmd := fmt.Sprintf(
		"printf '\\033[33m  Fetching branches...\\033[0m' && git -C '%s' fetch origin --prune -q 2>/dev/null; git -C '%s' branch -a --sort=-committerdate | sed 's/^[* ]*//' | sed 's|remotes/origin/||' | sort -u | fzf --prompt='Branch> ' --reverse > %s",
		path, path, popupResultFile)
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "50%", Height: "70%", Title: title,
		BorderStyle: "fg=magenta", BorderLines: "rounded",
	}, cmd)
	m.pendingPopup = &PendingPopup{
		Kind: PopupWsBranchPick, WsName: wsName, WsBranch: wsBranch,
		ItemsData: itemsData, Idx: idx,
	}
}

func (m *Model) reopenWsSelect(wsName, wsBranch, itemsData string) {
	exe, _ := os.Executable()
	cmd := fmt.Sprintf("%s popup --type ws-select --data '%s'", exe, escSh(itemsData))
	itemCount := len(strings.Split(itemsData, ","))
	h := fmt.Sprintf("%d", min(itemCount+4, 20))
	title := fmt.Sprintf(" Create workspace: %s ", wsBranch)
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "55", Height: h, Title: title,
		BorderStyle: "fg=green", BorderLines: "rounded",
	}, cmd)
	m.pendingPopup = &PendingPopup{Kind: PopupWsRepoSelect, WsName: wsName, WsBranch: wsBranch}
}

func (m *Model) runShortcutInPopup(cmd, desc, dir string) {
	log := "/tmp/tncli-shortcut-output.log"
	script := fmt.Sprintf("#!/bin/zsh\nLOG='%s'\ncd '%s'\n(%s) 2>&1 | tee \"$LOG\"\nless -R --mouse +G \"$LOG\"\nrm -f \"$LOG\"\n", log, dir, cmd)
	scriptPath := "/tmp/tncli-shortcut-run.sh"
	_ = os.WriteFile(scriptPath, []byte(script), 0o755)
	tmux.DisplayPopup("80%", "80%", scriptPath)
	m.SetMessage(fmt.Sprintf("running: %s", desc))
}

func (m *Model) doStopAll() {
	for svc := range m.RunningWindows {
		m.Stopping[svc] = true
	}
	m.SetMessage("stopping all services...")
	exe, _ := os.Executable()
	go func() {
		_ = exec.Command(exe, "stop").Run()
	}()
}

// ── Pipeline helpers ──

func (m *Model) startCreatePipeline(wsName, wsBranch string, selected []services.DirBranch) {
	// TODO: run pipeline in background, send events to TUI
	m.SetMessage(fmt.Sprintf("creating workspace %s (branch %s)...", wsName, wsBranch))
	m.CreatingWs[wsBranch] = true
	m.RebuildComboTree()
}

func (m *Model) startDeletePipeline(branch string) {
	m.SetMessage(fmt.Sprintf("deleting workspace %s...", branch))
	m.DeletingWs[branch] = true
	m.RebuildComboTree()
}

func (m *Model) addRepoToWorkspace(dirName, branch string) {
	dirPath := m.DirPath(dirName)
	dir := m.Config.Repos[dirName]
	if dir == nil {
		return
	}
	var copyFiles []string
	if dir.WT() != nil {
		copyFiles = dir.WT().Copy
	}
	wsFolder := filepath.Join(filepath.Dir(m.ConfigPath), "workspace--"+branch)
	wtPath, err := services.CreateWorktreeFromBase(dirPath, branch, m.Config.DefaultBranchFor(dirName), copyFiles, wsFolder)
	if err != nil {
		m.SetMessage(fmt.Sprintf("add failed: %v", err))
		return
	}
	wsKey := "ws-" + branch
	ip := services.AllocateIP(m.Session, wsKey)
	_ = services.WriteEnvFile(wtPath, ip)
	m.scanWorktrees()
	m.SetMessage(fmt.Sprintf("added %s to workspace %s", dirName, branch))
}

func (m *Model) deleteWorktree(wtKey string) {
	wt, ok := m.Worktrees[wtKey]
	if !ok {
		m.SetMessage("worktree not found")
		return
	}
	dirPath := m.DirPath(wt.ParentDir)
	_ = services.RemoveWorktree(dirPath, wt.Path, wt.Branch)
	delete(m.Worktrees, wtKey)
	m.RebuildComboTree()
	m.SetMessage(fmt.Sprintf("removed %s", wtKey))
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
