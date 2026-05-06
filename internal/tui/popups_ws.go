package tui

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/pipeline"
	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tmux"
)

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

	var itemsStr []string
	for _, dirName := range uniqueDirs {
		alias := dirName
		if dir, ok := m.Config.Repos[dirName]; ok && dir.Alias != "" {
			alias = dir.Alias
		}
		base := m.Config.DefaultBranchFor(dirName)
		path := m.DirPath(dirName)
		itemsStr = append(itemsStr, fmt.Sprintf("%s|%s|%s|%s|%s", alias, base, wsBranch, path, dirName))
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
	if !m.requireTool("fzf", "brew install fzf") {
		return
	}
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
		tmux.DisplayMessage(" all repos already in workspace")
		return
	}

	_ = os.Remove(popupResultFile)
	cmd := fmt.Sprintf("printf '%s' | fzf --prompt='Add repo> ' --with-nth=2 --delimiter='\t' | cut -f1 > %s",
		escSh(strings.Join(available, "\n")), popupResultFile)
	tmux.DisplayPopup("50%", "40%", cmd)
	m.pendingPopup = &PendingPopup{Kind: PopupWsAdd, Branch: branch}
}

func (m *Model) buildWsRemoveList(branch string) {
	if !m.requireTool("fzf", "brew install fzf") {
		return
	}
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
		tmux.DisplayMessage(" no repos to remove")
		return
	}

	_ = os.Remove(popupResultFile)
	var items []string
	for _, r := range repos {
		items = append(items, fmt.Sprintf("%s\t%s", r.key, r.alias))
	}
	cmd := fmt.Sprintf("printf '%s' | fzf --prompt='Remove repo> ' --with-nth=2 --delimiter='\t' | cut -f1 > %s",
		escSh(strings.Join(items, "\n")), popupResultFile)
	tmux.DisplayPopup("50%", "40%", cmd)
	m.pendingPopup = &PendingPopup{Kind: PopupWsRemove}
}

func (m *Model) startCreatePipeline(wsName, wsBranch string, selected []services.DirBranch) {
	tmux.DisplayMessage(fmt.Sprintf(" creating workspace %s (branch %s)...", wsName, wsBranch))
	m.CreatingWs[wsBranch] = true
	m.RebuildComboTree()

	configPath := m.ConfigPath
	cfg := m.Config
	go func() {
		var ctx *pipeline.CreateContext
		var err error
		if len(selected) > 0 {
			ctx, err = pipeline.FromConfigWithSelection(cfg, configPath, wsName, wsBranch, selected)
		} else {
			ctx, err = pipeline.FromConfig(cfg, configPath, wsName, wsBranch, nil)
		}
		if err != nil {
			tmux.DisplayMessage(fmt.Sprintf(" create failed: %v ", err))
			delete(m.CreatingWs, wsBranch)
			m.RebuildComboTree()
			return
		}

		ch := make(chan pipeline.Event, 16)
		go pipeline.RunCreatePipeline(ctx, ch)

		for evt := range ch {
			switch evt.Type {
			case pipeline.EventStageStarted:
				tmux.DisplayMessage(fmt.Sprintf(" [%d/%d] %s ", evt.Index+1, evt.Total, evt.Name))
			case pipeline.EventPipelineCompleted:
				tmux.DisplayMessage(fmt.Sprintf(" workspace %s ready ", wsBranch))
				delete(m.CreatingWs, wsBranch)
				m.scanWorktrees()
				m.RebuildComboTree()
			case pipeline.EventPipelineFailed:
				tmux.DisplayMessage(fmt.Sprintf(" create failed at stage %d: %s ", evt.Index+1, evt.Error))
				delete(m.CreatingWs, wsBranch)
				m.RebuildComboTree()
			}
		}
	}()
}

func (m *Model) startDeletePipeline(branch string) {
	tmux.DisplayMessage(fmt.Sprintf(" deleting workspace %s...", branch))
	m.DeletingWs[branch] = true
	m.RebuildComboTree()

	cfg := m.Config
	cfgPath := m.ConfigPath
	go func() {
		// Reuse CLI delete logic
		configDir := filepath.Dir(cfgPath)
		branchSafe := services.BranchSafe(branch)

		var cleanupItems []pipeline.CleanupItem
		var dbsToDrop []pipeline.DBDropItem

		for dirName, dir := range cfg.Repos {
			wtPath := filepath.Join(configDir, "workspace--"+branch, dirName)
			if _, err := os.Stat(wtPath); os.IsNotExist(err) {
				continue
			}
			dirPath := filepath.Join(configDir, "workspace--"+cfg.GlobalDefaultBranch(), dirName)
			if _, err := os.Stat(dirPath); os.IsNotExist(err) {
				dirPath = filepath.Join(configDir, dirName)
			}
			cleanupItems = append(cleanupItems, pipeline.CleanupItem{
				DirPath: dirPath, WtPath: wtPath, WtBranch: branch, PreDelete: dir.PreDelete,
			})
			for _, dbTpl := range dir.Databases {
				dbName := strings.ReplaceAll(dbTpl, "{{branch_safe}}", branchSafe)
				dbName = strings.ReplaceAll(dbName, "{{branch}}", branch)
				pgPort := uint16(services.SharedPort("postgres"))
				if pgPort == 0 {
					pgPort = 5432
				}
				dbsToDrop = append(dbsToDrop, pipeline.DBDropItem{
					Host: "postgres", Port: pgPort,
					DBName: cfg.Session + "_" + dbName, User: "postgres", Password: "postgres",
				})
			}
		}

		ctx := &pipeline.DeleteContext{
			Branch: branch, Config: cfg, ConfigDir: configDir,
			CleanupItems: cleanupItems, DBsToDrop: dbsToDrop,
		}

		ch := make(chan pipeline.Event, 16)
		go pipeline.RunDeletePipeline(ctx, ch)

		for evt := range ch {
			switch evt.Type {
			case pipeline.EventStageStarted:
				tmux.DisplayMessage(fmt.Sprintf(" [%d/%d] %s ", evt.Index+1, evt.Total, evt.Name))
			case pipeline.EventPipelineCompleted:
				tmux.DisplayMessage(fmt.Sprintf(" workspace %s deleted ", branch))
				delete(m.DeletingWs, branch)
				m.scanWorktrees()
				m.RebuildComboTree()
			case pipeline.EventPipelineFailed:
				tmux.DisplayMessage(fmt.Sprintf(" delete failed: %s ", evt.Error))
				delete(m.DeletingWs, branch)
				m.RebuildComboTree()
			}
		}
	}()
}

func (m *Model) addRepoToWorkspace(dirName, branch string) {
	dirPath := m.DirPath(dirName)
	dir := m.Config.Repos[dirName]
	if dir == nil {
		return
	}
	var copyFiles []string
	if dir.HasWorktreeConfig() {
		copyFiles = dir.Copy
	}
	wsFolder := filepath.Join(filepath.Dir(m.ConfigPath), "workspace--"+branch)
	wtPath, err := services.CreateWorktreeFromBase(dirPath, branch, m.Config.DefaultBranchFor(dirName), copyFiles, wsFolder)
	if err != nil {
		tmux.DisplayMessage(fmt.Sprintf(" add failed: %v", err))
		return
	}
	_ = services.WriteEnvFile(wtPath)
	m.scanWorktrees()
	tmux.DisplayMessage(fmt.Sprintf(" added %s to workspace %s", dirName, branch))
}

func (m *Model) deleteWorktree(wtKey string) {
	wt, ok := m.Worktrees[wtKey]
	if !ok {
		tmux.DisplayMessage(" worktree not found")
		return
	}
	_ = services.RemoveWorktree(m.DirPath(wt.ParentDir), wt.Path, wt.Branch)
	delete(m.Worktrees, wtKey)
	m.RebuildComboTree()
	tmux.DisplayMessage(fmt.Sprintf(" removed %s", wtKey))
}
