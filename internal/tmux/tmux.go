package tmux

import (
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"
)

func run(args ...string) (string, bool) {
	cmd := exec.Command("tmux", args...)
	out, err := cmd.Output()
	return strings.TrimSpace(string(out)), err == nil && cmd.ProcessState.ExitCode() == 0
}

func runOk(args ...string) bool {
	_, ok := run(args...)
	return ok
}

func SessionExists(session string) bool {
	return runOk("has-session", "-t", "="+session)
}

func ListWindows(session string) map[string]bool {
	out, ok := run("list-windows", "-t", "="+session, "-F", "#{window_name}")
	if !ok {
		return nil
	}
	result := make(map[string]bool)
	for _, line := range strings.Split(out, "\n") {
		if line != "" {
			result[line] = true
		}
	}
	return result
}

func WindowExists(session, window string) bool {
	return ListWindows(session)[window]
}

func CreateSessionIfNeeded(session string) bool {
	if SessionExists(session) {
		return false
	}
	runOk("new-session", "-d", "-s", session, "-n", "_tncli_init")
	go func() {
		time.Sleep(2 * time.Second)
		if WindowExists(session, "_tncli_init") {
			KillWindow(session, "_tncli_init")
		}
	}()
	return true
}

func CleanupInitWindow(session string) {
	if WindowExists(session, "_tncli_init") {
		KillWindow(session, "_tncli_init")
	}
}

func KillWindow(session, window string) {
	runOk("kill-window", "-t", fmt.Sprintf("=%s:%s", session, window))
}

func GracefulStop(session, window string) {
	target := fmt.Sprintf("=%s:%s", session, window)
	runOk("send-keys", "-t", target, "C-c")
	time.Sleep(500 * time.Millisecond)
	KillWindow(session, window)
}

func KillSession(session string) {
	runOk("kill-session", "-t", "="+session)
}

func NewWindow(session, name, shellCmd string) {
	fullCmd := fmt.Sprintf("%s; echo -e '\\n\\033[33m[tncli] process exited. press enter to close.\\033[0m'; read", shellCmd)
	runOk("new-window", "-d", "-t", "="+session, "-n", name, "zsh", "-ic", fullCmd)
}

func NewWindowAutoclose(session, name, shellCmd string) {
	runOk("new-window", "-d", "-t", "="+session, "-n", name, "zsh", "-ic", shellCmd)
}

func CapturePane(session, window string, lines int) []string {
	target := fmt.Sprintf("=%s:%s", session, window)
	start := fmt.Sprintf("-%d", lines)
	out, ok := run("capture-pane", "-t", target, "-e", "-p", "-S", start)
	if !ok {
		return nil
	}
	result := strings.Split(out, "\n")
	if len(result) > lines+100 {
		result = result[len(result)-lines-100:]
	}
	return result
}

func InTmux() bool {
	_, ok := os.LookupEnv("TMUX")
	return ok
}

func CurrentSessionName() string {
	out, ok := run("display-message", "-p", "#{session_name}")
	if !ok || out == "" {
		return ""
	}
	return out
}

func CurrentWindowID() string {
	out, ok := run("display-message", "-p", "#{window_id}")
	if !ok || out == "" {
		return ""
	}
	return out
}

func CurrentPaneID() string {
	out, ok := run("display-message", "-p", "#{pane_id}")
	if !ok || out == "" {
		return ""
	}
	return out
}

func ListPaneIDs(windowID string) []string {
	out, ok := run("list-panes", "-t", windowID, "-F", "#{pane_id}")
	if !ok {
		return nil
	}
	var result []string
	for _, line := range strings.Split(out, "\n") {
		if line != "" {
			result = append(result, line)
		}
	}
	return result
}

func SplitWindowRight(sizePct int, cmd string) bool {
	size := fmt.Sprintf("%d%%", sizePct)
	args := []string{"split-window", "-dh", "-l", size}
	if cmd != "" {
		args = append(args, cmd)
	}
	return runOk(args...)
}

func KillPane(paneID string) {
	runOk("kill-pane", "-t", paneID)
}

func BreakPaneTo(paneID, destSession, windowName string) bool {
	return runOk("break-pane", "-d", "-s", paneID, "-t", "="+destSession+":", "-n", windowName)
}

func SelectPane(paneID string) {
	runOk("select-pane", "-t", paneID)
}

func SetPaneTitle(paneID, title string) {
	runOk("select-pane", "-t", paneID, "-T", title)
}

func SetWindowOption(windowID, option, value string) {
	runOk("set-option", "-w", "-t", windowID, option, value)
}

func UnsetWindowOption(windowID, option string) {
	runOk("set-option", "-wu", "-t", windowID, option)
}

func SwapPane(sourceSession, sourceWindow, targetPaneID string) error {
	src := fmt.Sprintf("=%s:%s", sourceSession, sourceWindow)
	cmd := exec.Command("tmux", "swap-pane", "-d", "-s", src, "-t", targetPaneID)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("%s", strings.TrimSpace(string(out)))
	}
	return nil
}

func DisplayMessage(msg string) {
	runOk("display-message", msg)
}

func DisplayPopup(width, height, cmd string) {
	runOk("display-popup", "-E", "-w", width, "-h", height, cmd)
}

type PopupOptions struct {
	Width       string
	Height      string
	Title       string
	BorderStyle string
	Style       string
	BorderLines string
}

func DisplayPopupStyled(opts PopupOptions, cmd string) {
	args := []string{"display-popup", "-E", "-w", opts.Width, "-h", opts.Height}
	if opts.Title != "" {
		args = append(args, "-T", opts.Title)
	}
	if opts.BorderStyle != "" {
		args = append(args, "-S", opts.BorderStyle)
	}
	if opts.Style != "" {
		args = append(args, "-s", opts.Style)
	}
	if opts.BorderLines != "" {
		args = append(args, "-b", opts.BorderLines)
	}
	args = append(args, cmd)
	runOk(args...)
}

func EnsureSession(session string) {
	if !SessionExists(session) {
		runOk("new-session", "-d", "-s", session)
	}
}

func SendKeys(target, keys string) {
	runOk("send-keys", "-t", target, keys)
}

func NewWindowInDir(session, name, cwd, shellCmd string) {
	runOk("new-window", "-d", "-t", "="+session, "-c", cwd, "-n", name, "zsh", "-c", shellCmd)
}

func NewSessionInDir(session, name, cwd, shellCmd string) {
	runOk("new-session", "-d", "-s", session, "-c", cwd, "-n", name, "zsh", "-c", shellCmd)
}

func AttachSession(target string) {
	cmd := exec.Command("tmux", "attach-session", "-t", target)
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	_ = cmd.Run()
}

func Attach(session string, window string) error {
	if window != "" {
		runOk("select-window", "-t", fmt.Sprintf("=%s:%s", session, window))
	}

	// Save original status-right
	originalStatus, _ := run("show-option", "-t", "="+session, "-gv", "status-right")

	runOk("set-option", "-t", "="+session, "status-right",
		" #[fg=yellow,bold] Ctrl+b d #[default]to return to tncli ")

	var status error
	if InTmux() {
		cmd := exec.Command("tmux", "switch-client", "-t", "="+session)
		cmd.Stdin = os.Stdin
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		status = cmd.Run()
	} else {
		cmd := exec.Command("tmux", "attach-session", "-t", "="+session)
		cmd.Stdin = os.Stdin
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		status = cmd.Run()
	}

	// Restore original status-right
	runOk("set-option", "-t", "="+session, "status-right", originalStatus)

	return status
}
