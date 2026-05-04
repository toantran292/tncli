package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var attachCmd = &cobra.Command{
	Use:   "attach [target]",
	Short: "Attach to tmux session",
	Args:  cobra.MaximumNArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		target := ""
		if len(args) > 0 {
			target = args[0]
		}
		return commands.Attach(appConfig, target)
	},
}
