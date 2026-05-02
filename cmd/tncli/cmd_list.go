package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var listCmd = &cobra.Command{
	Use:   "list",
	Short: "List all services and combinations",
	RunE: func(cmd *cobra.Command, args []string) error {
		commands.List(appConfig)
		return nil
	},
}
