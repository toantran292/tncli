package popup

import (
	"strings"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

var (
	titleStyle   = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("6")).MarginBottom(1)
	sectionStyle = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("3")).MarginTop(1)
	keyStyle     = lipgloss.NewStyle().Foreground(lipgloss.Color("2")).Width(14)
	descStyle    = lipgloss.NewStyle().Foreground(lipgloss.Color("7"))
	dimStyle     = lipgloss.NewStyle().Foreground(lipgloss.Color("8"))
)

type keybinding struct {
	key  string
	desc string
}

type section struct {
	title    string
	bindings []keybinding
}

var sections = []section{
	{"Navigation", []keybinding{
		{"j / k", "Move down / up"},
		{"Enter", "Toggle start/stop or collapse"},
		{"Space", "Toggle start/stop or collapse"},
		{"Tab / l", "Focus service pane"},
		{"n / N", "Cycle running services"},
	}},
	{"Services", []keybinding{
		{"s", "Start service / instance"},
		{"x", "Stop service / instance"},
		{"X", "Stop all (confirm)"},
		{"r", "Restart service"},
		{"o", "Open in browser"},
	}},
	{"Workspace", []keybinding{
		{"w", "Create workspace / add-remove repo"},
		{"d", "Delete workspace (confirm)"},
		{"B", "Database menu (create/drop/reset)"},
	}},
	{"Tools", []keybinding{
		{"c", "Shortcuts popup"},
		{"e", "Open in editor"},
		{"g", "Git: checkout / pull / diff"},
		{"t", "Shell in popup"},
		{"I", "Shared services (lazydocker)"},
		{"R", "Reload config"},
	}},
	{"Global", []keybinding{
		{"?", "This cheat-sheet"},
		{"q", "Quit"},
	}},
}

type cheatModel struct {
	width, height int
	scroll        int
}

func (m cheatModel) Init() tea.Cmd { return nil }

func (m cheatModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.String() {
		case "q", "esc", "?":
			return m, tea.Quit
		case "j", "down":
			m.scroll++
		case "k", "up":
			if m.scroll > 0 {
				m.scroll--
			}
		}
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
	}
	return m, nil
}

func (m cheatModel) View() string {
	var b strings.Builder


	for _, sec := range sections {
		b.WriteString(sectionStyle.Render("  " + sec.title))
		b.WriteString("\n")
		for _, kb := range sec.bindings {
			b.WriteString("  ")
			b.WriteString(keyStyle.Render(kb.key))
			b.WriteString(descStyle.Render(kb.desc))
			b.WriteString("\n")
		}
	}

	b.WriteString("\n")
	b.WriteString(dimStyle.Render("  q/esc to close"))

	lines := strings.Split(b.String(), "\n")
	if m.scroll > 0 && m.scroll < len(lines) {
		lines = lines[m.scroll:]
	}
	if m.height > 0 && len(lines) > m.height-1 {
		lines = lines[:m.height-1]
	}

	return strings.Join(lines, "\n")
}

func RunCheatsheet() error {
	p := tea.NewProgram(cheatModel{}, tea.WithAltScreen())
	_, err := p.Run()
	return err
}
