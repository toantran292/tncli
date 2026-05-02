package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var startCmd = &cobra.Command{
	Use:   "start <target>",
	Short: "Start a service or combination",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		return commands.Start(appConfig, configPath, args[0])
	},
}
