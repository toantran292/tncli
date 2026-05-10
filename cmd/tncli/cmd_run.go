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
	Use:   "run <command> [repo]",
	Short: "Run a shortcut command in a repo directory",
	Long: `Run a command in a repo's worktree directory. Sources the primary env_output file before execution.

Examples:
  tncli run "bundle exec rake db:migrate" api
  tncli run "RACK_ENV=test bundle exec rspec spec/models" api
  tncli run "npm test" client
  tncli run "bundle install"                  # run in first repo`,
	Args: cobra.MinimumNArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		shellCmd := args[0]
		configDir := filepath.Dir(configPath)
		branch, _ := cmd.Flags().GetString("branch")
		if branch == "" {
			branch = appConfig.GlobalDefaultBranch()
		}

		// Find repo dir
		var dirName string
		var workDir string
		if len(args) >= 2 {
			target := args[1]
			for dn, dir := range appConfig.Repos {
				if dn == target || dir.Alias == target {
					dirName = dn
					workDir = filepath.Join(configDir, "workspace--"+branch, dn)
					break
				}
			}
			if workDir == "" {
				return fmt.Errorf("repo '%s' not found", target)
			}
		} else {
			if len(appConfig.RepoOrder) > 0 {
				dirName = appConfig.RepoOrder[0]
				workDir = filepath.Join(configDir, "workspace--"+branch, dirName)
			}
		}

		if _, err := os.Stat(workDir); os.IsNotExist(err) {
			return fmt.Errorf("directory not found: %s", workDir)
		}

		// Regenerate env for this workspace
		services.RegenerateWorkspaceEnv(configDir, appConfig, branch)

		// Run dir's pre_start (e.g. source .env.local for Prisma)
		preStart := ""
		if dir := appConfig.Repos[dirName]; dir != nil && dir.PreStart != "" {
			preStart = dir.PreStart + " && "
		}

		fullCmd := fmt.Sprintf("cd '%s' && %s%s", workDir, preStart, shellCmd)
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
