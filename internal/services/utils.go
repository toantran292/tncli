package services

import (
	"fmt"
	"os"
	"strings"
)

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
