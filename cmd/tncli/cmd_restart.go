package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var restartCmd = &cobra.Command{
	Use:   "restart <target>",
	Short: "Restart a service or combination",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		return commands.Restart(appConfig, configPath, args[0])
	},
}
