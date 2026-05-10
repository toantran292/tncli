package popup

import (
	"fmt"
	"os"
	"strings"

	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

const ResultFile = "/tmp/tncli-popup-result"

// ── Text Input ──

type inputModel struct {
	ti textinput.Model
}

func (m inputModel) Init() tea.Cmd {
	return textinput.Blink
}

func (m inputModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	if msg, ok := msg.(tea.KeyMsg); ok {
		switch msg.String() {
		case "enter":
			val := strings.TrimSpace(m.ti.Value())
			if val != "" {
				_ = os.WriteFile(ResultFile, []byte(val), 0o644)
			}
			return m, tea.Quit
		case "esc":
			return m, tea.Quit
		}
	}
	var cmd tea.Cmd
	m.ti, cmd = m.ti.Update(msg)
	return m, cmd
}

func (m inputModel) View() string {
	hint := StyleHint.Render(" Enter=confirm  Esc=cancel")
	return "\n " + m.ti.View() + "\n" + hint
}

func RunInput() error {
	_ = os.Remove(ResultFile)
	ti := textinput.New()
	ti.Prompt = lipgloss.NewStyle().Foreground(ColorHighlight).Render("▸ ")
	ti.Cursor.Style = lipgloss.NewStyle().Foreground(ColorSuccess)
	ti.Focus()
	p := tea.NewProgram(inputModel{ti: ti}, tea.WithAltScreen())
	_, err := p.Run()
	return err
}

// ── Workspace Repo Selection ──

type wsItem struct {
	alias, source, target, path, dirName string
	selected                             bool
}

type wsSelectModel struct {
	items  []wsItem
	cursor int
}

func (m wsSelectModel) Init() tea.Cmd { return nil }

func (m wsSelectModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	if msg, ok := msg.(tea.KeyMsg); ok {
		switch msg.String() {
		case "esc", "q":
			return m, tea.Quit
		case "up", "k":
			if m.cursor > 0 {
				m.cursor--
			}
		case "down", "j":
			if m.cursor+1 < len(m.items) {
				m.cursor++
			}
		case " ":
			m.items[m.cursor].selected = !m.items[m.cursor].selected
		case "enter":
			var lines []string
			for _, item := range m.items {
				if item.selected {
					lines = append(lines, fmt.Sprintf("%s:%s", item.dirName, item.target))
				}
			}
			if len(lines) > 0 {
				_ = os.WriteFile(ResultFile, []byte(strings.Join(lines, "\n")), 0o644)
			}
			return m, tea.Quit
		}
	}
	return m, nil
}

func (m wsSelectModel) View() string {
	var lines []string
	aliasW := 4
	for _, item := range m.items {
		if len(item.alias) > aliasW {
			aliasW = len(item.alias)
		}
	}

	for i, item := range m.items {
		isCur := i == m.cursor
		check := lipgloss.NewStyle().Foreground(ColorMuted).Render("[ ]")
		if item.selected {
			check = lipgloss.NewStyle().Foreground(ColorSuccess).Render("[✓]")
		}

		var line string
		if !item.selected {
			line = fmt.Sprintf(" %s %-*s  -", check, aliasW, item.alias)
		} else {
			line = fmt.Sprintf(" %s %-*s  %s → %s", check, aliasW, item.alias, item.source, item.target)
		}

		style := lipgloss.NewStyle()
		if isCur {
			style = StyleFocus
		} else if !item.selected {
			style = StyleMuted
		}
		lines = append(lines, style.Render(line))
	}

	lines = append(lines, "")
	lines = append(lines, RenderHint("Space=toggle  Enter=create  Esc=cancel"))
	return strings.Join(lines, "\n")
}

func RunWsSelect(data string) error {
	_ = os.Remove(ResultFile)
	var items []wsItem
	for _, entry := range strings.Split(data, ",") {
		if entry == "" {
			continue
		}
		parts := strings.SplitN(entry, "|", 6)
		if len(parts) < 5 {
			continue
		}
		selected := true
		if len(parts) >= 6 && parts[5] == "0" {
			selected = false
		}
		items = append(items, wsItem{
			alias: parts[0], source: parts[1], target: parts[2], path: parts[3],
			dirName: parts[4], selected: selected,
		})
	}
	if len(items) == 0 {
		return nil
	}
	p := tea.NewProgram(wsSelectModel{items: items}, tea.WithAltScreen())
	_, err := p.Run()
	return err
}

// ── Confirm Dialog ──

type confirmModel struct {
	selected bool
}

func (m confirmModel) Init() tea.Cmd { return nil }

func (m confirmModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	if msg, ok := msg.(tea.KeyMsg); ok {
		switch msg.String() {
		case "esc", "n":
			return m, tea.Quit
		case "y":
			_ = os.WriteFile(ResultFile, []byte("y"), 0o644)
			return m, tea.Quit
		case "left", "right", "h", "l", "tab":
			m.selected = !m.selected
		case "enter":
			if m.selected {
				_ = os.WriteFile(ResultFile, []byte("y"), 0o644)
			}
			return m, tea.Quit
		}
	}
	return m, nil
}

func (m confirmModel) View() string {
	yesStyle := StyleMuted
	noStyle := StyleMuted
	if m.selected {
		yesStyle = lipgloss.NewStyle().Background(ColorSuccess).Foreground(ColorBg).Bold(true)
	} else {
		noStyle = lipgloss.NewStyle().Background(ColorDanger).Foreground(ColorBg).Bold(true)
	}
	btnLine := "  " + yesStyle.Render(" Yes ") + "   " + noStyle.Render(" No ")
	hint := RenderHint("y/n  Tab=switch  Enter=confirm")
	return "\n" + btnLine + "\n\n" + hint
}

func RunConfirm() error {
	_ = os.Remove(ResultFile)
	p := tea.NewProgram(confirmModel{}, tea.WithAltScreen())
	_, err := p.Run()
	return err
}
