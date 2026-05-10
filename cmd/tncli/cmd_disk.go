package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var diskCmd = &cobra.Command{
	Use:   "disk",
	Short: "Show disk usage across all registered projects",
	RunE: func(cmd *cobra.Command, args []string) error {
		commands.Disk()
		return nil
	},
}
