package paths

import (
	"os"
	"path/filepath"
)

// StateDir returns the directory for tncli state files.
// Respects XDG_STATE_HOME. Default: ~/.local/state/tncli
// Falls back to legacy ~/.tncli if it exists and XDG dir doesn't.
func StateDir() string {
	xdg := os.Getenv("XDG_STATE_HOME")
	if xdg == "" {
		home, _ := os.UserHomeDir()
		if home == "" {
			return "/tmp/tncli"
		}
		xdg = filepath.Join(home, ".local", "state")
	}
	xdgDir := filepath.Join(xdg, "tncli")

	// If XDG dir exists, use it
	if isDir(xdgDir) {
		return xdgDir
	}

	// If legacy dir exists and XDG doesn't, use legacy
	legacy := legacyDir()
	if isDir(legacy) {
		return legacy
	}

	// Fresh install: use XDG
	return xdgDir
}

// StatePath returns the full path for a state file.
func StatePath(rel string) string {
	return filepath.Join(StateDir(), rel)
}

// EnsureStateDir creates the state directory if needed.
func EnsureStateDir() string {
	dir := StateDir()
	_ = os.MkdirAll(dir, 0o755)
	return dir
}

// LegacyDir returns the old ~/.tncli path for migration checks.
func LegacyDir() string {
	return legacyDir()
}

// MigrateFromLegacy moves files from ~/.tncli/ to XDG state dir.
// Only runs if legacy exists and XDG doesn't.
func MigrateFromLegacy() bool {
	legacy := legacyDir()
	if !isDir(legacy) {
		return false
	}

	xdg := os.Getenv("XDG_STATE_HOME")
	if xdg == "" {
		home, _ := os.UserHomeDir()
		if home == "" {
			return false
		}
		xdg = filepath.Join(home, ".local", "state")
	}
	xdgDir := filepath.Join(xdg, "tncli")

	if isDir(xdgDir) {
		return false // already migrated
	}

	_ = os.MkdirAll(filepath.Dir(xdgDir), 0o755)
	if os.Rename(legacy, xdgDir) != nil {
		return false
	}
	// Leave a symlink for tools that may still reference ~/.tncli
	_ = os.Symlink(xdgDir, legacy)
	return true
}

func legacyDir() string {
	home, _ := os.UserHomeDir()
	if home == "" {
		return "/tmp/.tncli"
	}
	return filepath.Join(home, ".tncli")
}

func isDir(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.IsDir()
}
