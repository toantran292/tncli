package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var statusCmd = &cobra.Command{
	Use:   "status",
	Short: "Show running services",
	RunE: func(cmd *cobra.Command, args []string) error {
		commands.Status(appConfig)
		return nil
	},
}
