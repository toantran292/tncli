package main

import (
	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/commands"
)

var statusGlobal bool

var statusCmd = &cobra.Command{
	Use:   "status",
	Short: "Show running services",
	RunE: func(cmd *cobra.Command, args []string) error {
		if statusGlobal {
			commands.StatusGlobal()
			return nil
		}
		if appConfig == nil {
			if err := loadConfig(); err != nil {
				commands.StatusGlobal()
				return nil
			}
		}
		commands.Status(appConfig)
		return nil
	},
}

func init() {
	statusCmd.Flags().BoolVarP(&statusGlobal, "global", "g", false, "Show all sessions across projects")
}
