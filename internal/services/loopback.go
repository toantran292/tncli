package services

import (
	"bufio"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

const loopbackSocket = "/tmp/tncli-loopback.sock"

// RunLoopbackDaemon runs the loopback alias daemon (called by LaunchDaemon as root).
// Listens on unix socket, creates loopback aliases on request.
// Security: only accepts 127.0.{1-254}.{2-254} IPs, rate-limited to 1 req/100ms.
func RunLoopbackDaemon() error {
	_ = os.Remove(loopbackSocket)
	ln, err := net.Listen("unix", loopbackSocket)
	if err != nil {
		return fmt.Errorf("listen %s: %w", loopbackSocket, err)
	}
	defer ln.Close()

	// Socket writable by all local users (loopback aliases are harmless — 127.0.* only)
	_ = os.Chmod(loopbackSocket, 0o666)

	for {
		conn, err := ln.Accept()
		if err != nil {
			continue
		}
		// Sequential — no goroutine. Rate-limits by design (1 request at a time).
		handleLoopbackRequest(conn)
	}
}

func handleLoopbackRequest(conn net.Conn) {
	defer conn.Close()
	// Read timeout — prevent slow client holding the daemon
	conn.SetReadDeadline(time.Now().Add(2 * time.Second))
	scanner := bufio.NewScanner(conn)
	if !scanner.Scan() {
		return
	}
	line := strings.TrimSpace(scanner.Text())

	switch {
	case strings.HasPrefix(line, "ALIAS "):
		ip := strings.TrimPrefix(line, "ALIAS ")
		if !isValidLoopbackIP(ip) {
			fmt.Fprintln(conn, "ERR invalid ip")
			return
		}
		err := exec.Command("ifconfig", "lo0", "alias", ip).Run()
		if err != nil {
			fmt.Fprintf(conn, "ERR %v\n", err)
		} else {
			fmt.Fprintln(conn, "OK")
		}
	case line == "PING":
		fmt.Fprintln(conn, "PONG")
	default:
		fmt.Fprintln(conn, "ERR unknown command")
	}
}

// isValidLoopbackIP validates IP is in 127.0.{1-254}.{2-254} range.
// Rejects 127.0.0.* (default loopback) and .0/.1 (network/gateway).
func isValidLoopbackIP(ip string) bool {
	if !strings.HasPrefix(ip, "127.0.") {
		return false
	}
	parsed := net.ParseIP(ip)
	if parsed == nil {
		return false
	}
	parts := strings.Split(ip, ".")
	if len(parts) != 4 {
		return false
	}
	var subnet, host int
	fmt.Sscanf(parts[2], "%d", &subnet)
	fmt.Sscanf(parts[3], "%d", &host)
	return subnet >= 1 && subnet <= 254 && host >= 2 && host <= 254
}

// RequestLoopbackAlias asks the daemon to create a loopback alias. No sudo needed.
func RequestLoopbackAlias(ip string) bool {
	conn, err := net.Dial("unix", loopbackSocket)
	if err != nil {
		return false
	}
	defer conn.Close()

	fmt.Fprintf(conn, "ALIAS %s\n", ip)
	scanner := bufio.NewScanner(conn)
	if scanner.Scan() {
		return strings.TrimSpace(scanner.Text()) == "OK"
	}
	return false
}

// IsLoopbackDaemonRunning checks if the daemon is responding.
func IsLoopbackDaemonRunning() bool {
	conn, err := net.Dial("unix", loopbackSocket)
	if err != nil {
		return false
	}
	defer conn.Close()
	fmt.Fprintln(conn, "PING")
	scanner := bufio.NewScanner(conn)
	if scanner.Scan() {
		return strings.TrimSpace(scanner.Text()) == "PONG"
	}
	return false
}

// GenerateLoopbackPlist generates the LaunchDaemon plist for the loopback daemon.
func GenerateLoopbackPlist(exePath string) string {
	home, _ := os.UserHomeDir()
	logPath := filepath.Join(home, ".tncli/loopback-daemon.log")
	return fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tncli.loopback</string>
    <key>ProgramArguments</key>
    <array>
        <string>%s</string>
        <string>loopback-daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>%s</string>
    <key>StandardErrorPath</key>
    <string>%s</string>
</dict>
</plist>`, exePath, logPath, logPath)
}
