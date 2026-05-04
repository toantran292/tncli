package tui

import (
	"fmt"
	"os"
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
			tmux.DisplayPopup("70%", "50%", fmt.Sprintf("(git -C '%s' pull origin %s) 2>&1 | less -R --mouse +G", popup.Path, branch))
		case "diff view":
			tmux.DisplayPopup("90%", "90%", fmt.Sprintf("cd '%s' && git diff --color=always | less -R --mouse", popup.Path))
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
		var selected []services.DirBranch
		for _, line := range strings.Split(result, "\n") {
			line = strings.TrimSpace(line)
			if a, b, ok := strings.Cut(line, ":"); ok && line != "" {
				selected = append(selected, services.DirBranch{Name: strings.TrimSpace(a), Branch: strings.TrimSpace(b)})
			}
		}
		if len(selected) > 0 {
			m.wsName = popup.WsName
			m.startCreatePipeline(popup.WsName, popup.WsBranch, selected)
		}

	case PopupWsBranchPick:
		if result != "" {
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
					tmux.DisplayMessage(fmt.Sprintf(" created branch %s in %s", result, dir))
				} else {
					tmux.DisplayMessage(fmt.Sprintf(" create branch failed: %v", err))
				}
			}
		}

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
	tmux.DisplayPopup("80%", "80%", run)
}

func (m *Model) runShortcutInPopup(cmd, desc, dir string) {
	log := "/tmp/tncli-shortcut-output.log"
	script := fmt.Sprintf("#!/bin/zsh\nLOG='%s'\ncd '%s'\n(%s) 2>&1 | tee \"$LOG\"\nless -R --mouse +G \"$LOG\"\nrm -f \"$LOG\"\n", log, dir, cmd)
	_ = os.WriteFile("/tmp/tncli-shortcut-run.sh", []byte(script), 0o755)
	tmux.DisplayPopup("80%", "80%", "/tmp/tncli-shortcut-run.sh")
	tmux.DisplayMessage(fmt.Sprintf(" running: %s", desc))
}
