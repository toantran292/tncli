package tmux

// Runner defines the interface for tmux operations.
// Tests can replace Default with a mock implementation.
type Runner interface {
	SessionExists(session string) bool
	ListWindows(session string) map[string]bool
	WindowExists(session, window string) bool
	CreateSessionIfNeeded(session string) bool
	CleanupInitWindow(session string)
	NewWindow(session, name, shellCmd string)
	NewWindowAutoclose(session, name, shellCmd string)
	GracefulStop(session, window string)
	KillWindow(session, window string)
	KillSession(session string)
	CapturePane(session, window string, lines int) []string
}

// Default is the runner used by package-level functions.
// Replace in tests with a mock.
var Default Runner = &ExecRunner{}
