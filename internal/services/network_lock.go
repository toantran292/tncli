package services

import (
	"fmt"
	"os"
	"syscall"
	"time"
)

// WithProjectLock provides file-lock protected operation for a project.
func WithProjectLock(projectDir string, fn func()) {
	lp := netLockPath(projectDir)
	_ = os.MkdirAll(lp[:len(lp)-len("/network.lock")], 0o755)

	deadline := time.Now().Add(30 * time.Second)
	for {
		f, err := os.OpenFile(lp, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
		if err == nil {
			fmt.Fprintf(f, "%d", os.Getpid())
			f.Close()
			break
		}
		if time.Now().After(deadline) {
			_ = os.Remove(lp)
			continue
		}
		if info, err := os.Stat(lp); err == nil {
			if time.Since(info.ModTime()) > 10*time.Second {
				if data, err := os.ReadFile(lp); err == nil {
					var pid int
					if _, err := fmt.Sscanf(string(data), "%d", &pid); err != nil || !isProcessAlive(pid) {
						_ = os.Remove(lp)
						continue
					}
				}
			}
		}
		time.Sleep(50 * time.Millisecond)
	}
	defer os.Remove(lp)
	fn()
}

func isProcessAlive(pid int) bool {
	if pid <= 0 {
		return false
	}
	p, err := os.FindProcess(pid)
	if err != nil {
		return false
	}
	return p.Signal(syscall.Signal(0)) == nil
}
