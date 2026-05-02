package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/tui"
)

var uiCmd = &cobra.Command{
	Use:   "ui",
	Short: "Open interactive TUI",
	RunE: func(cmd *cobra.Command, args []string) error {
		return tui.Run()
	},
}
