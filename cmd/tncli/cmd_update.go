package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var updateCmd = &cobra.Command{
	Use:   "update",
	Short: "Update tncli to latest release",
	RunE: func(cmd *cobra.Command, args []string) error {
		return commands.Update()
	},
}
