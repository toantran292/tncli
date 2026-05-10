package popup

import (
	"strings"

	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
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
		{"m", "Toggle service mode (dev/build)"},
		{"o", "Open in browser"},
	}},
	{"Workspace", []keybinding{
		{"w", "Create workspace / add-remove repo"},
		{"d", "Delete workspace (confirm)"},
		{"E", "Set environment (staging/local)"},
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
	viewport viewport.Model
	ready    bool
}

func (m cheatModel) Init() tea.Cmd { return nil }

func (m cheatModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.String() {
		case "q", "esc", "?":
			return m, tea.Quit
		}
	case tea.WindowSizeMsg:
		if !m.ready {
			m.viewport = viewport.New(msg.Width, msg.Height-1)
			m.viewport.SetContent(buildCheatContent())
			m.ready = true
		} else {
			m.viewport.Width = msg.Width
			m.viewport.Height = msg.Height - 1
		}
	}
	var cmd tea.Cmd
	m.viewport, cmd = m.viewport.Update(msg)
	return m, cmd
}

func (m cheatModel) View() string {
	if !m.ready {
		return "Loading..."
	}
	footer := StyleHint.Render(" j/k=scroll  q/Esc=close")
	return m.viewport.View() + "\n" + footer
}

func buildCheatContent() string {
	sectionStyle := lipgloss.NewStyle().Bold(true).Foreground(ColorPrimary).MarginTop(1)
	keyStyle := lipgloss.NewStyle().Foreground(ColorSuccess).Bold(true).Width(14)
	descStyle := lipgloss.NewStyle().Foreground(ColorFg)
	sepStyle := lipgloss.NewStyle().Foreground(ColorMuted)

	var b strings.Builder
	for i, sec := range sections {
		if i > 0 {
			b.WriteString(sepStyle.Render("  ─────────────────────────────"))
			b.WriteString("\n")
		}
		b.WriteString(sectionStyle.Render("  " + sec.title))
		b.WriteString("\n")
		for _, kb := range sec.bindings {
			b.WriteString("    ")
			b.WriteString(keyStyle.Render(kb.key))
			b.WriteString(descStyle.Render(kb.desc))
			b.WriteString("\n")
		}
	}
	return b.String()
}

func RunCheatsheet() error {
	p := tea.NewProgram(cheatModel{}, tea.WithAltScreen())
	_, err := p.Run()
	return err
}
