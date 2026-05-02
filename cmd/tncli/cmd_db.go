package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var dbCmd = &cobra.Command{
	Use:   "db",
	Short: "Database management",
}

var dbResetCmd = &cobra.Command{
	Use:   "reset <branch>",
	Short: "Drop and recreate databases for a workspace",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		return commands.DBReset(appConfig, args[0])
	},
}

func init() {
	dbCmd.AddCommand(dbResetCmd)
}
