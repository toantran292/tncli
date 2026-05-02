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
		return commands.WorkspaceCreate(appConfig, configPath, ws, branch, fromStage, repos)
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

func init() {
	wsCreateCmd.Flags().Int("from-stage", 0, "Resume from stage N (1-based)")
	wsCreateCmd.Flags().String("repos", "", "Selected repos: repo1:branch1,repo2:branch2")

	workspaceCmd.AddCommand(wsCreateCmd)
	workspaceCmd.AddCommand(wsDeleteCmd)
	workspaceCmd.AddCommand(wsListCmd)
}
