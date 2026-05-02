package main

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tui"
)

const version = "0.5.0"

var (
	appConfig  *config.Config
	configPath string
)

var noConfigCmds = map[string]bool{
	"ui": true, "update": true, "version": true, "completion": true, "popup": true, "help": true, "tncli": true,
}

var rootCmd = &cobra.Command{
	Use:   "tncli",
	Short: "tmux-based project launcher",
	Long:  "tncli — manage multi-repo dev environments with tmux, port allocation, and git worktrees.",
	PersistentPreRunE: func(cmd *cobra.Command, args []string) error {
		if noConfigCmds[cmd.Name()] {
			return nil
		}
		return loadConfig()
	},
	RunE: func(cmd *cobra.Command, args []string) error {
		return tui.Run()
	},
	SilenceUsage:  true,
	SilenceErrors: true,
}

func loadConfig() error {
	var err error
	configPath, err = config.FindConfig()
	if err != nil {
		return err
	}
	appConfig, err = config.Load(configPath)
	if err != nil {
		return err
	}
	services.InitNetwork(filepath.Dir(configPath), appConfig.Session, appConfig)
	return nil
}

func configDir() string {
	return filepath.Dir(configPath)
}

func execute() {
	rootCmd.AddCommand(uiCmd, startCmd, stopCmd, restartCmd, statusCmd)
	rootCmd.AddCommand(attachCmd, logsCmd, listCmd, updateCmd, setupCmd)
	rootCmd.AddCommand(workspaceCmd, dbCmd, popupCmd, versionCmd, completionCmd)

	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
