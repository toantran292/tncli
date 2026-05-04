package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var logsCmd = &cobra.Command{
	Use:   "logs <target>",
	Short: "Show recent output of a service",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		return commands.Logs(appConfig, args[0])
	},
}
