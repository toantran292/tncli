package services

import (
	"fmt"
	"os"
	"os/exec"
	"strings"
)

// ── GitRunner interface implementation ──

func (r *ExecGitRunner) ListWorktrees(dir string) []GitWorktree {
	out, err := exec.Command("git", "-C", dir, "worktree", "list", "--porcelain").Output()
	if err != nil {
		return nil
	}

	var result []GitWorktree
	var currentPath, currentBranch string
	for _, line := range strings.Split(string(out), "\n") {
		if path, ok := strings.CutPrefix(line, "worktree "); ok {
			currentPath = path
		} else if branch, ok := strings.CutPrefix(line, "branch refs/heads/"); ok {
			currentBranch = branch
		} else if line == "" && currentPath != "" {
			if currentBranch != "" {
				result = append(result, GitWorktree{Path: currentPath, Branch: currentBranch})
			}
			currentPath = ""
			currentBranch = ""
		}
	}
	if currentPath != "" && currentBranch != "" {
		result = append(result, GitWorktree{Path: currentPath, Branch: currentBranch})
	}
	return result
}

func (r *ExecGitRunner) CurrentBranch(dir string) string {
	out, err := exec.Command("git", "-C", dir, "rev-parse", "--abbrev-ref", "HEAD").Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

func (r *ExecGitRunner) CreateWorktreeFromBase(repoDir, newBranch, baseBranch string, copyFilesList []string, workspaceDir string) (string, error) {
	repoName := fileBase(repoDir)

	var worktreeDir string
	if workspaceDir != "" {
		worktreeDir = workspaceDir + "/" + repoName
	} else {
		dirSuffix := strings.ReplaceAll(newBranch, "/", "-")
		worktreeDir = repoDir + "/../" + repoName + "--" + dirSuffix
	}

	DockerForceCleanup(DockerProjectName(worktreeDir))

	if _, err := os.Stat(worktreeDir); err == nil {
		return "", fmt.Errorf("worktree directory already exists: %s", worktreeDir)
	}

	branchExists := exec.Command("git", "-C", repoDir, "rev-parse", "--verify", newBranch).Run() == nil
	remoteExists := !branchExists && exec.Command("git", "-C", repoDir, "rev-parse", "--verify", "origin/"+newBranch).Run() == nil

	var cmd *exec.Cmd
	if branchExists {
		cmd = exec.Command("git", "-C", repoDir, "worktree", "add", worktreeDir, newBranch)
	} else if remoteExists {
		cmd = exec.Command("git", "-C", repoDir, "worktree", "add", "--track", "-b", newBranch, worktreeDir, "origin/"+newBranch)
	} else {
		cmd = exec.Command("git", "-C", repoDir, "worktree", "add", "-b", newBranch, worktreeDir, baseBranch)
	}

	if out, err := cmd.CombinedOutput(); err != nil {
		return "", fmt.Errorf("git worktree add: %s (%w)", strings.TrimSpace(string(out)), err)
	}

	CopyFiles(repoDir, worktreeDir, copyFilesList)
	return worktreeDir, nil
}

func (r *ExecGitRunner) RemoveWorktree(repoDir, worktreePath, branch string) error {
	if info, err := os.Stat(worktreePath); err == nil && info.IsDir() {
		if _, err := os.Stat(worktreePath + "/docker-compose.yml"); err == nil {
			cmd := exec.Command("docker", "compose", "down", "-v", "--remove-orphans", "--timeout", "5")
			cmd.Dir = worktreePath
			_ = cmd.Run()
		}
	}

	DockerForceCleanup(DockerProjectName(worktreePath))
	_ = exec.Command("git", "-C", repoDir, "worktree", "remove", "--force", worktreePath).Run()

	if _, err := os.Stat(worktreePath); err == nil {
		_ = os.RemoveAll(worktreePath)
	}

	_ = exec.Command("git", "-C", repoDir, "worktree", "prune").Run()
	_ = exec.Command("git", "-C", repoDir, "branch", "-D", branch).Run()
	return nil
}

// ── Package-level functions (delegate to DefaultGit) ──

func ListWorktrees(dir string) []GitWorktree { return DefaultGit.ListWorktrees(dir) }
func CurrentBranch(dir string) string         { return DefaultGit.CurrentBranch(dir) }

func CreateWorktreeFromBase(repoDir, newBranch, baseBranch string, copyFiles []string, workspaceDir string) (string, error) {
	return DefaultGit.CreateWorktreeFromBase(repoDir, newBranch, baseBranch, copyFiles, workspaceDir)
}

func RemoveWorktree(repoDir, worktreePath, branch string) error {
	return DefaultGit.RemoveWorktree(repoDir, worktreePath, branch)
}

// ── Non-interface git helpers ──

func IsBranchInWorktree(dir, branch string) bool {
	for _, wt := range ListWorktrees(dir) {
		if wt.Branch == branch {
			return true
		}
	}
	return false
}

func Checkout(dir, branch string) error {
	out, err := exec.Command("git", "-C", dir, "checkout", branch).CombinedOutput()
	if err != nil {
		return fmt.Errorf("git checkout %s: %s (%w)", branch, strings.TrimSpace(string(out)), err)
	}
	return nil
}

func CheckoutNewBranch(dir, branch string) error {
	out, err := exec.Command("git", "-C", dir, "checkout", "-b", branch).CombinedOutput()
	if err != nil {
		return fmt.Errorf("git checkout -b %s: %s (%w)", branch, strings.TrimSpace(string(out)), err)
	}
	return nil
}

func CreateWorktree(repoDir, branch string, copyFilesList []string) (string, error) {
	dirSuffix := strings.ReplaceAll(branch, "/", "-")
	repoName := fileBase(repoDir)
	parent := repoDir + "/.."
	worktreeDir := parent + "/" + repoName + "--" + dirSuffix

	if _, err := os.Stat(worktreeDir); err == nil {
		return "", fmt.Errorf("worktree directory already exists: %s", worktreeDir)
	}

	out, err := exec.Command("git", "-C", repoDir, "worktree", "add", worktreeDir, branch).CombinedOutput()
	if err != nil {
		stderr := string(out)
		if strings.Contains(stderr, "already checked out") || strings.Contains(stderr, "is already used") {
			newBranch := "wt/" + dirSuffix
			out2, err2 := exec.Command("git", "-C", repoDir, "worktree", "add", worktreeDir, newBranch).CombinedOutput()
			if err2 != nil {
				out3, err3 := exec.Command("git", "-C", repoDir, "worktree", "add", "-b", newBranch, worktreeDir, branch).CombinedOutput()
				if err3 != nil {
					return "", fmt.Errorf("git worktree add failed: %s", strings.TrimSpace(string(out3)))
				}
				_ = out2
			}
		} else {
			return "", fmt.Errorf("git worktree add failed: %s", strings.TrimSpace(stderr))
		}
	}

	CopyFiles(repoDir, worktreeDir, copyFilesList)
	return worktreeDir, nil
}

func fileBase(path string) string {
	parts := strings.Split(strings.TrimRight(path, "/"), "/")
	if len(parts) == 0 {
		return "repo"
	}
	return parts[len(parts)-1]
}
