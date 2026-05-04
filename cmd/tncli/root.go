package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"
	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/paths"
	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tui"
)

const version = "0.5.0"

var (
	appConfig  *config.Config
	configPath string
)

var noConfigCmds = map[string]bool{
	"ui": true, "update": true, "version": true, "completion": true, "popup": true, "help": true, "tncli": true, "status": true,
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
		if appConfig == nil {
			if err := loadConfig(); err != nil {
				return selectAndLaunchProject()
			}
		}
		return tui.Run()
	},
	SilenceUsage:  true,
	SilenceErrors: true,
}

func selectAndLaunchProject() error {
	projects := services.ListProjects()
	if len(projects) == 0 {
		return fmt.Errorf("no tncli.yml found and no registered projects\nCreate tncli.yml in your project root, then run tncli")
	}

	// Build selection list
	var names []string
	for name, dir := range projects {
		names = append(names, fmt.Sprintf("%s\t%s", name, dir))
	}

	if len(names) == 1 {
		// Only one project — go directly
		for _, dir := range projects {
			os.Chdir(dir)
			return loadConfig()
		}
	}

	// Use fzf if available
	if _, err := exec.LookPath("fzf"); err == nil {
		cmd := exec.Command("bash", "-c",
			fmt.Sprintf("printf '%s' | fzf --prompt='Select project> ' --delimiter='\t' --with-nth=1 | cut -f2",
				strings.Join(names, "\\n")))
		cmd.Stdin = os.Stdin
		cmd.Stderr = os.Stderr
		out, err := cmd.Output()
		if err != nil || strings.TrimSpace(string(out)) == "" {
			return fmt.Errorf("no project selected")
		}
		dir := strings.TrimSpace(string(out))
		os.Chdir(dir)
		if err := loadConfig(); err != nil {
			return err
		}
		return tui.Run()
	}

	// Fallback: list and ask
	fmt.Println("Registered projects:")
	i := 0
	var dirs []string
	for name, dir := range projects {
		fmt.Printf("  [%d] %s — %s\n", i, name, dir)
		dirs = append(dirs, dir)
		i++
	}
	fmt.Print("\nSelect [0]: ")
	var choice int
	fmt.Scanln(&choice)
	if choice < 0 || choice >= len(dirs) {
		choice = 0
	}
	os.Chdir(dirs[choice])
	if err := loadConfig(); err != nil {
		return err
	}
	return tui.Run()
}

func loadConfig() error {
	checkDeps()
	paths.MigrateFromLegacy()
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

func checkDeps() {
	required := []struct{ name, install string }{
		{"tmux", "brew install tmux"},
		{"docker", "https://docs.docker.com/get-docker/"},
		{"git", "brew install git"},
		{"zsh", "brew install zsh"},
	}
	for _, dep := range required {
		if _, err := exec.LookPath(dep.name); err != nil {
			fmt.Fprintf(os.Stderr, "required: %s not found — install: %s\n", dep.name, dep.install)
			os.Exit(1)
		}
	}
}

func configDir() string {
	return filepath.Dir(configPath)
}

func execute() {
	rootCmd.AddCommand(uiCmd, startCmd, stopCmd, restartCmd, statusCmd)
	rootCmd.AddCommand(attachCmd, logsCmd, listCmd, updateCmd, setupCmd)
	rootCmd.AddCommand(workspaceCmd, dbCmd, popupCmd, versionCmd, completionCmd, migrateCmd, runCmd)

	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
