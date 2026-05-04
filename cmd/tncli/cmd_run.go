package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/services"
)

var runCmd = &cobra.Command{
	Use:   "run <shortcut> [repo]",
	Short: "Run a shortcut command in a repo directory",
	Long: `Run a shortcut defined in tncli.yml. Sources .env.local before execution.

Examples:
  tncli run "dip migrate"              # run in first repo
  tncli run "dip migrate" api          # run in specific repo (by alias)
  tncli run "dip bundle install" api   # any shell command works`,
	Args: cobra.RangeArgs(1, 2),
	RunE: func(cmd *cobra.Command, args []string) error {
		shellCmd := args[0]
		configDir := filepath.Dir(configPath)
		branch, _ := cmd.Flags().GetString("branch")
		if branch == "" {
			branch = appConfig.GlobalDefaultBranch()
		}

		// Find repo dir
		var workDir string
		if len(args) >= 2 {
			target := args[1]
			for dirName, dir := range appConfig.Repos {
				if dirName == target || dir.Alias == target {
					workDir = filepath.Join(configDir, "workspace--"+branch, dirName)
					break
				}
			}
			if workDir == "" {
				return fmt.Errorf("repo '%s' not found", target)
			}
		} else {
			if len(appConfig.RepoOrder) > 0 {
				workDir = filepath.Join(configDir, "workspace--"+branch, appConfig.RepoOrder[0])
			}
		}

		if _, err := os.Stat(workDir); os.IsNotExist(err) {
			return fmt.Errorf("directory not found: %s", workDir)
		}

		// Regenerate env + compose override for this workspace
		services.RegenerateWorkspaceEnv(configDir, appConfig, branch)

		// Build command with env sourcing
		fullCmd := fmt.Sprintf("cd '%s' && set -a && source .env.local 2>/dev/null; set +a && export DOTENV_CONFIG_PATH=.env.local && %s", workDir, shellCmd)
		c := exec.Command("zsh", "-c", fullCmd)
		c.Stdin = os.Stdin
		c.Stdout = os.Stdout
		c.Stderr = os.Stderr
		return c.Run()
	},
}

var runListCmd = &cobra.Command{
	Use:   "shortcuts",
	Short: "List available shortcuts",
	Run: func(cmd *cobra.Command, args []string) {
		for _, dirName := range appConfig.RepoOrder {
			dir := appConfig.Repos[dirName]
			alias := dirName
			if dir.Alias != "" {
				alias = dir.Alias
			}
			if len(dir.Shortcuts) == 0 {
				continue
			}
			fmt.Printf("%s (%s):\n", dirName, alias)
			for _, s := range dir.Shortcuts {
				fmt.Printf("  %s — %s\n", s.Cmd, s.Desc)
			}
			for _, svcName := range dir.ServiceOrder {
				svc := dir.Services[svcName]
				if svc == nil {
					continue
				}
				for _, s := range svc.Shortcuts {
					fmt.Printf("  %s — %s [%s]\n", s.Cmd, s.Desc, svcName)
				}
			}
		}
	},
}

func init() {
	runCmd.Flags().StringP("branch", "b", "", "Workspace branch (default: main)")
	runCmd.AddCommand(runListCmd)

	// Also allow: tncli shortcuts (alias)
	shortcuts := strings.Replace(runListCmd.UseLine(), "shortcuts", "shortcuts", 1)
	_ = shortcuts
}
