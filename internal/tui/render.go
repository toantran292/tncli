package tui

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/charmbracelet/lipgloss"
	"github.com/toantran292/tncli/internal/services"
)

func (m *Model) View() string {
	if m.Width == 0 {
		return "Loading..."
	}

	w := m.Width
	if w < 20 {
		w = 40
	}
	innerW := w - 4 // │ + space + content + space + │

	singleCombo := len(m.Combos) <= 1

	// ── Build all item lines + find cursor visual index ──
	var allLines []string
	cursorVisual := 0
	for i, item := range m.ComboItems {
		if singleCombo && item.Kind == KindCombo {
			continue
		}
		if i == m.Cursor {
			cursorVisual = len(allLines)
		}
		var line string
		if !singleCombo && item.Kind == KindCombo {
			line = m.renderComboMonitor(item, i == m.Cursor, innerW)
		} else {
			line = m.renderItem(item, i == m.Cursor, innerW)
		}
		allLines = append(allLines, line)
	}

	// ── Scroll viewport ──
	chromeLines := 4 // header + stats + separator + footer
	maxVisible := m.Height - 2 - chromeLines
	if maxVisible < 1 {
		maxVisible = 1
	}
	if cursorVisual < m.ScrollOffset {
		m.ScrollOffset = cursorVisual
	}
	if cursorVisual >= m.ScrollOffset+maxVisible {
		m.ScrollOffset = cursorVisual - maxVisible + 1
	}
	if len(allLines) > maxVisible && m.ScrollOffset > len(allLines)-maxVisible {
		m.ScrollOffset = len(allLines) - maxVisible
	}
	if m.ScrollOffset < 0 {
		m.ScrollOffset = 0
	}
	end := m.ScrollOffset + maxVisible
	if end > len(allLines) {
		end = len(allLines)
	}
	visibleLines := allLines[m.ScrollOffset:end]

	// ── Header: ┌─  services ─── 6/14 ─┐ ──
	totalRunning, totalCount := m.totalRunningCount()
	countStr := fmt.Sprintf("%d/%d", totalRunning, totalCount)
	title := nfServer + " services"
	titleW := lipgloss.Width(title)
	// ┌ + ─ + space + TITLE + space + ─×fill + space + COUNT + space + ─ + ┐ = w
	fillW := w - titleW - len(countStr) - 8
	if fillW < 1 {
		fillW = 1
	}
	header := dimStyle.Render("┌─ ") + dimStyle.Render(title) + dimStyle.Render(" "+strings.Repeat("─", fillW)+" "+countStr+" ─┐")
	header = padLine(header, w)

	// ── Stats bar ──
	statsLine := boxLine(m.renderStatsBar(innerW), innerW, w)

	// ── Separator ──
	sep := padLine(dimStyle.Render("├"+strings.Repeat("─", w-2)+"┤"), w)

	// ── Framed items ──
	var framedItems []string
	for _, line := range visibleLines {
		framedItems = append(framedItems, boxLine(line, innerW, w))
	}
	for len(framedItems) < maxVisible {
		framedItems = append(framedItems, boxLine("", innerW, w))
	}

	// ── Footer ──
	footer := padLine(dimStyle.Render("└"+strings.Repeat("─", w-2)+"┘"), w)

	// ── Assemble ──
	var lines []string
	lines = append(lines, header, statsLine, sep)
	lines = append(lines, framedItems...)
	lines = append(lines, footer)

	// ── Message line (outside frame) ──
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

	content := strings.Join(lines, "\n")
	if msgLine != "" {
		content += "\n" + msgLine
	}
	return content
}

// boxLine wraps content in │ borders, padded/truncated to exact totalW.
func boxLine(content string, innerW, totalW int) string {
	visW := lipgloss.Width(content)
	if visW > innerW {
		content = truncateVisible(content, innerW)
		visW = lipgloss.Width(content)
	}
	pad := innerW - visW
	if pad < 0 {
		pad = 0
	}
	line := dimStyle.Render("│") + " " + content + strings.Repeat(" ", pad) + " " + dimStyle.Render("│")
	return padLine(line, totalW)
}

func (m *Model) renderItem(item ComboItem, isCur bool, w int) string {
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
		if item.Dir == "_ws" {
			return m.renderWsService(item, isCur, w)
		}
		return m.renderService(item, isCur, w)
	}
	return ""
}

var (
	dimStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("8"))
	selStyle = lipgloss.NewStyle().Background(lipgloss.Color("6")).Foreground(lipgloss.Color("0")).Bold(true)
)

// Nerd Font icons (JetBrains Mono Nerd Font)
const (
	nfChevronRight = "\uf054" //
	nfChevronDown  = "\uf078" //
	nfBranch       = "\ue0a0" //
	nfFolder       = "\uf07b" //
	nfCircle       = "\uf111" //
	nfCircleO      = "\uf10c" //
	nfAdjust       = "\uf042" //  (half-filled)
	nfServer       = "\uf233" //
)

func (m *Model) renderCombo(item ComboItem, isCur bool, w int) string {
	return m.renderComboMonitor(item, isCur, w)
}

func (m *Model) renderComboMonitor(item ComboItem, isCur bool, w int) string {
	running, total := m.comboRunningCount(item.Name)
	_, iconColor := statusIcon(running, total)

	comboKey := "ws-combo-" + item.Name
	arrow := nfChevronDown
	if m.ComboCollapsed[comboKey] {
		arrow = nfChevronRight
	}

	style := lipgloss.NewStyle().Bold(true)
	if isCur {
		style = selStyle
	} else if running > 0 {
		style = style.Foreground(lipgloss.Color("2"))
	} else {
		style = dimStyle.Bold(true)
	}
	arrowStyle := lipgloss.NewStyle().Foreground(lipgloss.Color(iconColor))
	if isCur {
		arrowStyle = style
	}

	left := arrowStyle.Render(arrow) + " " + style.Render(item.Name)
	right := compactCount(running, total, iconColor)
	return padRightTruncate(left, right, w)
}

func (m *Model) renderInstance(item ComboItem, isCur bool, w int) string {
	if m.CreatingWs[item.Branch] || m.DeletingWs[item.Branch] {
		color := "11"
		if m.DeletingWs[item.Branch] {
			color = "9"
		}
		style := lipgloss.NewStyle().Foreground(lipgloss.Color(color))
		if isCur {
			style = selStyle
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
	_, iconColor := statusIcon(running, total)

	var totalRSS float64
	for tmuxName, stat := range m.ProcStats {
		if stat.RSS > 0 && m.isServiceInInstance(tmuxName, item.Branch, item.IsMain) {
			totalRSS += stat.RSS
		}
	}

	style := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("5"))
	if isCur {
		style = selStyle
	}
	iStyle := lipgloss.NewStyle().Foreground(lipgloss.Color(iconColor))
	if isCur {
		iStyle = style
	}
	left := iStyle.Render(nfBranch+" ") + style.Render(item.Branch)

	right := ""
	if running > 0 || isCur {
		right = compactCount(running, total, iconColor)
		if totalRSS > 0 {
			right = dimStyle.Render(FormatStat(ProcStat{RSS: totalRSS})+" ") + right
		}
	}
	return padRightTruncate(left, right, w)
}

func (m *Model) renderWsService(item ComboItem, isCur bool, w int) string {
	icon, color := nfCircleO, "8"
	if m.IsRunning(item.TmuxName) {
		icon, color = nfCircle, "5"
	}
	style := lipgloss.NewStyle().Foreground(lipgloss.Color(color))
	if isCur {
		style = selStyle
	}
	return style.Render("  "+icon+" ") + style.Render(item.Svc)
}

func (m *Model) renderDir(item ComboItem, isCur bool, w int) string {
	alias := item.Dir
	dirAlias := item.Dir
	if dir, ok := m.Config.Repos[item.Dir]; ok && dir.Alias != "" {
		alias = dir.Alias
		dirAlias = dir.Alias
	}
	running, total := m.dirRunningCount(item.Dir, item.Branch, item.IsMain)
	_, iconColor := statusIcon(running, total)

	var totalRSS float64
	for _, s := range m.Config.AllServicesFor(item.Dir) {
		tmuxName := fmt.Sprintf("%s~%s", dirAlias, s)
		if !item.IsMain {
			tmuxName = fmt.Sprintf("%s~%s~%s", dirAlias, s, strings.ReplaceAll(item.Branch, "/", "-"))
		}
		if stat, ok := m.ProcStats[tmuxName]; ok {
			totalRSS += stat.RSS
		}
	}

	// Env indicator
	envMark := ""
	if env := services.ServiceEnvironment(filepath.Dir(m.ConfigPath), item.Branch, dirAlias); env != "" {
		envMark = lipgloss.NewStyle().Foreground(lipgloss.Color("3")).Italic(true).Render(" "+env)
	}

	style := lipgloss.NewStyle().Bold(true)
	if isCur {
		style = selStyle
	}
	iStyle := lipgloss.NewStyle().Foreground(lipgloss.Color(iconColor))
	if isCur {
		iStyle = style
	}
	left := dimStyle.Render("  ") + iStyle.Render(nfFolder+" ") + style.Render(alias) + envMark

	right := ""
	if running > 0 || isCur {
		right = compactCount(running, total, iconColor)
		if totalRSS > 0 {
			right = dimStyle.Render(FormatStat(ProcStat{RSS: totalRSS})+" ") + right
		}
	}
	return padRightTruncate(left, right, w)
}

func (m *Model) renderService(item ComboItem, isCur bool, w int) string {
	running := m.IsRunning(item.TmuxName)
	stopping, starting := m.Stopping[item.TmuxName], m.Starting[item.TmuxName]

	icon, color := nfCircleO, "8"
	if stopping || starting {
		icon, color = "~", "11"
	} else if running {
		icon, color = nfCircle, "2"
	}

	style := lipgloss.NewStyle()
	if isCur {
		switch {
		case stopping || starting:
			style = lipgloss.NewStyle().Background(lipgloss.Color("11")).Foreground(lipgloss.Color("0")).Bold(true)
		case running:
			style = lipgloss.NewStyle().Background(lipgloss.Color("2")).Foreground(lipgloss.Color("0")).Bold(true)
		default:
			style = selStyle
		}
	} else if running {
		style = lipgloss.NewStyle().Foreground(lipgloss.Color("2")).Bold(true)
	} else if stopping || starting {
		style = lipgloss.NewStyle().Foreground(lipgloss.Color("11"))
	} else {
		style = dimStyle
	}
	iStyle := lipgloss.NewStyle().Foreground(lipgloss.Color(color))
	if isCur {
		iStyle = style
	}

	// Env indicator on service
	envMark := ""
	dirAlias := item.Dir
	if dir, ok := m.Config.Repos[item.Dir]; ok && dir.Alias != "" {
		dirAlias = dir.Alias
	}
	svcKey := dirAlias + "/" + item.Svc
	if env := services.ServiceEnvironment(filepath.Dir(m.ConfigPath), item.Branch, svcKey); env != "" {
		envMark = lipgloss.NewStyle().Foreground(lipgloss.Color("3")).Italic(true).Render(" "+env)
	}

	modeMark := ""
	if dir, ok := m.Config.Repos[item.Dir]; ok {
		if svc, ok := dir.Services[item.Svc]; ok && len(svc.Modes) > 0 {
			mode := svc.Mode
			if mode == "" {
				mode = "default"
			}
			modeMark = dimStyle.Render(" [") + lipgloss.NewStyle().Foreground(lipgloss.Color("5")).Render(mode) + dimStyle.Render("]")
		}
	}

	left := dimStyle.Render("    ") + iStyle.Render(icon+" ") + style.Render(item.Svc) + envMark + modeMark

	right := ""
	if stat, ok := m.ProcStats[item.TmuxName]; ok && stat.RSS > 0 {
		right = memoryBar(stat.RSS, 6) + " " + dimStyle.Render(fmt.Sprintf("%5s", FormatStat(stat)))
	}
	if right != "" {
		return padRightTruncate(left, right, w)
	}
	return left
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

func (m *Model) isServiceInInstance(tmuxName, branch string, isMain bool) bool {
	if isMain {
		return !strings.Contains(tmuxName, "~") || strings.Count(tmuxName, "~") == 1
	}
	branchSafe := strings.ReplaceAll(branch, "/", "-")
	return strings.HasSuffix(tmuxName, "~"+branchSafe)
}

func statusIcon(running, total int) (icon, color string) {
	if running == total && total > 0 {
		return nfCircle, "2"
	} else if running > 0 {
		return nfAdjust, "3"
	}
	return nfCircleO, "8"
}

func compactCount(running, total int, color string) string {
	return lipgloss.NewStyle().Foreground(lipgloss.Color(color)).Render(fmt.Sprintf("%d/%d", running, total))
}

func (m *Model) renderStatsBar(w int) string {
	totalRunning, totalCount := m.totalRunningCount()
	var totalRSS float64
	for _, stat := range m.ProcStats {
		totalRSS += stat.RSS
	}

	memStr := ""
	if totalRSS > 0 {
		memStr = FormatStat(ProcStat{RSS: totalRSS})
	}
	memW := len(memStr)
	if memW > 0 {
		memW += 2 // "  " before mem
	}

	barWidth := w - memW
	if barWidth < 8 {
		barWidth = 8
	}

	filled := 0
	if totalCount > 0 {
		filled = barWidth * totalRunning / totalCount
	}
	empty := barWidth - filled

	greenStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("2"))
	bar := greenStyle.Render(strings.Repeat("█", filled)) + dimStyle.Render(strings.Repeat("░", empty))

	if memStr != "" {
		bar += "  " + dimStyle.Render(memStr)
	}
	return bar
}

func memoryBar(rss float64, barWidth int) string {
	if rss <= 0 {
		return ""
	}
	// Absolute scale: each block ≈ 200MB
	filled := int(rss / 200)
	if filled < 1 {
		filled = 1
	}
	if filled > barWidth {
		filled = barWidth
	}
	empty := barWidth - filled

	color := "2" // green
	if rss >= 1024 {
		color = "3" // yellow > 1GB
	}
	if rss >= 4096 {
		color = "1" // red > 4GB
	}

	return lipgloss.NewStyle().Foreground(lipgloss.Color(color)).Render(strings.Repeat("█", filled)) +
		dimStyle.Render(strings.Repeat("░", empty))
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

func padLine(line string, width int) string {
	visW := lipgloss.Width(line)
	if visW < width {
		return line + strings.Repeat(" ", width-visW)
	}
	if visW > width {
		return truncateVisible(line, width)
	}
	return line
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
