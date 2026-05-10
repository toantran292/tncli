package popup

import "github.com/charmbracelet/lipgloss"

var (
	ColorPrimary   = lipgloss.Color("6")
	ColorSuccess   = lipgloss.Color("2")
	ColorWarning   = lipgloss.Color("3")
	ColorDanger    = lipgloss.Color("1")
	ColorMuted     = lipgloss.Color("8")
	ColorAccent    = lipgloss.Color("5")
	ColorHighlight = lipgloss.Color("14")
	ColorBg        = lipgloss.Color("0")
	ColorFg        = lipgloss.Color("7")
)

var (
	StyleFocus = lipgloss.NewStyle().Background(ColorPrimary).Foreground(ColorBg).Bold(true)
	StyleMuted = lipgloss.NewStyle().Foreground(ColorMuted)
	StyleHint  = lipgloss.NewStyle().Foreground(ColorMuted)
)

func RenderHint(text string) string {
	return StyleHint.Render(" " + text)
}
