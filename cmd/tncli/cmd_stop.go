package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var stopCmd = &cobra.Command{
	Use:   "stop [target]",
	Short: "Stop service(s), no arg = stop all",
	Args:  cobra.MaximumNArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		target := ""
		if len(args) > 0 {
			target = args[0]
		}
		return commands.Stop(appConfig, configPath, target)
	},
}
