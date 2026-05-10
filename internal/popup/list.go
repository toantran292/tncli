package popup

import (
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/charmbracelet/bubbles/list"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

// listItem implements list.Item for simple string items.
type listItem string

func (i listItem) FilterValue() string { return string(i) }

// listDelegate renders each item in the list.
type listDelegate struct{}

func (d listDelegate) Height() int                             { return 1 }
func (d listDelegate) Spacing() int                            { return 0 }
func (d listDelegate) Update(_ tea.Msg, _ *list.Model) tea.Cmd { return nil }

func (d listDelegate) Render(w io.Writer, m list.Model, index int, item list.Item) {
	str := fmt.Sprintf("%s", item.(listItem))

	if index == m.Index() {
		fmt.Fprint(w, lipgloss.NewStyle().
			PaddingLeft(1).
			Foreground(lipgloss.Color("#EE6FF8")).
			Render("▸ "+str))
	} else {
		fmt.Fprint(w, lipgloss.NewStyle().
			PaddingLeft(3).
			Foreground(ColorFg).
			Render(str))
	}
}

type listModel struct {
	list list.Model
}

func (m listModel) Init() tea.Cmd { return nil }

func (m listModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.String() {
		case "enter":
			if item, ok := m.list.SelectedItem().(listItem); ok {
				_ = os.WriteFile(ResultFile, []byte(string(item)), 0o644)
			}
			return m, tea.Quit
		case "esc", "q":
			if m.list.FilterState() == list.Filtering {
				m.list.ResetFilter()
				return m, nil
			}
			return m, tea.Quit
		}
	case tea.WindowSizeMsg:
		m.list.SetSize(msg.Width, msg.Height)
		return m, nil
	}

	var cmd tea.Cmd
	m.list, cmd = m.list.Update(msg)
	return m, cmd
}

func (m listModel) View() string {
	return m.list.View()
}

func RunList(data string) error {
	_ = os.Remove(ResultFile)
	lines := strings.Split(strings.TrimSpace(data), "\n")
	var items []list.Item
	for _, line := range lines {
		if line != "" {
			items = append(items, listItem(line))
		}
	}
	if len(items) == 0 {
		return nil
	}

	delegate := listDelegate{}
	l := list.New(items, delegate, 0, 0)
	l.SetShowTitle(false)
	l.SetShowStatusBar(false)
	l.SetShowHelp(false)
	l.SetFilteringEnabled(true)
	l.DisableQuitKeybindings()

	l.Styles.NoItems = StyleMuted

	p := tea.NewProgram(listModel{list: l}, tea.WithAltScreen())
	_, err := p.Run()
	return err
}
