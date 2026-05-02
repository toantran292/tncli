package main

import (
	"fmt"

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
		switch popupType {
		case "input":
			return popup.RunInput()
		case "ws-select":
			return popup.RunWsSelect(popupData)
		case "confirm":
			return popup.RunConfirm()
		default:
			return fmt.Errorf("unknown popup type: %s", popupType)
		}
	},
}

func init() {
	popupCmd.Flags().String("type", "", "Popup type: input, ws-select, confirm")
	popupCmd.Flags().String("data", "", "Popup data")
}
