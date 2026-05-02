package paths

import (
	"os"
	"path/filepath"
	"testing"
)

func TestStateDir_XDG(t *testing.T) {
	tmp := t.TempDir()
	t.Setenv("XDG_STATE_HOME", tmp)
	t.Setenv("HOME", tmp) // isolate from real ~/.tncli

	dir := StateDir()
	want := filepath.Join(tmp, "tncli")
	if dir != want {
		t.Errorf("StateDir() = %q, want %q", dir, want)
	}
}

func TestStatePath(t *testing.T) {
	tmp := t.TempDir()
	t.Setenv("XDG_STATE_HOME", tmp)
	t.Setenv("HOME", tmp)

	got := StatePath("slots.json")
	want := filepath.Join(tmp, "tncli", "slots.json")
	if got != want {
		t.Errorf("StatePath() = %q, want %q", got, want)
	}
}

func TestEnsureStateDir(t *testing.T) {
	tmp := t.TempDir()
	t.Setenv("XDG_STATE_HOME", tmp)

	dir := EnsureStateDir()
	if _, err := os.Stat(dir); os.IsNotExist(err) {
		t.Errorf("EnsureStateDir did not create %q", dir)
	}
}

func TestStateDir_FallbackLegacy(t *testing.T) {
	tmp := t.TempDir()
	t.Setenv("XDG_STATE_HOME", tmp)
	t.Setenv("HOME", tmp)

	// Create legacy dir
	legacy := filepath.Join(tmp, ".tncli")
	_ = os.MkdirAll(legacy, 0o755)

	// XDG dir doesn't exist → should fall back to legacy
	dir := StateDir()
	if dir != legacy {
		t.Errorf("StateDir() = %q, want legacy %q", dir, legacy)
	}
}

func TestMigrateFromLegacy(t *testing.T) {
	tmp := t.TempDir()
	t.Setenv("HOME", tmp)
	t.Setenv("XDG_STATE_HOME", "")

	legacy := filepath.Join(tmp, ".tncli")
	_ = os.MkdirAll(legacy, 0o755)
	_ = os.WriteFile(filepath.Join(legacy, "test.json"), []byte("{}"), 0o644)

	ok := MigrateFromLegacy()
	if !ok {
		t.Fatal("MigrateFromLegacy returned false")
	}

	xdgDir := filepath.Join(tmp, ".local", "state", "tncli")
	if _, err := os.Stat(filepath.Join(xdgDir, "test.json")); os.IsNotExist(err) {
		t.Error("file not migrated to XDG dir")
	}

	// Legacy should be a symlink now
	info, err := os.Lstat(legacy)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode()&os.ModeSymlink == 0 {
		t.Error("legacy dir should be a symlink after migration")
	}
}
