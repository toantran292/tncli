package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
	"github.com/toantran292/tncli/internal/services"
)

var workspaceCmd = &cobra.Command{
	Use:   "workspace",
	Short: "Manage workspaces",
}

var wsCreateCmd = &cobra.Command{
	Use:   "create <workspace> <branch>",
	Short: "Create workspace (worktrees for all dirs)",
	Args:  cobra.ExactArgs(2),
	RunE: func(cmd *cobra.Command, args []string) error {
		ws, branch := args[0], args[1]
		if err := services.ValidateBranchName(branch); err != nil {
			return err
		}
		fromStage, _ := cmd.Flags().GetInt("from-stage")
		repos, _ := cmd.Flags().GetString("repos")
		envName, _ := cmd.Flags().GetString("env")
		if envName != "" {
			if err := appConfig.ValidateEnvironment(envName); err != nil {
				return err
			}
		}
		return commands.WorkspaceCreate(appConfig, configPath, ws, branch, fromStage, repos, envName)
	},
}

var wsDeleteCmd = &cobra.Command{
	Use:   "delete <branch>",
	Short: "Delete workspace",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		if err := services.ValidateBranchName(args[0]); err != nil {
			return err
		}
		return commands.WorkspaceDelete(appConfig, configPath, args[0])
	},
}

var wsListCmd = &cobra.Command{
	Use:   "list",
	Short: "List active workspaces",
	RunE: func(cmd *cobra.Command, args []string) error {
		commands.WorkspaceList(appConfig, configPath)
		return nil
	},
}

var wsEnvCmd = &cobra.Command{
	Use:   "env <branch> <repo> [environment]",
	Short: "Show or set per-repo environment",
	Args:  cobra.RangeArgs(1, 3),
	RunE: func(cmd *cobra.Command, args []string) error {
		branch := args[0]
		if len(args) == 1 {
			commands.WorkspaceShowEnv(appConfig, configPath, branch)
			return nil
		}
		repo := args[1]
		clear, _ := cmd.Flags().GetBool("clear")
		if clear {
			return commands.WorkspaceSetEnv(appConfig, configPath, branch, repo, "")
		}
		if len(args) == 3 {
			return commands.WorkspaceSetEnv(appConfig, configPath, branch, repo, args[2])
		}
		commands.WorkspaceShowEnv(appConfig, configPath, branch)
		return nil
	},
}

func init() {
	wsCreateCmd.Flags().Int("from-stage", 0, "Resume from stage N (1-based)")
	wsCreateCmd.Flags().String("repos", "", "Selected repos: repo1:branch1,repo2:branch2")
	wsCreateCmd.Flags().String("env", "", "Environment (e.g., staging, sandbox)")

	wsEnvCmd.Flags().Bool("clear", false, "Clear environment (use local)")

	workspaceCmd.AddCommand(wsCreateCmd)
	workspaceCmd.AddCommand(wsDeleteCmd)
	workspaceCmd.AddCommand(wsListCmd)
	workspaceCmd.AddCommand(wsEnvCmd)
}
