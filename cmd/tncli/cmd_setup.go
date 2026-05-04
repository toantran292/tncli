package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var setupCmd = &cobra.Command{
	Use:   "setup",
	Short: "Initial setup (gitignore, port pool)",
	RunE: func(cmd *cobra.Command, args []string) error {
		return commands.Setup(appConfig)
	},
}

var migrateCmd = &cobra.Command{
	Use:   "migrate",
	Short: "Migrate from old IP-based system to port allocation",
	RunE: func(cmd *cobra.Command, args []string) error {
		return commands.Migrate(appConfig, configPath)
	},
}
