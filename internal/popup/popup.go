package popup

import (
	"fmt"
	"os"
	"strings"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

const ResultFile = "/tmp/tncli-popup-result"

// ── Text Input ──

type inputModel struct {
	input string
}

func (m inputModel) Init() tea.Cmd { return nil }

func (m inputModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	if msg, ok := msg.(tea.KeyMsg); ok {
		switch msg.String() {
		case "enter":
			if strings.TrimSpace(m.input) != "" {
				_ = os.WriteFile(ResultFile, []byte(strings.TrimSpace(m.input)), 0o644)
			}
			return m, tea.Quit
		case "esc":
			return m, tea.Quit
		case "backspace":
			if len(m.input) > 0 {
				m.input = m.input[:len(m.input)-1]
			}
		default:
			if len(msg.String()) == 1 {
				m.input += msg.String()
			}
		}
	}
	return m, nil
}

func (m inputModel) View() string {
	prompt := lipgloss.NewStyle().Foreground(lipgloss.Color("14")).Render(" > ")
	cursor := lipgloss.NewStyle().Foreground(lipgloss.Color("8")).Render("_")
	hint := lipgloss.NewStyle().Foreground(lipgloss.Color("8")).Render(" Enter=confirm  Esc=cancel")
	return "\n" + prompt + m.input + cursor + "\n" + hint
}

func RunInput() error {
	_ = os.Remove(ResultFile)
	p := tea.NewProgram(inputModel{}, tea.WithAltScreen())
	_, err := p.Run()
	return err
}

// ── Workspace Repo Selection ──

type wsItem struct {
	alias, source, target, path string
	selected                    bool
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
		case "b":
			if m.items[m.cursor].selected && m.items[m.cursor].path != "" {
				result := fmt.Sprintf("BRANCH_PICK:%d:%s", m.cursor, serializeItems(m.items))
				_ = os.WriteFile(ResultFile, []byte(result), 0o644)
				return m, tea.Quit
			}
		case "enter":
			var lines []string
			for _, item := range m.items {
				if item.selected {
					lines = append(lines, fmt.Sprintf("%s:%s", item.alias, item.target))
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
		check := "[ ]"
		if item.selected {
			check = "[x]"
		}

		var line string
		if !item.selected {
			line = fmt.Sprintf(" %s %-*s  -", check, aliasW, item.alias)
		} else if item.source != item.target {
			line = fmt.Sprintf(" %s %-*s  %s -> %s", check, aliasW, item.alias, item.source, item.target)
		} else {
			line = fmt.Sprintf(" %s %-*s  %s -> %s", check, aliasW, item.alias, item.source, item.target)
		}

		style := lipgloss.NewStyle()
		if isCur {
			style = style.Background(lipgloss.Color("14")).Foreground(lipgloss.Color("0")).Bold(true)
		} else if !item.selected {
			style = style.Foreground(lipgloss.Color("8"))
		}
		lines = append(lines, style.Render(line))
	}

	footer := lipgloss.NewStyle().Foreground(lipgloss.Color("8")).Render(
		" Space=toggle  b=branch  Enter=create  Esc=cancel")
	lines = append(lines, "", footer)
	return strings.Join(lines, "\n")
}

func serializeItems(items []wsItem) string {
	var parts []string
	for _, i := range items {
		sel := "0"
		if i.selected {
			sel = "1"
		}
		parts = append(parts, fmt.Sprintf("%s|%s|%s|%s|%s", i.alias, i.source, i.target, i.path, sel))
	}
	return strings.Join(parts, ",")
}

func RunWsSelect(data string) error {
	_ = os.Remove(ResultFile)
	var items []wsItem
	for _, entry := range strings.Split(data, ",") {
		if entry == "" {
			continue
		}
		parts := strings.SplitN(entry, "|", 5)
		if len(parts) < 4 {
			continue
		}
		selected := true
		if len(parts) >= 5 && parts[4] == "0" {
			selected = false
		}
		items = append(items, wsItem{
			alias: parts[0], source: parts[1], target: parts[2], path: parts[3],
			selected: selected,
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
	selected bool // false = No, true = Yes
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
	yesStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("8"))
	noStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("8"))
	if m.selected {
		yesStyle = lipgloss.NewStyle().Background(lipgloss.Color("2")).Foreground(lipgloss.Color("0")).Bold(true)
	} else {
		noStyle = lipgloss.NewStyle().Background(lipgloss.Color("1")).Foreground(lipgloss.Color("0")).Bold(true)
	}
	btnLine := "  " + yesStyle.Render(" Yes ") + "   " + noStyle.Render(" No ")
	hint := lipgloss.NewStyle().Foreground(lipgloss.Color("8")).Render(" y/n  Tab=switch  Enter=confirm")
	return "\n" + btnLine + "\n\n" + hint
}

func RunConfirm() error {
	_ = os.Remove(ResultFile)
	p := tea.NewProgram(confirmModel{}, tea.WithAltScreen())
	_, err := p.Run()
	return err
}
