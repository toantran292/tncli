package services

import (
	"fmt"
	"os"
	"strings"
)

// EnvVar represents a key-value environment variable pair.
type EnvVar struct {
	Key   string
	Value string
}

// GitWorktree represents a git worktree entry (path + branch).
type GitWorktree struct {
	Path   string
	Branch string
}

// DirMapping maps a directory name to its resolved filesystem path.
type DirMapping struct {
	Name string
	Path string
}

// DirBranch maps a directory name to its current git branch.
type DirBranch struct {
	Name   string
	Branch string
}

// FileExists checks if a path exists and is a regular file.
func FileExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}

// DirExists checks if a path exists and is a directory.
func DirExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.IsDir()
}

// ContainsStr checks if a string slice contains a value.
func ContainsStr(ss []string, s string) bool {
	for _, v := range ss {
		if v == s {
			return true
		}
	}
	return false
}

// Min returns the smaller of two ints.
func Min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

// ValidateBranchName checks that a branch name is safe for filesystem operations.
// Rejects path traversal attempts and empty names.
func ValidateBranchName(branch string) error {
	if branch == "" {
		return fmt.Errorf("branch name cannot be empty")
	}
	if strings.Contains(branch, "..") {
		return fmt.Errorf("branch name cannot contain '..'")
	}
	if strings.HasPrefix(branch, "/") || strings.HasPrefix(branch, "~") {
		return fmt.Errorf("branch name cannot start with '/' or '~'")
	}
	return nil
}
