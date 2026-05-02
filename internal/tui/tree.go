package tui

import (
	"fmt"
	"sort"
	"strings"
)

// RebuildComboTree builds flattened workspace tree.
func (m *Model) RebuildComboTree() {
	m.ComboItems = nil

	// Collect active workspace instances grouped by branch
	type branchDirs struct {
		branch string
		dirs   [][2]string // (dir_name, wt_key)
	}
	instanceMap := make(map[string][][2]string)
	for wtKey, wt := range m.Worktrees {
		branch := WorkspaceBranch(wt)
		if branch != "" {
			instanceMap[branch] = append(instanceMap[branch], [2]string{wt.ParentDir, wtKey})
		}
	}

	// Sort branches
	var branches []string
	for b := range instanceMap {
		branches = append(branches, b)
	}
	sort.Strings(branches)

	allWs := m.Config.AllWorkspaces()

	// Build combo ordering for dir sort
	var comboOrder []string
	for _, entries := range allWs {
		for _, entry := range entries {
			d, _, ok := m.Config.FindServiceEntryQuiet(entry)
			if ok {
				found := false
				for _, e := range comboOrder {
					if e == d {
						found = true
						break
					}
				}
				if !found {
					comboOrder = append(comboOrder, d)
				}
			}
		}
	}

	// Sort dirs within each instance
	for _, dirs := range instanceMap {
		sort.Slice(dirs, func(i, j int) bool {
			ia, ib := len(comboOrder), len(comboOrder)
			for idx, d := range comboOrder {
				if d == dirs[i][0] {
					ia = idx
				}
				if d == dirs[j][0] {
					ib = idx
				}
			}
			return ia < ib
		})
	}

	matched := make(map[string]bool)

	for _, name := range m.Combos {
		m.ComboItems = append(m.ComboItems, ComboItem{Kind: KindCombo, Name: name})

		// Get dirs for this combo
		var comboDirs []string
		if entries, ok := allWs[name]; ok {
			for _, entry := range entries {
				d, _, ok := m.Config.FindServiceEntryQuiet(entry)
				if ok {
					found := false
					for _, e := range comboDirs {
						if e == d {
							found = true
							break
						}
					}
					if !found {
						comboDirs = append(comboDirs, d)
					}
				}
			}
		}

		// Main instance
		defaultBranch := m.Config.GlobalDefaultBranch()
		m.ComboItems = append(m.ComboItems, ComboItem{Kind: KindInstance, Branch: defaultBranch, IsMain: true})
		mainInstKey := fmt.Sprintf("ws-inst-main-%s", name)
		if !m.ComboCollapsed[mainInstKey] {
			mainDirs := make([][2]string, len(comboDirs))
			for i, d := range comboDirs {
				mainDirs[i] = [2]string{d, ""}
			}
			m.buildInstanceDirs(defaultBranch, true, name, mainDirs)
		}

		// Matched non-main instances
		for _, branch := range branches {
			if matched[branch] {
				continue
			}
			dirs := instanceMap[branch]
			instDirNames := make(map[string]bool)
			for _, d := range dirs {
				instDirNames[d[0]] = true
			}
			matches := len(comboDirs) > 0
			for _, d := range comboDirs {
				if !instDirNames[d] {
					matches = false
					break
				}
			}
			if !matches {
				continue
			}

			matched[branch] = true
			m.ComboItems = append(m.ComboItems, ComboItem{Kind: KindInstance, Branch: branch, IsMain: false})
			instKey := fmt.Sprintf("ws-inst-%s", branch)
			if !m.ComboCollapsed[instKey] {
				m.buildInstanceDirs(branch, false, "", dirs)
			}
		}

		// Creating workspaces
		for branch := range m.CreatingWs {
			if !matched[branch] {
				if _, ok := instanceMap[branch]; !ok {
					m.ComboItems = append(m.ComboItems, ComboItem{Kind: KindInstance, Branch: branch, IsMain: false})
					matched[branch] = true
				}
			}
		}
	}

	// Orphan instances
	for _, branch := range branches {
		if matched[branch] {
			continue
		}
		dirs := instanceMap[branch]
		m.ComboItems = append(m.ComboItems, ComboItem{Kind: KindInstance, Branch: branch, IsMain: false})
		instKey := fmt.Sprintf("ws-inst-%s", branch)
		if !m.ComboCollapsed[instKey] {
			m.buildInstanceDirs(branch, false, "", dirs)
		}
	}

	m.ClampCursor()
}

func (m *Model) buildInstanceDirs(branch string, isMain bool, comboName string, dirs [][2]string) {
	for _, d := range dirs {
		dirName, wtKey := d[0], d[1]
		m.ComboItems = append(m.ComboItems, ComboItem{
			Kind: KindInstanceDir, Branch: branch, Dir: dirName, WtKey: wtKey, IsMain: isMain,
		})

		var dirKey string
		if isMain {
			dirKey = fmt.Sprintf("ws-dir-main-%s-%s", comboName, dirName)
		} else {
			dirKey = fmt.Sprintf("ws-dir-%s-%s", branch, dirName)
		}

		allSvcs := m.Config.AllServicesFor(dirName)
		if len(allSvcs) > 1 && !m.ComboCollapsed[dirKey] {
			for _, svcName := range allSvcs {
				var tmuxName string
				if isMain {
					alias := dirName
					if dir, ok := m.Config.Repos[dirName]; ok && dir.Alias != "" {
						alias = dir.Alias
					}
					tmuxName = fmt.Sprintf("%s~%s", alias, svcName)
				} else {
					tmuxName = m.WtTmuxName(dirName, svcName, branch)
				}
				m.ComboItems = append(m.ComboItems, ComboItem{
					Kind: KindInstanceService, Branch: branch, Dir: dirName, WtKey: wtKey,
					Svc: svcName, TmuxName: tmuxName, IsMain: isMain,
				})
			}
		}
	}

	// Worktree-level global services
	for _, gs := range m.Config.WorktreeLevelServices() {
		m.ComboItems = append(m.ComboItems, ComboItem{
			Kind: KindInstanceDir, Branch: branch, Dir: "_global:" + gs.Name, IsMain: isMain,
		})
	}
}

func (m *Model) ToggleCollapse() {
	item := m.CurrentItem()
	if item == nil {
		return
	}
	switch item.Kind {
	case KindInstance:
		var key string
		if item.IsMain {
			comboName := m.FindParentCombo(m.Cursor)
			key = fmt.Sprintf("ws-inst-main-%s", comboName)
		} else {
			key = fmt.Sprintf("ws-inst-%s", item.Branch)
		}
		m.ComboCollapsed[key] = !m.ComboCollapsed[key]
		m.RebuildComboTree()
	case KindInstanceDir:
		if strings.HasPrefix(item.Dir, "_global:") {
			return
		}
		svcCount := len(m.Config.AllServicesFor(item.Dir))
		if svcCount <= 1 {
			return
		}
		var key string
		if item.IsMain {
			comboName := m.FindParentCombo(m.Cursor)
			key = fmt.Sprintf("ws-dir-main-%s-%s", comboName, item.Dir)
		} else {
			key = fmt.Sprintf("ws-dir-%s-%s", item.Branch, item.Dir)
		}
		m.ComboCollapsed[key] = !m.ComboCollapsed[key]
		m.RebuildComboTree()
	}
	m.saveCollapseState()
}

func (m *Model) FindParentCombo(idx int) string {
	for i := idx; i >= 0; i-- {
		if i < len(m.ComboItems) && m.ComboItems[i].Kind == KindCombo {
			return m.ComboItems[i].Name
		}
	}
	return ""
}
