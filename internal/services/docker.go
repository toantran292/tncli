package services

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
)

// DockerForceCleanup removes all containers, volumes, networks for a project.
func DockerForceCleanup(projectName string) {
	filters := []string{
		"name=" + projectName,
		"label=com.docker.compose.project=" + projectName,
	}
	for _, filter := range filters {
		out, err := exec.Command("docker", "ps", "-aq", "--filter", filter).Output()
		if err != nil {
			continue
		}
		ids := nonEmpty(strings.Split(strings.TrimSpace(string(out)), "\n"))
		if len(ids) > 0 {
			args := append([]string{"rm", "-f"}, ids...)
			_ = exec.Command("docker", args...).Run()
		}
	}
	out, err := exec.Command("docker", "volume", "ls", "-q", "--filter", "name="+projectName).Output()
	if err == nil {
		ids := nonEmpty(strings.Split(strings.TrimSpace(string(out)), "\n"))
		if len(ids) > 0 {
			args := append([]string{"volume", "rm", "-f"}, ids...)
			_ = exec.Command("docker", args...).Run()
		}
	}
	_ = exec.Command("docker", "network", "rm", projectName+"_default").Run()
}

// DockerProjectName gets project name from worktree path.
func DockerProjectName(worktreePath string) string {
	dirName := filepath.Base(worktreePath)
	parentName := filepath.Base(filepath.Dir(worktreePath))
	if strings.HasPrefix(parentName, "workspace--") {
		ws := strings.TrimPrefix(parentName, "workspace--")
		return dirName + "-" + ws
	}
	return dirName
}

func CreateDockerNetwork(name string) error {
	out, err := exec.Command("docker", "network", "create", name).CombinedOutput()
	if err != nil {
		stderr := string(out)
		if strings.Contains(stderr, "already exists") {
			return nil
		}
		return fmt.Errorf("docker network create %s: %w", name, err)
	}
	return nil
}

func RemoveDockerNetwork(name string) {
	_ = exec.Command("docker", "network", "rm", name).Run()
}

func EnsureWorkspaceFolder(configDir, name string) string {
	wsFolder := filepath.Join(configDir, "workspace--"+name)
	_ = os.MkdirAll(wsFolder, 0o755)
	return wsFolder
}

func DeleteWorkspaceFolder(configDir, name string) {
	wsFolder := filepath.Join(configDir, "workspace--"+name)
	if info, err := os.Stat(wsFolder); err == nil && info.IsDir() {
		_ = os.RemoveAll(wsFolder)
	}
}

// EnsureMainWorkspace ensures main workspace folder exists with repos moved into it.
func EnsureMainWorkspace(configDir string, cfg *config.Config) string {
	branch := cfg.GlobalDefaultBranch()
	wsDir := filepath.Join(configDir, "workspace--"+branch)
	_ = os.MkdirAll(wsDir, 0o755)

	for _, dirName := range cfg.RepoOrder {
		if filepath.IsAbs(dirName) {
			continue
		}
		src := filepath.Join(configDir, dirName)
		dst := filepath.Join(wsDir, dirName)
		if info, err := os.Stat(src); err == nil && info.IsDir() {
			if _, err := os.Stat(dst); os.IsNotExist(err) {
				if os.Rename(src, dst) == nil {
					fixWorktreeRefsAfterMove(dst)
				}
			}
		}
	}
	return wsDir
}

func fixWorktreeRefsAfterMove(newRepoDir string) {
	gitDir := filepath.Join(newRepoDir, ".git")
	info, err := os.Stat(gitDir)
	if err != nil || !info.IsDir() {
		return
	}
	wtDir := filepath.Join(gitDir, "worktrees")
	if info, err := os.Stat(wtDir); err != nil || !info.IsDir() {
		return
	}

	entries, err := os.ReadDir(wtDir)
	if err != nil {
		return
	}

	for _, entry := range entries {
		gitdirFile := filepath.Join(wtDir, entry.Name(), "gitdir")
		data, err := os.ReadFile(gitdirFile)
		if err != nil {
			continue
		}
		wtGitPath := strings.TrimSpace(string(data))
		content, err := os.ReadFile(wtGitPath)
		if err != nil {
			continue
		}
		contentStr := string(content)
		if !strings.HasPrefix(contentStr, "gitdir: ") {
			continue
		}
		oldGitdir := strings.TrimSpace(contentStr[len("gitdir: "):])
		wtName := filepath.Base(oldGitdir)
		newGitdir := filepath.Join(gitDir, "worktrees", wtName)
		newContent := fmt.Sprintf("gitdir: %s\n", newGitdir)
		_ = os.WriteFile(wtGitPath, []byte(newContent), 0o644)
	}
}

func nonEmpty(ss []string) []string {
	var result []string
	for _, s := range ss {
		if s != "" {
			result = append(result, s)
		}
	}
	return result
}
