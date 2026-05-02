package main

import (
	"fmt"
	"os"

	"github.com/toantran292/tncli/internal/commands"
	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/popup"
	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tui"
)

const version = "0.5.0"

func main() {
	if len(os.Args) < 2 {
		run(tui.Run)
		return
	}

	switch os.Args[1] {
	case "ui":
		run(tui.Run)
	case "start":
		requireArg(2, "target")
		withConfig(func(cfg *config.Config, p string) error { return commands.Start(cfg, p, os.Args[2]) })
	case "stop":
		target := ""
		if len(os.Args) > 2 {
			target = os.Args[2]
		}
		withConfig(func(cfg *config.Config, _ string) error { return commands.Stop(cfg, target) })
	case "restart":
		requireArg(2, "target")
		withConfig(func(cfg *config.Config, p string) error { return commands.Restart(cfg, p, os.Args[2]) })
	case "status":
		withConfig(func(cfg *config.Config, _ string) error { commands.Status(cfg); return nil })
	case "attach":
		target := ""
		if len(os.Args) > 2 {
			target = os.Args[2]
		}
		withConfig(func(cfg *config.Config, _ string) error { return commands.Attach(cfg, target) })
	case "logs":
		requireArg(2, "target")
		withConfig(func(cfg *config.Config, _ string) error { return commands.Logs(cfg, os.Args[2]) })
	case "list":
		withConfig(func(cfg *config.Config, _ string) error { commands.List(cfg); return nil })
	case "update":
		run(commands.Update)
	case "setup":
		withConfig(func(cfg *config.Config, _ string) error { return commands.Setup(cfg) })
	case "workspace":
		requireArg(2, "subcommand")
		switch os.Args[2] {
		case "create":
			requireArg(4, "workspace branch")
			ws, branch := os.Args[3], os.Args[4]
			if err := services.ValidateBranchName(branch); err != nil {
				fatal("invalid branch: %v", err)
			}
			fromStage, repos := parseCreateFlags()
			withConfig(func(cfg *config.Config, p string) error {
				return commands.WorkspaceCreate(cfg, p, ws, branch, fromStage, repos)
			})
		case "delete":
			requireArg(3, "branch")
			if err := services.ValidateBranchName(os.Args[3]); err != nil {
				fatal("invalid branch: %v", err)
			}
			withConfig(func(cfg *config.Config, p string) error {
				return commands.WorkspaceDelete(cfg, p, os.Args[3])
			})
		case "list":
			withConfig(func(cfg *config.Config, p string) error { commands.WorkspaceList(cfg, p); return nil })
		default:
			fatal("unknown workspace subcommand: %s", os.Args[2])
		}
	case "db":
		requireArg(2, "subcommand")
		if os.Args[2] == "reset" {
			requireArg(3, "branch")
			withConfig(func(cfg *config.Config, _ string) error { return commands.DBReset(cfg, os.Args[3]) })
		}
	case "proxy":
		requireArg(2, "subcommand")
		switch os.Args[2] {
		case "serve":
			run(services.RunProxyServer)
		case "start":
			run(commands.ProxyStart)
		case "stop":
			commands.ProxyStop()
		case "restart":
			run(commands.ProxyRestart)
		case "status":
			commands.ProxyStatus()
		case "install":
			run(commands.ProxyInstall)
		case "uninstall":
			commands.ProxyUninstall()
		default:
			fatal("unknown proxy subcommand: %s", os.Args[2])
		}
	case "popup":
		handlePopup()
	case "--version", "-v", "version":
		fmt.Printf("tncli v%s\n", version)
	case "--help", "-h", "help":
		printUsage()
	default:
		fmt.Fprintf(os.Stderr, "unknown command: %s\n", os.Args[1])
		printUsage()
		os.Exit(1)
	}
}

// ── Helpers ──

func run(fn func() error) {
	if err := fn(); err != nil {
		fatal("%v", err)
	}
}

func withConfig(fn func(*config.Config, string) error) {
	cfgPath, err := config.FindConfig()
	if err != nil {
		fatal("%v", err)
	}
	cfg, err := config.Load(cfgPath)
	if err != nil {
		fatal("%v", err)
	}
	if err := fn(cfg, cfgPath); err != nil {
		fatal("%v", err)
	}
}

func requireArg(n int, name string) {
	if len(os.Args) <= n {
		fatal("missing argument: %s", name)
	}
}

func fatal(format string, args ...interface{}) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}

func parseCreateFlags() (fromStage int, repos string) {
	for i := 5; i < len(os.Args); i++ {
		if os.Args[i] == "--from-stage" && i+1 < len(os.Args) {
			fmt.Sscanf(os.Args[i+1], "%d", &fromStage)
			i++
		}
		if os.Args[i] == "--repos" && i+1 < len(os.Args) {
			repos = os.Args[i+1]
			i++
		}
	}
	return
}

func handlePopup() {
	popupType, popupData := "", ""
	for i := 2; i < len(os.Args); i++ {
		if os.Args[i] == "--type" && i+1 < len(os.Args) {
			popupType = os.Args[i+1]
			i++
		}
		if os.Args[i] == "--data" && i+1 < len(os.Args) {
			popupData = os.Args[i+1]
			i++
		}
	}
	var err error
	switch popupType {
	case "input":
		err = popup.RunInput()
	case "ws-select":
		err = popup.RunWsSelect(popupData)
	case "confirm":
		err = popup.RunConfirm()
	default:
		fatal("unknown popup type: %s", popupType)
	}
	if err != nil {
		fatal("%v", err)
	}
}

func printUsage() {
	fmt.Printf(`tncli — tmux-based project launcher (v%s)

Usage: tncli <command> [args]

Commands:
  ui              Open interactive TUI (default)
  start <target>  Start a service or combination
  stop [target]   Stop service(s), no arg = stop all
  restart <target> Restart a service or combination
  status          Show running services
  attach [target] Attach to tmux session
  logs <target>   Show recent output of a service
  list            List all services and combinations
  update          Update tncli to latest release
  setup           Setup loopback IPs and /etc/hosts (requires sudo)

  workspace create <ws> <branch> [--from-stage N] [--repos r1:b1,r2:b2]
  workspace delete <branch>
  workspace list

  db reset <branch>

  proxy start|stop|restart|status|install|uninstall
`, version)
}
