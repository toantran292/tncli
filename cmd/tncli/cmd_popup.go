package main

import (
	"fmt"
	"os"

	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/popup"
)

var popupCmd = &cobra.Command{
	Use:    "popup",
	Short:  "Internal popup dialogs",
	Hidden: true,
	RunE: func(cmd *cobra.Command, args []string) error {
		popupType, _ := cmd.Flags().GetString("type")
		popupData, _ := cmd.Flags().GetString("data")
		if dataFile, _ := cmd.Flags().GetString("data-file"); dataFile != "" {
			raw, err := os.ReadFile(dataFile)
			if err != nil {
				return fmt.Errorf("read data file: %w", err)
			}
			popupData = string(raw)
		}
		switch popupType {
		case "input":
			return popup.RunInput()
		case "ws-select":
			return popup.RunWsSelect(popupData)
		case "confirm":
			return popup.RunConfirm()
		case "cheatsheet":
			return popup.RunCheatsheet()
		case "list":
			return popup.RunList(popupData)
		default:
			return fmt.Errorf("unknown popup type: %s", popupType)
		}
	},
}

func init() {
	popupCmd.Flags().String("type", "", "Popup type: input, ws-select, confirm, list")
	popupCmd.Flags().String("data", "", "Popup data (inline)")
	popupCmd.Flags().String("data-file", "", "Popup data (from file)")
}
