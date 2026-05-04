package tui

import (
	"fmt"
	"strings"

	"github.com/charmbracelet/lipgloss"
)

func (m *Model) View() string {
	if m.Width == 0 {
		return "Loading..."
	}

	singleCombo := len(m.Combos) <= 1
	var lines []string
	for i, item := range m.ComboItems {
		if singleCombo && item.Kind == KindCombo {
			continue
		}
		lines = append(lines, m.renderItem(item, i == m.Cursor))
	}

	for len(lines) < m.Height-2 {
		lines = append(lines, "")
	}

	msgLine := ""
	if m.Message != "" {
		msgLine = lipgloss.NewStyle().Foreground(lipgloss.Color("11")).Bold(true).Render(" " + m.Message)
	}
	for _, p := range m.ActivePipelines {
		status := fmt.Sprintf(" [%d/%d] %s: %s", p.CurrentStage+1, p.TotalStages, p.Operation, p.StageName)
		if p.Failed != nil {
			status += fmt.Sprintf(" FAILED: %s", p.Failed.Error)
		}
		msgLine = lipgloss.NewStyle().Foreground(lipgloss.Color("33")).Render(status)
	}

	content := strings.Join(lines[:m.Height-2], "\n")
	if msgLine != "" {
		content += "\n" + msgLine
	}
	return content
}

func (m *Model) renderItem(item ComboItem, isCur bool) string {
	w := m.Width
	if w < 20 {
		w = 40
	}

	switch item.Kind {
	case KindCombo:
		return m.renderCombo(item, isCur, w)
	case KindInstance:
		return m.renderInstance(item, isCur, w)
	case KindInstanceDir:
		return m.renderDir(item, isCur, w)
	case KindInstanceService:
		return m.renderService(item, isCur)
	}
	return ""
}

func (m *Model) renderCombo(item ComboItem, isCur bool, w int) string {
	entries := m.Config.AllWorkspaces()[item.Name]
	running, total := 0, len(entries)
	for _, entry := range entries {
		d, s, ok := m.Config.FindServiceEntryQuiet(entry)
		if !ok {
			continue
		}
		alias := d
		if dir, ok := m.Config.Repos[d]; ok && dir.Alias != "" {
			alias = dir.Alias
		}
		if m.IsRunning(fmt.Sprintf("%s~%s", alias, s)) {
			running++
		}
	}
	icon, iconColor := statusIcon(running, total)
	counter := fmt.Sprintf("%d/%d", running, total)

	style := lipgloss.NewStyle().Bold(true)
	if isCur {
		style = style.Background(lipgloss.Color("6")).Foreground(lipgloss.Color("0"))
	} else if running > 0 {
		style = style.Foreground(lipgloss.Color("15"))
	} else {
		style = style.Foreground(lipgloss.Color("8"))
	}
	iStyle := lipgloss.NewStyle().Foreground(lipgloss.Color(iconColor))
	if isCur {
		iStyle = style
	}
	left := iStyle.Render(fmt.Sprintf(" %s ", icon)) + style.Render(item.Name)
	return padRightTruncate(left, lipgloss.NewStyle().Foreground(lipgloss.Color(iconColor)).Render(counter), w)
}

func (m *Model) renderInstance(item ComboItem, isCur bool, w int) string {
	if m.CreatingWs[item.Branch] || m.DeletingWs[item.Branch] {
		color := "11"
		if m.DeletingWs[item.Branch] {
			color = "9"
		}
		style := lipgloss.NewStyle().Foreground(lipgloss.Color(color))
		if isCur {
			style = style.Background(lipgloss.Color("6")).Foreground(lipgloss.Color("0")).Bold(true)
		}
		progress := "..."
		for _, p := range m.ActivePipelines {
			if p.Branch == item.Branch && p.TotalStages > 0 {
				progress = fmt.Sprintf("%d%%", (p.CurrentStage*100)/p.TotalStages)
			}
		}
		left := style.Render("~ " + item.Branch)
		return padRightTruncate(left, lipgloss.NewStyle().Foreground(lipgloss.Color(color)).Render(progress), w)
	}

	running, total := m.instanceRunningCount(item.Branch, item.IsMain)
	icon, iconColor := statusIcon(running, total)
	counter := fmt.Sprintf("%d/%d", running, total)

	style := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("5"))
	if isCur {
		style = style.Background(lipgloss.Color("6")).Foreground(lipgloss.Color("0"))
	}
	iStyle := lipgloss.NewStyle().Foreground(lipgloss.Color(iconColor))
	if isCur {
		iStyle = style
	}
	left := iStyle.Render(fmt.Sprintf("%s ", icon)) + style.Render(item.Branch)
	return padRightTruncate(left, lipgloss.NewStyle().Foreground(lipgloss.Color(iconColor)).Render(counter), w)
}

func (m *Model) renderDir(item ComboItem, isCur bool, w int) string {
	alias := item.Dir
	if dir, ok := m.Config.Repos[item.Dir]; ok && dir.Alias != "" {
		alias = dir.Alias
	}
	running, total := m.dirRunningCount(item.Dir, item.Branch, item.IsMain)
	icon, iconColor := statusIcon(running, total)
	counter := fmt.Sprintf("%d/%d", running, total)

	style := lipgloss.NewStyle().Bold(true)
	if isCur {
		style = style.Background(lipgloss.Color("6")).Foreground(lipgloss.Color("0"))
	}
	iStyle := lipgloss.NewStyle().Foreground(lipgloss.Color(iconColor))
	if isCur {
		iStyle = style
	}
	left := lipgloss.NewStyle().Foreground(lipgloss.Color("8")).Render(" ├ ") + iStyle.Render(icon+" ") + style.Render(alias)
	return padRightTruncate(left, lipgloss.NewStyle().Foreground(lipgloss.Color(iconColor)).Render(counter), w)
}

func (m *Model) renderService(item ComboItem, isCur bool) string {
	running := m.IsRunning(item.TmuxName)
	stopping, starting := m.Stopping[item.TmuxName], m.Starting[item.TmuxName]

	icon, color := "○", "8"
	if stopping || starting {
		icon, color = "~", "11"
	} else if running {
		icon, color = "●", "2"
	}

	style := lipgloss.NewStyle()
	if isCur {
		switch {
		case stopping || starting:
			style = style.Background(lipgloss.Color("11")).Foreground(lipgloss.Color("0")).Bold(true)
		case running:
			style = style.Background(lipgloss.Color("2")).Foreground(lipgloss.Color("0")).Bold(true)
		default:
			style = style.Background(lipgloss.Color("6")).Foreground(lipgloss.Color("0")).Bold(true)
		}
	} else if running {
		style = style.Foreground(lipgloss.Color("2")).Bold(true)
	} else if stopping || starting {
		style = style.Foreground(lipgloss.Color("11"))
	} else {
		style = style.Foreground(lipgloss.Color("8"))
	}
	iStyle := lipgloss.NewStyle().Foreground(lipgloss.Color(color))
	if isCur {
		iStyle = style
	}
	tree := lipgloss.NewStyle().Foreground(lipgloss.Color("8")).Render(" │ ├ ")
	return tree + iStyle.Render(icon+" ") + style.Render(item.Svc)
}

func (m *Model) instanceRunningCount(branch string, isMain bool) (running, total int) {
	if isMain {
		comboName := m.FindParentCombo(m.Cursor)
		entries := m.Config.AllWorkspaces()[comboName]
		total = len(entries)
		for _, entry := range entries {
			d, s, ok := m.Config.FindServiceEntryQuiet(entry)
			if !ok {
				continue
			}
			alias := d
			if dir, ok := m.Config.Repos[d]; ok && dir.Alias != "" {
				alias = dir.Alias
			}
			if m.IsRunning(fmt.Sprintf("%s~%s", alias, s)) {
				running++
			}
		}
	} else {
		for _, wt := range m.Worktrees {
			if WorkspaceBranch(wt) != branch {
				continue
			}
			alias := wt.ParentDir
			if dir, ok := m.Config.Repos[wt.ParentDir]; ok && dir.Alias != "" {
				alias = dir.Alias
			}
			branchSafe := strings.ReplaceAll(branch, "/", "-")
			for _, s := range m.Config.AllServicesFor(wt.ParentDir) {
				total++
				if m.IsRunning(fmt.Sprintf("%s~%s~%s", alias, s, branchSafe)) {
					running++
				}
			}
		}
	}
	return
}

func (m *Model) dirRunningCount(dirName, branch string, isMain bool) (running, total int) {
	alias := dirName
	if dir, ok := m.Config.Repos[dirName]; ok && dir.Alias != "" {
		alias = dir.Alias
	}
	for _, s := range m.Config.AllServicesFor(dirName) {
		total++
		tmuxName := fmt.Sprintf("%s~%s", alias, s)
		if !isMain {
			tmuxName = fmt.Sprintf("%s~%s~%s", alias, s, strings.ReplaceAll(branch, "/", "-"))
		}
		if m.IsRunning(tmuxName) {
			running++
		}
	}
	return
}

func statusIcon(running, total int) (icon, color string) {
	if running == total && total > 0 {
		return "●", "2"
	} else if running > 0 {
		return "◐", "3"
	}
	return "○", "8"
}

func (m *Model) visualToRealIdx(visualIdx int) int {
	singleCombo := len(m.Combos) <= 1
	row := 0
	for i, item := range m.ComboItems {
		if singleCombo && item.Kind == KindCombo {
			continue
		}
		if row == visualIdx {
			return i
		}
		row++
	}
	return -1
}

func padRight(left, right string, width int) string {
	pad := width - lipgloss.Width(left) - lipgloss.Width(right)
	if pad < 1 {
		pad = 1
	}
	return left + strings.Repeat(" ", pad) + right
}

func truncateVisible(s string, maxLen int) string {
	if lipgloss.Width(s) <= maxLen {
		return s
	}
	runes := []rune(s)
	for len(runes) > 0 {
		if lipgloss.Width(string(runes)) <= maxLen-1 {
			return string(runes) + "…"
		}
		runes = runes[:len(runes)-1]
	}
	return "…"
}

func padRightTruncate(left, right string, width int) string {
	rightLen := lipgloss.Width(right)
	maxLeft := width - rightLen - 1
	if maxLeft < 4 {
		maxLeft = 4
	}
	if lipgloss.Width(left) > maxLeft {
		left = truncateVisible(left, maxLeft)
	}
	pad := width - lipgloss.Width(left) - rightLen
	if pad < 1 {
		pad = 1
	}
	return left + strings.Repeat(" ", pad) + right
}
