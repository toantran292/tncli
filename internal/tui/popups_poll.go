package tui

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tmux"
)

func (m *Model) pollPopupResult() {
	popup := m.pendingPopup
	if popup == nil {
		return
	}

	data, err := os.ReadFile(popupResultFile)
	if err != nil {
		return
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
				tmux.DisplayMessage(fmt.Sprintf(" checked out %s", result))
			} else {
				tmux.DisplayMessage(fmt.Sprintf(" checkout failed: %v", err))
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
			tmux.DisplayPopupStyled(tmux.PopupOptions{
				Width: "70%", Height: "50%",
				Title: " git pull origin " + branch + " ",
				BorderStyle: "fg=cyan", BorderLines: "rounded",
			}, fmt.Sprintf("(git -C '%s' pull origin %s) 2>&1 | less -R --mouse +G", popup.Path, branch))
		case "diff view":
			tmux.DisplayPopupStyled(tmux.PopupOptions{
				Width: "90%", Height: "90%",
				Title: " git diff ",
				BorderStyle: "fg=cyan", BorderLines: "rounded",
			}, fmt.Sprintf("cd '%s' && git diff --color=always | less -R --mouse", popup.Path))
		}

	case PopupGitPullAll:
		if result == "pull all repos" {
			m.gitPullAll()
		}

	case PopupShortcut:
		var idx int
		if _, err := fmt.Sscanf(result, "%d", &idx); err == nil && idx < len(m.shortcutItems) {
			shortcut := m.shortcutItems[idx]
			if dir := m.selectedWorkDir(); dir != "" {
				// Regenerate env before running shortcut
				item := m.CurrentItem()
				if item != nil {
					branch := m.Config.GlobalDefaultBranch()
					if !item.IsMain {
						branch = item.Branch
					}
					configDir := filepath.Dir(m.ConfigPath)
					services.RegenerateWorkspaceEnv(configDir, m.Config, branch)
				}
				dirName := ""
			if item != nil {
				dirName = item.Dir
			}
			m.runShortcutInPopup(m.Config.TransformInstallCmd(shortcut.Cmd, false), shortcut.Desc, dir, dirName)
			}
		}

	case PopupEnvSelect:
		configDir := filepath.Dir(m.ConfigPath)
		envName := result
		if envName == "local" {
			envName = ""
		}
		wsFolder := filepath.Join(configDir, "workspace--"+popup.Branch)
		state := services.LoadWorkspaceState(wsFolder)
		if state.ServiceEnvs == nil {
			state.ServiceEnvs = make(map[string]string)
		}
		if strings.Contains(popup.Dir, "/") {
			// Single service: "alias/svc" — also clear repo-level key
			repoAlias := popup.Dir[:strings.Index(popup.Dir, "/")]
			delete(state.ServiceEnvs, repoAlias)
			if envName == "" {
				delete(state.ServiceEnvs, popup.Dir)
			} else {
				state.ServiceEnvs[popup.Dir] = envName
			}
		} else {
			// Whole repo: set repo-level key + clear per-service keys
			alias := popup.Dir
			if envName == "" {
				delete(state.ServiceEnvs, alias)
			} else {
				state.ServiceEnvs[alias] = envName
			}
			for dirName, dir := range m.Config.Repos {
				a := dir.Alias
				if a == "" {
					a = dirName
				}
				if a == alias {
					for _, svc := range dir.ServiceOrder {
						delete(state.ServiceEnvs, alias+"/"+svc)
					}
					break
				}
			}
		}
		services.SaveWorkspaceState(wsFolder, &state)
		services.RegenerateWorkspaceEnv(configDir, m.Config, popup.Branch)
		if envName == "" {
			tmux.DisplayMessage(fmt.Sprintf(" %s → local", popup.Dir))
		} else {
			tmux.DisplayMessage(fmt.Sprintf(" %s → %s", popup.Dir, envName))
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
		var selected []services.DirBranch
		for _, line := range strings.Split(result, "\n") {
			line = strings.TrimSpace(line)
			if dirName, _, ok := strings.Cut(line, ":"); ok && line != "" {
				selected = append(selected, services.DirBranch{Name: strings.TrimSpace(dirName)})
			}
		}
		if len(selected) > 0 {
			m.wsName = popup.WsName
			m.startCreatePipeline(popup.WsName, popup.WsBranch, selected)
		}

	case PopupNameInput:
		if result != "" {
			if popup.Context == "workspace" {
				m.buildWsSelect(result)
			} else if strings.HasPrefix(popup.Context, "branch:") {
				dir := strings.TrimPrefix(popup.Context, "branch:")
				if err := services.CheckoutNewBranch(m.DirPath(dir), result); err == nil {
					m.scanWorktrees()
					tmux.DisplayMessage(fmt.Sprintf(" created branch %s in %s", result, dir))
				} else {
					tmux.DisplayMessage(fmt.Sprintf(" create branch failed: %v", err))
				}
			}
		}

	case PopupDBMenu:
		m.doDBAction(result)

	case PopupConfirm:
		if strings.EqualFold(strings.TrimSpace(result), "y") {
			if strings.HasPrefix(popup.Action, "delete_ws:") {
				m.startDeletePipeline(strings.TrimPrefix(popup.Action, "delete_ws:"))
			} else if popup.Action == "stop_all" {
				m.doStopAll()
			}
		} else {
			tmux.DisplayMessage(" cancelled")
		}
	}
}

func (m *Model) gitPullAll() {
	var script strings.Builder
	script.WriteString("#!/bin/zsh\n")
	for i, dirName := range m.DirNames {
		path := m.DirPath(dirName)
		branch := m.Config.DefaultBranchFor(dirName)
		fmt.Fprintf(&script,
			"( cd '%s' && git pull origin %s > /tmp/tncli-pull-%d.log 2>&1 && echo '\\033[32m✓ %s\\033[0m' || echo '\\033[31m✗ %s\\033[0m'; cat /tmp/tncli-pull-%d.log; rm -f /tmp/tncli-pull-%d.log; echo ) &\n",
			path, branch, i, dirName, dirName, i, i)
	}
	script.WriteString("wait\necho '\\033[32m[Done]\\033[0m'\n")
	scriptPath := "/tmp/tncli-pull-all.sh"
	_ = os.WriteFile(scriptPath, []byte(script.String()), 0o755)
	log := "/tmp/tncli-pull-all.log"
	run := fmt.Sprintf("%s 2>&1 | tee '%s'; less -R --mouse +G '%s'; rm -f '%s' '%s'", scriptPath, log, log, log, scriptPath)
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "80%", Height: "80%",
		Title: " git pull all repos ",
		BorderStyle: "fg=cyan", BorderLines: "rounded",
	}, run)
}

func (m *Model) runShortcutInPopup(cmd, desc, dir, dirName string) {
	log := "/tmp/tncli-shortcut-output.log"

	// Source env via dir's pre_start if available
	preStart := ""
	if d := m.Config.Repos[dirName]; d != nil && d.PreStart != "" {
		preStart = d.PreStart + "\n"
	}

	script := fmt.Sprintf("#!/bin/zsh\nLOG='%s'\ncd '%s'\n%s(%s) 2>&1 | tee \"$LOG\"\nless -R --mouse +G \"$LOG\"\nrm -f \"$LOG\"\n", log, dir, preStart, cmd)
	_ = os.WriteFile("/tmp/tncli-shortcut-run.sh", []byte(script), 0o755)
	tmux.DisplayPopupStyled(tmux.PopupOptions{
		Width: "80%", Height: "80%",
		Title:       " " + desc + " ",
		BorderStyle: "fg=cyan", BorderLines: "rounded",
	}, "/tmp/tncli-shortcut-run.sh")
}
