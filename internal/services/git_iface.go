package services

// GitRunner defines the interface for git operations.
// Tests can replace DefaultGit with a mock.
type GitRunner interface {
	ListWorktrees(dir string) []GitWorktree
	CurrentBranch(dir string) string
	CreateWorktreeFromBase(repoDir, newBranch, baseBranch string, copyFiles []string, workspaceDir string) (string, error)
	RemoveWorktree(repoDir, worktreePath, branch string) error
}

// DefaultGit is the git runner used by package-level functions.
var DefaultGit GitRunner = &ExecGitRunner{}

// ExecGitRunner implements GitRunner via exec.Command.
type ExecGitRunner struct{}
