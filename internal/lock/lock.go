package lock

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

const lockDir = "/tmp/tncli"

func lockPath(session, service string) string {
	return filepath.Join(lockDir, fmt.Sprintf("%s_%s.lock", session, service))
}

func EnsureDir() {
	_ = os.MkdirAll(lockDir, 0o755)
}

func Acquire(session, service string) {
	EnsureDir()
	_ = os.WriteFile(lockPath(session, service), []byte(fmt.Sprintf("%d", os.Getpid())), 0o644)
}

func Release(session, service string) {
	_ = os.Remove(lockPath(session, service))
}

func ReleaseAll(session string) {
	entries, err := os.ReadDir(lockDir)
	if err != nil {
		return
	}
	prefix := session + "_"
	for _, e := range entries {
		name := e.Name()
		if strings.HasPrefix(name, prefix) && strings.HasSuffix(name, ".lock") {
			_ = os.Remove(filepath.Join(lockDir, name))
		}
	}
}
