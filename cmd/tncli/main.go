package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/lock"
	"github.com/toantran292/tncli/internal/pipeline"
	"github.com/toantran292/tncli/internal/popup"
	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tmux"
	"github.com/toantran292/tncli/internal/tui"
)

const version = "0.5.0"

const (
	green  = "\033[0;32m"
	yellow = "\033[0;33m"
	blue   = "\033[0;34m"
	cyan   = "\033[0;36m"
	bold   = "\033[1m"
	dim    = "\033[2m"
	nc     = "\033[0m"
)

func main() {
	if len(os.Args) < 2 {
		runUI()
		return
	}

	switch os.Args[1] {
	case "ui":
		runUI()
	case "start":
		requireArg(2, "target")
		withConfig(func(cfg *config.Config, cfgPath string) { cmdStart(cfg, cfgPath, os.Args[2]) })
	case "stop":
		target := ""
		if len(os.Args) > 2 {
			target = os.Args[2]
		}
		withConfig(func(cfg *config.Config, cfgPath string) { cmdStop(cfg, target) })
	case "restart":
		requireArg(2, "target")
		withConfig(func(cfg *config.Config, cfgPath string) { cmdRestart(cfg, cfgPath, os.Args[2]) })
	case "status":
		withConfig(func(cfg *config.Config, _ string) { cmdStatus(cfg) })
	case "attach":
		target := ""
		if len(os.Args) > 2 {
			target = os.Args[2]
		}
		withConfig(func(cfg *config.Config, _ string) { cmdAttach(cfg, target) })
	case "logs":
		requireArg(2, "target")
		withConfig(func(cfg *config.Config, _ string) { cmdLogs(cfg, os.Args[2]) })
	case "list":
		withConfig(func(cfg *config.Config, _ string) { cmdList(cfg) })
	case "update":
		cmdUpdate()
	case "setup":
		withConfig(func(cfg *config.Config, _ string) { cmdSetup(cfg) })
	case "workspace":
		requireArg(2, "subcommand")
		switch os.Args[2] {
		case "create":
			requireArg(4, "workspace branch")
			ws, branch := os.Args[3], os.Args[4]
			if err := services.ValidateBranchName(branch); err != nil {
				fatal("invalid branch: %v", err)
			}
			var fromStage int
			var repos string
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
			withConfig(func(cfg *config.Config, cfgPath string) {
				cmdWorkspaceCreate(cfg, cfgPath, ws, branch, fromStage, repos)
			})
		case "delete":
			requireArg(3, "branch")
			if err := services.ValidateBranchName(os.Args[3]); err != nil {
				fatal("invalid branch: %v", err)
			}
			withConfig(func(cfg *config.Config, cfgPath string) {
				cmdWorkspaceDelete(cfg, cfgPath, os.Args[3])
			})
		case "list":
			withConfig(func(cfg *config.Config, cfgPath string) { cmdWorkspaceList(cfg, cfgPath) })
		default:
			fatal("unknown workspace subcommand: %s", os.Args[2])
		}
	case "db":
		requireArg(2, "subcommand")
		if os.Args[2] == "reset" {
			requireArg(3, "branch")
			withConfig(func(cfg *config.Config, _ string) { cmdDBReset(cfg, os.Args[3]) })
		}
	case "proxy":
		requireArg(2, "subcommand")
		switch os.Args[2] {
		case "serve":
			if err := services.RunProxyServer(); err != nil {
				fatal("%v", err)
			}
		case "start":
			cmdProxyStart()
		case "stop":
			cmdProxyStop()
		case "restart":
			cmdProxyRestart()
		case "status":
			cmdProxyStatus()
		case "install":
			cmdProxyInstall()
		case "uninstall":
			cmdProxyUninstall()
		default:
			fatal("unknown proxy subcommand: %s", os.Args[2])
		}
	case "popup":
		popupType := ""
		popupData := ""
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
		switch popupType {
		case "input":
			if err := popup.RunInput(); err != nil {
				fatal("%v", err)
			}
		case "ws-select":
			if err := popup.RunWsSelect(popupData); err != nil {
				fatal("%v", err)
			}
		case "confirm":
			if err := popup.RunConfirm(); err != nil {
				fatal("%v", err)
			}
		default:
			fatal("unknown popup type: %s", popupType)
		}
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

func printUsage() {
	fmt.Printf(`%stncli%s — tmux-based project launcher (v%s)

%sUsage:%s tncli <command> [args]

%sCommands:%s
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
`, bold, nc, version, bold, nc, bold, nc)
}

// ── Helpers ──

func requireArg(n int, name string) {
	if len(os.Args) <= n {
		fatal("missing argument: %s", name)
	}
}

func fatal(format string, args ...interface{}) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}

func withConfig(fn func(*config.Config, string)) {
	cfgPath, err := config.FindConfig()
	if err != nil {
		fatal("%v", err)
	}
	cfg, err := config.Load(cfgPath)
	if err != nil {
		fatal("%v", err)
	}
	fn(cfg, cfgPath)
}

// ── Commands ──

func runUI() {
	if err := tui.Run(); err != nil {
		fatal("%v", err)
	}
}

func cmdStart(cfg *config.Config, cfgPath, target string) {
	pairs, err := cfg.ResolveServices(target)
	if err != nil {
		fatal("%v", err)
	}
	configDir := filepath.Dir(cfgPath)

	lock.EnsureDir()
	createdSession := tmux.CreateSessionIfNeeded(cfg.SvcSession())
	started, skipped := 0, 0

	for _, pair := range pairs {
		dirName, svcName := pair[0], pair[1]
		if tmux.WindowExists(cfg.SvcSession(), svcName) {
			fmt.Fprintf(os.Stderr, "%swarning:%s '%s' is already running — skipping\n", yellow, nc, svcName)
			skipped++
			continue
		}

		resolved, err := cfg.ResolveService(configDir, dirName, svcName)
		if err != nil {
			fatal("%v", err)
		}

		var fullCmd strings.Builder
		if resolved.Env != "" {
			fullCmd.WriteString(resolved.Env + " ")
		}
		fmt.Fprintf(&fullCmd, "cd '%s'", resolved.WorkDir)
		if resolved.PreStart != "" {
			fullCmd.WriteString(" && " + resolved.PreStart)
		}
		fullCmd.WriteString(" && " + resolved.Cmd)

		tmux.NewWindow(cfg.SvcSession(), svcName, fullCmd.String())
		lock.Acquire(cfg.SvcSession(), svcName)
		fmt.Printf("%s>>>%s started %s%s%s (%s%s%s)\n", green, nc, bold, svcName, nc, dim, dirName, nc)
		started++
	}

	if createdSession {
		tmux.CleanupInitWindow(cfg.SvcSession())
	}
	if started > 0 {
		fmt.Printf("\n%s%d service(s) started%s in session %s%s%s\n", green, started, nc, cyan, cfg.Session, nc)
		fmt.Printf("%sattach: tncli attach%s\n", dim, nc)
	}
	if skipped > 0 {
		fmt.Printf("%s%d service(s) skipped (already running)%s\n", yellow, skipped, nc)
	}
}

func cmdStop(cfg *config.Config, target string) {
	lock.EnsureDir()

	if target == "" {
		if tmux.SessionExists(cfg.SvcSession()) {
			tmux.KillSession(cfg.SvcSession())
			lock.ReleaseAll(cfg.SvcSession())
			fmt.Printf("%s>>>%s stopped all services (session %s%s%s killed)\n", green, nc, cyan, cfg.Session, nc)
		} else {
			fmt.Printf("%s>>>%s no running session '%s'\n", blue, nc, cfg.Session)
		}
		return
	}

	pairs, err := cfg.ResolveServices(target)
	if err != nil {
		fatal("%v", err)
	}
	stopped := 0
	for _, pair := range pairs {
		svcName := pair[1]
		if tmux.WindowExists(cfg.SvcSession(), svcName) {
			tmux.GracefulStop(cfg.SvcSession(), svcName)
			lock.Release(cfg.SvcSession(), svcName)
			fmt.Printf("%s>>>%s stopped %s%s%s\n", green, nc, bold, svcName, nc)
			stopped++
		} else {
			fmt.Fprintf(os.Stderr, "%swarning:%s '%s' is not running\n", yellow, nc, svcName)
		}
	}

	if !tmux.SessionExists(cfg.SvcSession()) {
		lock.ReleaseAll(cfg.SvcSession())
	} else {
		windows := tmux.ListWindows(cfg.SvcSession())
		if len(windows) == 0 {
			tmux.KillSession(cfg.SvcSession())
			lock.ReleaseAll(cfg.SvcSession())
		}
	}
	fmt.Printf("%s%d service(s) stopped%s\n", green, stopped, nc)
}

func cmdRestart(cfg *config.Config, cfgPath, target string) {
	cmdStop(cfg, target)
	cmdStart(cfg, cfgPath, target)
}

func cmdStatus(cfg *config.Config) {
	if !tmux.SessionExists(cfg.SvcSession()) {
		fmt.Printf("%sno active session '%s'%s\n", dim, cfg.Session, nc)
		return
	}
	fmt.Printf("%sSession:%s %s%s%s\n\n", bold, nc, cyan, cfg.Session, nc)
	windows := tmux.ListWindows(cfg.SvcSession())
	for _, dirName := range cfg.RepoOrder {
		dir := cfg.Repos[dirName]
		fmt.Printf("%s%s%s\n", bold, dirName, nc)
		for _, svcName := range dir.ServiceOrder {
			if windows[svcName] {
				fmt.Printf("  %s●%s %s\n", green, nc, svcName)
			} else {
				fmt.Printf("  %s○ %s%s\n", dim, svcName, nc)
			}
		}
	}
	fmt.Printf("\n%sattach: tncli attach%s\n", dim, nc)
}

func cmdAttach(cfg *config.Config, target string) {
	if !tmux.SessionExists(cfg.SvcSession()) {
		fatal("no active session '%s'", cfg.Session)
	}
	if err := tmux.Attach(cfg.SvcSession(), target); err != nil {
		fatal("tmux attach failed")
	}
}

func cmdLogs(cfg *config.Config, target string) {
	if !tmux.WindowExists(cfg.SvcSession(), target) {
		fatal("service '%s' is not running", target)
	}
	for _, line := range tmux.CapturePane(cfg.SvcSession(), target, 100) {
		fmt.Println(line)
	}
}

func cmdList(cfg *config.Config) {
	fmt.Printf("%sServices:%s\n", bold, nc)
	for _, dirName := range cfg.RepoOrder {
		dir := cfg.Repos[dirName]
		alias := ""
		if dir.Alias != "" {
			alias = " (" + dir.Alias + ")"
		}
		fmt.Printf("  %s%s%s%s\n", bold, dirName, alias, nc)
		for _, svcName := range dir.ServiceOrder {
			svc := dir.Services[svcName]
			cmd := "n/a"
			if svc.Cmd != "" {
				cmd = svc.Cmd
			}
			fmt.Printf("    %s: %s\n", svcName, cmd)
		}
	}

	workspaces := cfg.AllWorkspaces()
	if len(workspaces) > 0 {
		fmt.Printf("\n%sWorkspaces:%s\n", bold, nc)
		for name, entries := range workspaces {
			fmt.Printf("  %s: %s\n", name, strings.Join(entries, ", "))
		}
	}
}

func cmdWorkspaceCreate(cfg *config.Config, cfgPath, workspace, branch string, fromStage int, repos string) {
	services.MigrateLegacyIPs()

	skipStages := make(map[int]bool)
	if fromStage > 1 {
		for i := 0; i < fromStage-1; i++ {
			skipStages[i] = true
		}
	}

	var selectedDirs [][2]string
	if repos != "" {
		for _, entry := range strings.Split(repos, ",") {
			parts := strings.SplitN(entry, ":", 2)
			if len(parts) == 2 {
				selectedDirs = append(selectedDirs, [2]string{parts[0], parts[1]})
			} else {
				selectedDirs = append(selectedDirs, [2]string{parts[0], branch})
			}
		}
	}

	var ctx *pipeline.CreateContext
	var err error
	if len(selectedDirs) > 0 {
		ctx, err = pipeline.FromConfigWithSelection(cfg, cfgPath, workspace, branch, selectedDirs)
	} else {
		ctx, err = pipeline.FromConfig(cfg, cfgPath, workspace, branch, skipStages)
	}
	if err != nil {
		fatal("%v", err)
	}

	ch := make(chan pipeline.Event, 16)
	go pipeline.RunCreatePipeline(ctx, ch)

	for evt := range ch {
		switch evt.Type {
		case pipeline.EventStageStarted:
			fmt.Printf("%s>>>%s [%d/%d] %s\n", blue, nc, evt.Index+1, evt.Total, evt.Name)
		case pipeline.EventStageCompleted:
			fmt.Printf("    %sdone%s\n", green, nc)
		case pipeline.EventStageSkipped:
			label := pipeline.AllCreateStages[evt.Index].Label()
			fmt.Printf("%s    skipped: %s%s\n", dim, label, nc)
		case pipeline.EventPipelineCompleted:
			configDir := filepath.Dir(cfgPath)
			fmt.Printf("\n%sWorkspace ready:%s BIND_IP=%s\n", green, nc, ctx.BindIP)
			fmt.Printf("  cd %s/workspace--%s\n", configDir, branch)
			return
		case pipeline.EventPipelineFailed:
			fmt.Fprintf(os.Stderr, "\n%sFailed at stage %d:%s %s\n", yellow, evt.Index+1, nc, evt.Error)
			fmt.Fprintf(os.Stderr, "%sRetry: tncli workspace create %s %s --from-stage %d%s\n", dim, workspace, branch, evt.Index+1, nc)
			os.Exit(1)
		}
	}
}

func cmdWorkspaceDelete(cfg *config.Config, cfgPath, branch string) {
	configDir := filepath.Dir(cfgPath)
	branchSafe := services.BranchSafe(branch)

	var cleanupItems []pipeline.CleanupItem
	var dbsToDrop []pipeline.DBDropItem

	for dirName, dir := range cfg.Repos {
		dirPath := dirName
		if !filepath.IsAbs(dirName) {
			defaultBranch := cfg.GlobalDefaultBranch()
			wsPath := filepath.Join(configDir, "workspace--"+defaultBranch, dirName)
			if info, err := os.Stat(wsPath); err == nil && info.IsDir() {
				dirPath = wsPath
			} else {
				dirPath = filepath.Join(configDir, dirName)
			}
		}

		wsFolder := filepath.Join(configDir, "workspace--"+branch)
		wtPath := filepath.Join(wsFolder, dirName)
		if _, err := os.Stat(wtPath); os.IsNotExist(err) {
			continue
		}

		var preDelete []string
		if dir.WT() != nil {
			preDelete = dir.WT().PreDelete
		}
		cleanupItems = append(cleanupItems, pipeline.CleanupItem{
			DirPath: dirPath, WtPath: wtPath, WtBranch: branch, PreDelete: preDelete,
		})

		if dir.WT() != nil {
			pgSvc := findPGService(cfg)
			pgHost := cfg.SharedHost("postgres")
			pgPort := uint16(5432)
			pgUser := "postgres"
			pgPw := "postgres"
			if pgSvc != nil {
				pgPort = services.FirstPortFromList(pgSvc.Ports)
				if pgPort == 0 {
					pgPort = 5432
				}
				if pgSvc.DBUser != "" {
					pgUser = pgSvc.DBUser
				}
				if pgSvc.DBPassword != "" {
					pgPw = pgSvc.DBPassword
				}
			}

			for _, sref := range dir.WT().SharedServices {
				if sref.DBName != "" {
					dbName := strings.ReplaceAll(sref.DBName, "{{branch_safe}}", branchSafe)
					dbName = strings.ReplaceAll(dbName, "{{branch}}", branch)
					dbsToDrop = append(dbsToDrop, pipeline.DBDropItem{
						Host: pgHost, Port: pgPort, DBName: dbName, User: pgUser, Password: pgPw,
					})
				}
			}
			for _, dbTpl := range dir.WT().Databases {
				dbName := strings.ReplaceAll(dbTpl, "{{branch_safe}}", branchSafe)
				dbName = strings.ReplaceAll(dbName, "{{branch}}", branch)
				dbsToDrop = append(dbsToDrop, pipeline.DBDropItem{
					Host: pgHost, Port: pgPort, DBName: cfg.Session + "_" + dbName, User: pgUser, Password: pgPw,
				})
			}
		}
	}

	ctx := &pipeline.DeleteContext{
		Branch:       branch,
		Config:       cfg,
		ConfigDir:    configDir,
		CleanupItems: cleanupItems,
		DBsToDrop:    dbsToDrop,
		Network:      "tncli-ws-" + branch,
	}

	ch := make(chan pipeline.Event, 16)
	go pipeline.RunDeletePipeline(ctx, ch)

	for evt := range ch {
		switch evt.Type {
		case pipeline.EventStageStarted:
			fmt.Printf("%s>>>%s [%d/%d] %s\n", blue, nc, evt.Index+1, evt.Total, evt.Name)
		case pipeline.EventStageCompleted:
			fmt.Printf("    %sdone%s\n", green, nc)
		case pipeline.EventPipelineCompleted:
			fmt.Printf("\n%sWorkspace '%s' deleted%s\n", green, branch, nc)
			return
		case pipeline.EventPipelineFailed:
			fmt.Fprintf(os.Stderr, "\n%sDelete failed at stage %d:%s %s\n", yellow, evt.Index+1, nc, evt.Error)
			os.Exit(1)
		}
	}
}

func cmdWorkspaceList(cfg *config.Config, cfgPath string) {
	workspaces := cfg.AllWorkspaces()
	configDir := filepath.Dir(cfgPath)
	ipAllocs := services.LoadIPAllocations()

	fmt.Printf("%sWorkspace definitions:%s\n", bold, nc)
	for name, entries := range workspaces {
		fmt.Printf("  %s%s%s: %s\n", bold, name, nc, strings.Join(entries, ", "))
	}

	var wsBranches []string
	entries, _ := os.ReadDir(configDir)
	for _, e := range entries {
		if branch, ok := strings.CutPrefix(e.Name(), "workspace--"); ok {
			wsBranches = append(wsBranches, branch)
		}
	}

	if len(wsBranches) == 0 {
		fmt.Printf("\n%sNo active workspace instances%s\n", dim, nc)
		return
	}

	for _, branch := range wsBranches {
		wsKey := "ws-" + branch
		ip := ipAllocs[wsKey]
		if ip == "" {
			ip = "?"
		}
		fmt.Printf("\n%sWorkspace: %s%s%s %s(%s)%s\n", green, bold, branch, nc, dim, ip, nc)

		wsFolder := filepath.Join(configDir, "workspace--"+branch)
		for _, dirName := range cfg.RepoOrder {
			dir := cfg.Repos[dirName]
			wtDir := filepath.Join(wsFolder, dirName)
			if _, err := os.Stat(wtDir); os.IsNotExist(err) {
				continue
			}
			alias := ""
			if dir.Alias != "" {
				alias = " (" + dir.Alias + ")"
			}
			fmt.Printf("  %s%s%s%s\n", bold, dirName, alias, nc)
			for _, svcName := range dir.ServiceOrder {
				svc := dir.Services[svcName]
				cmd := "n/a"
				if svc.Cmd != "" {
					cmd = svc.Cmd
				}
				if p := services.ExtractPortFromCmd(cmd); p > 0 {
					fmt.Printf("    %s%s%s → %s:%d  %s%s%s\n", cyan, svcName, nc, ip, p, dim, cmd, nc)
				} else {
					fmt.Printf("    %s%s%s  %s%s%s\n", cyan, svcName, nc, dim, cmd, nc)
				}
			}
		}
	}

	if len(cfg.SharedServices) > 0 {
		fmt.Printf("\n%sShared services:%s\n", bold, nc)
		for name, svc := range cfg.SharedServices {
			host := svc.Host
			if host == "" {
				host = "localhost"
			}
			fmt.Printf("  %s%s%s: %s [%s] %s(%s)%s\n", cyan, name, nc, host, strings.Join(svc.Ports, ", "), dim, svc.Image, nc)
		}
	}
}

func cmdDBReset(cfg *config.Config, workspaceBranch string) {
	cfgPath, _ := config.FindConfig()
	configDir := filepath.Dir(cfgPath)

	type dbEntry struct {
		repo, dbName string
		port         uint16
		user, pw     string
	}
	var dbs []dbEntry

	for dirName, dir := range cfg.Repos {
		wt := dir.WT()
		if wt == nil {
			continue
		}

		repoBranch := workspaceBranch
		if workspaceBranch == cfg.GlobalDefaultBranch() {
			repoBranch = cfg.DefaultBranchFor(dirName)
		} else {
			wsDir := filepath.Join(configDir, "workspace--"+workspaceBranch, dirName)
			if b := services.CurrentBranch(wsDir); b != "" {
				repoBranch = b
			}
		}

		branchSafe := services.BranchSafe(repoBranch)
		pgSvc := findPGService(cfg)
		pgPort := uint16(5432)
		pgUser, pgPw := "postgres", "postgres"
		if pgSvc != nil {
			pgPort = services.FirstPortFromList(pgSvc.Ports)
			if pgPort == 0 {
				pgPort = 5432
			}
			if pgSvc.DBUser != "" {
				pgUser = pgSvc.DBUser
			}
			if pgSvc.DBPassword != "" {
				pgPw = pgSvc.DBPassword
			}
		}

		for _, sref := range wt.SharedServices {
			if sref.DBName != "" {
				dbName := strings.ReplaceAll(sref.DBName, "{{branch_safe}}", branchSafe)
				dbName = strings.ReplaceAll(dbName, "{{branch}}", repoBranch)
				dbs = append(dbs, dbEntry{dirName, dbName, pgPort, pgUser, pgPw})
			}
		}
		for _, dbTpl := range wt.Databases {
			dbName := strings.ReplaceAll(dbTpl, "{{branch_safe}}", branchSafe)
			dbName = strings.ReplaceAll(dbName, "{{branch}}", repoBranch)
			dbs = append(dbs, dbEntry{dirName, cfg.Session + "_" + dbName, pgPort, pgUser, pgPw})
		}
	}

	if len(dbs) == 0 {
		fmt.Printf("%sNo databases found for workspace '%s'%s\n", yellow, workspaceBranch, nc)
		return
	}

	fmt.Printf("%sResetting databases for workspace '%s':%s\n", bold, workspaceBranch, nc)
	for _, db := range dbs {
		fmt.Printf("  %s: %s\n", db.repo, db.dbName)
	}
	fmt.Println()

	var dbNames []string
	for _, db := range dbs {
		dbNames = append(dbNames, db.dbName)
	}

	host := "localhost"
	if pgSvc := findPGService(cfg); pgSvc != nil && pgSvc.Host != "" {
		host = pgSvc.Host
	}
	port := dbs[0].port
	user := dbs[0].user
	pw := dbs[0].pw

	fmt.Printf("%s>>>%s dropping %d databases...", blue, nc, len(dbNames))
	if services.DropSharedDBsBatch(host, port, dbNames, user, pw) {
		fmt.Printf(" %sok%s\n", green, nc)
	} else {
		fmt.Printf(" %ssome failed%s\n", yellow, nc)
	}

	fmt.Printf("%s>>>%s creating %d databases...", blue, nc, len(dbNames))
	services.CreateSharedDBsBatch(host, port, dbNames, user, pw)
	fmt.Printf(" %sok%s\n", green, nc)

	fmt.Printf("\n%sDatabase reset complete for workspace '%s'.%s\n", green, workspaceBranch, nc)
}

func cmdUpdate() {
	fmt.Printf("%sChecking for updates...%s\n", bold, nc)

	out, err := exec.Command("curl", "-sL", "https://api.github.com/repos/toantran292/tncli/releases/latest").Output()
	if err != nil {
		fatal("could not fetch latest version")
	}

	// Parse tag_name from JSON (simple string scan)
	body := string(out)
	latest := ""
	for _, line := range strings.Split(body, "\n") {
		if strings.Contains(line, `"tag_name"`) {
			parts := strings.Split(line, `"`)
			if len(parts) >= 4 {
				latest = strings.TrimPrefix(parts[3], "v")
			}
		}
	}
	if latest == "" {
		fatal("could not fetch latest version")
	}
	if latest == version {
		fmt.Printf("%sAlready up to date: v%s%s\n", green, version, nc)
		return
	}

	fmt.Printf("Current: v%s → Latest: v%s\n", version, latest)
	fmt.Printf("%s>>>%s Downloading update...\n", blue, nc)

	osName := "linux"
	if strings.Contains(strings.ToLower(os.Getenv("OSTYPE")), "darwin") || exec.Command("uname").Run() == nil {
		uname, _ := exec.Command("uname").Output()
		if strings.Contains(strings.ToLower(string(uname)), "darwin") {
			osName = "darwin"
		}
	}
	arch := "amd64"
	unameM, _ := exec.Command("uname", "-m").Output()
	if strings.Contains(string(unameM), "arm64") || strings.Contains(string(unameM), "aarch64") {
		arch = "arm64"
	}

	url := fmt.Sprintf("https://github.com/toantran292/tncli/releases/download/v%s/tncli-%s-%s.tar.gz", latest, osName, arch)
	tmpdir := filepath.Join(os.TempDir(), "tncli-update")
	_ = os.MkdirAll(tmpdir, 0o755)
	tarPath := filepath.Join(tmpdir, "tncli.tar.gz")

	if exec.Command("curl", "-sL", "-o", tarPath, url).Run() != nil {
		fatal("download failed")
	}
	if exec.Command("tar", "xzf", tarPath, "-C", tmpdir).Run() != nil {
		fatal("extract failed")
	}

	binary := filepath.Join(tmpdir, fmt.Sprintf("tncli-%s-%s", osName, arch))
	if _, err := os.Stat(binary); os.IsNotExist(err) {
		fatal("binary not found in archive")
	}

	if osName == "darwin" {
		_ = exec.Command("xattr", "-rd", "com.apple.quarantine", binary).Run()
	}

	home, _ := os.UserHomeDir()
	installDir := filepath.Join(home, ".local/bin")
	installPath := filepath.Join(installDir, "tncli")
	_ = os.MkdirAll(installDir, 0o755)

	if exec.Command("cp", binary, installPath).Run() != nil {
		fatal("failed to copy binary to %s", installPath)
	}
	_ = exec.Command("chmod", "+x", installPath).Run()
	if osName == "darwin" {
		_ = exec.Command("codesign", "-s", "-", "--force", installPath).Run()
		_ = exec.Command("xattr", "-rd", "com.apple.quarantine", installPath).Run()
	}

	// Ensure PATH
	pathEnv := os.Getenv("PATH")
	if !strings.Contains(pathEnv, installDir) {
		zshrc := filepath.Join(home, ".zshrc")
		content, _ := os.ReadFile(zshrc)
		if !strings.Contains(string(content), ".local/bin") {
			f, err := os.OpenFile(zshrc, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644)
			if err == nil {
				fmt.Fprintf(f, "\n# tncli\nexport PATH=\"$HOME/.local/bin:$PATH\"\n")
				f.Close()
				fmt.Printf("\n%sAdded ~/.local/bin to PATH in ~/.zshrc%s\n", yellow, nc)
			}
		}
	}

	// Remove old binary
	oldPath := "/usr/local/bin/tncli"
	if _, err := os.Stat(oldPath); err == nil {
		fmt.Printf("%s>>>%s Removing old binary from %s...\n", blue, nc, oldPath)
		_ = exec.Command("sudo", "rm", oldPath).Run()
	}

	_ = os.RemoveAll(tmpdir)
	fmt.Printf("\n%sv%s installed to %s%s\n", green, latest, installPath, nc)
}

func cmdSetup(cfg *config.Config) {
	// 1. Setup loopback IPs
	subnetCount := services.SetupSubnetCount
	hostMax := services.SetupHostMax
	var ips []string
	for subnet := 1; subnet <= int(subnetCount); subnet++ {
		for host := 2; host <= int(hostMax); host++ {
			ips = append(ips, fmt.Sprintf("127.0.%d.%d", subnet, host))
		}
	}
	hostsPerSubnet := int(hostMax) - 1
	total := len(ips)

	// Check if already setup
	alreadySetup := exec.Command("ping", "-c", "1", "-W", "1", "127.0.1.2").Run() == nil
	if alreadySetup {
		fmt.Printf("%s>>>%s loopback IPs already configured (%d IPs, %d subnets × %d hosts)\n", green, nc, total, subnetCount, hostsPerSubnet)
	} else {
		fmt.Printf("%sSetting up loopback IPs (127.0.{1..%d}.{2..%d})...%s\n", bold, subnetCount, hostMax, nc)
		var cmds []string
		for _, ip := range ips {
			cmds = append(cmds, fmt.Sprintf("ifconfig lo0 alias %s 2>/dev/null", ip))
		}
		script := strings.Join(cmds, "; ")
		if err := exec.Command("sudo", "sh", "-c", script).Run(); err == nil {
			fmt.Printf("%s>>>%s %s%d loopback IPs configured%s (%d subnets × %d hosts)\n", green, nc, green, total, nc, subnetCount, hostsPerSubnet)
		} else {
			fmt.Fprintf(os.Stderr, "%swarning:%s failed to setup loopback IPs (sudo required)\n", yellow, nc)
		}
		_ = exec.Command("sudo", "dscacheutil", "-flushcache").Run()
		_ = exec.Command("sudo", "killall", "-HUP", "mDNSResponder").Run()
	}

	// 1b. LaunchDaemon for persistence
	home, _ := os.UserHomeDir()
	scriptPath := filepath.Join(home, ".tncli/setup-loopback.sh")
	plistPath := "/Library/LaunchDaemons/com.tncli.loopback.plist"
	_ = os.MkdirAll(filepath.Join(home, ".tncli"), 0o755)
	var scriptLines []string
	scriptLines = append(scriptLines, "#!/bin/sh")
	for _, ip := range ips {
		scriptLines = append(scriptLines, fmt.Sprintf("ifconfig lo0 alias %s 2>/dev/null", ip))
	}
	_ = os.WriteFile(scriptPath, []byte(strings.Join(scriptLines, "\n")+"\n"), 0o755)

	if _, err := os.Stat(plistPath); err == nil {
		fmt.Printf("%s>>>%s LaunchDaemon already installed\n", green, nc)
	} else {
		plistContent := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tncli.loopback</string>
    <key>ProgramArguments</key>
    <array>
        <string>%s</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
`, scriptPath)
		tmpPlist := filepath.Join(home, ".tncli/com.tncli.loopback.plist")
		_ = os.WriteFile(tmpPlist, []byte(plistContent), 0o644)
		if exec.Command("sudo", "cp", tmpPlist, plistPath).Run() == nil {
			_ = exec.Command("sudo", "chown", "root:wheel", plistPath).Run()
			fmt.Printf("%s>>>%s LaunchDaemon installed\n", green, nc)
		}
		_ = os.Remove(tmpPlist)
	}

	// 2. /etc/hosts for shared services
	var hostnames []string
	for name, svc := range cfg.SharedServices {
		host := svc.Host
		if host == "" {
			host = fmt.Sprintf("%s.%s.tncli.test", cfg.Session, name)
		}
		found := false
		for _, h := range hostnames {
			if h == host {
				found = true
				break
			}
		}
		if !found {
			hostnames = append(hostnames, host)
		}
	}
	if len(hostnames) > 0 {
		hostsContent, _ := os.ReadFile("/etc/hosts")
		var missing []string
		for _, h := range hostnames {
			if !strings.Contains(string(hostsContent), h) {
				missing = append(missing, h)
			}
		}
		if len(missing) == 0 {
			fmt.Printf("%s>>>%s /etc/hosts already configured\n", green, nc)
		} else {
			fmt.Printf("%sAdding to /etc/hosts:%s\n", bold, nc)
			var entries []string
			for _, h := range missing {
				fmt.Printf("  127.0.0.1 %s\n", h)
				entries = append(entries, "127.0.0.1 "+h)
			}
			cmd := fmt.Sprintf("echo '\n# tncli shared services\n%s' >> /etc/hosts", strings.Join(entries, "\n"))
			if exec.Command("sudo", "sh", "-c", cmd).Run() == nil {
				fmt.Printf("%s>>>%s %s/etc/hosts updated%s\n", green, nc, green, nc)
			}
		}
	}

	// 3. Global gitignore
	services.EnsureGlobalGitignore()
	fmt.Printf("%s>>>%s global gitignore configured\n", green, nc)

	// 4. Caddy
	hasCaddy := exec.Command("caddy", "version").Run() == nil
	if hasCaddy {
		fmt.Printf("%s>>>%s caddy already installed\n", green, nc)
	} else {
		fmt.Printf("%sInstalling caddy...%s\n", bold, nc)
		if exec.Command("brew", "install", "caddy").Run() == nil {
			fmt.Printf("%s>>>%s %scaddy installed%s\n", green, nc, green, nc)
		} else {
			fmt.Fprintf(os.Stderr, "%swarning:%s failed to install caddy\n", yellow, nc)
		}
	}

	// 5. DNS
	fmt.Printf("\n%s[4/4] DNS (*.tncli.test → 127.0.0.1)%s\n", bold, nc)
	dnsStatus := services.GetDNSStatus()
	if dnsStatus.IsReady() {
		fmt.Printf("%s>>>%s dnsmasq already configured and running\n", green, nc)
		resolved := false
		for i := 0; i < 3; i++ {
			if services.VerifyResolution() {
				resolved = true
				break
			}
			time.Sleep(time.Second)
		}
		if resolved {
			fmt.Printf("%s>>>%s *.tncli.test resolves correctly\n", green, nc)
		} else {
			fmt.Fprintf(os.Stderr, "%swarning:%s DNS resolution not working — try: sudo brew services restart dnsmasq\n", yellow, nc)
		}
	} else {
		actions, err := services.SetupDnsmasq()
		if err == nil {
			for _, a := range actions {
				fmt.Printf("%s>>>%s %s\n", green, nc, a)
			}
			time.Sleep(2 * time.Second)
			if services.VerifyResolution() {
				fmt.Printf("%s>>>%s *.tncli.test resolves correctly\n", green, nc)
			} else {
				fmt.Fprintf(os.Stderr, "%swarning:%s DNS resolution not yet working — may need a few seconds\n", yellow, nc)
			}
		} else {
			fmt.Fprintf(os.Stderr, "%swarning:%s dnsmasq setup failed: %v\n", yellow, nc, err)
		}
	}

	fmt.Printf("\n%sSetup complete!%s\n", green, nc)
}

func cmdProxyStart() {
	if services.IsProxyRunning() {
		pid, _ := services.ReadPID()
		fmt.Printf("%sproxy already running%s (pid %d)\n", green, nc, pid)
		return
	}

	cfgPath, err := config.FindConfig()
	if err != nil {
		fatal("%v", err)
	}
	cfg, err := config.Load(cfgPath)
	if err != nil {
		fatal("%v", err)
	}
	registerProxyRoutesFromConfig(cfg)

	exe, _ := os.Executable()
	home, _ := os.UserHomeDir()
	_ = os.MkdirAll(filepath.Join(home, ".tncli"), 0o755)

	cmd := fmt.Sprintf("%s proxy serve", exe)
	child := strings.NewReader("")
	_ = child
	// Start as daemon
	proc, err := os.StartProcess(exe, []string{exe, "proxy", "serve"}, &os.ProcAttr{
		Files: []*os.File{nil, nil, nil},
	})
	if err != nil {
		fatal("failed to start proxy: %v", err)
	}
	fmt.Printf("%sproxy started%s (pid %d)\n", green, nc, proc.Pid)
	_ = proc.Release()
	_ = cmd
}

func cmdProxyStop() {
	pid, ok := services.ReadPID()
	if !ok {
		fmt.Println("proxy not running")
		return
	}
	p, err := os.FindProcess(pid)
	if err == nil {
		_ = p.Kill()
	}
	services.RemovePID()
	fmt.Printf("%sproxy stopped%s (was pid %d)\n", green, nc, pid)
}

func cmdProxyRestart() {
	cmdProxyStop()
	home, _ := os.UserHomeDir()
	_ = os.Remove(filepath.Join(home, ".tncli/proxy-routes.json"))
	cmdProxyStart()
}

func cmdProxyStatus() {
	if services.IsProxyRunning() {
		pid, _ := services.ReadPID()
		fmt.Printf("%sproxy running%s (pid %d)\n", green, nc, pid)
	} else {
		fmt.Printf("%sproxy not running%s\n", yellow, nc)
	}

	routes := services.LoadRoutes()
	if len(routes.Routes) == 0 {
		fmt.Println("no routes configured")
	} else {
		fmt.Printf("\n%sListen ports:%s %v\n", bold, nc, routes.ListenPorts)
		fmt.Printf("\n%sRoutes:%s\n", bold, nc)
		for hostname, target := range routes.Routes {
			fmt.Printf("  %s%s%s → %s\n", blue, hostname, nc, target)
		}
	}
}

func cmdProxyInstall() {
	exe, err := os.Executable()
	if err != nil {
		fatal("%v", err)
	}

	home, _ := os.UserHomeDir()
	plistDir := filepath.Join(home, "Library/LaunchAgents")
	plistPath := filepath.Join(plistDir, "com.tncli.proxy.plist")
	logPath := filepath.Join(home, ".tncli/proxy.log")

	plist := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tncli.proxy</string>
    <key>ProgramArguments</key>
    <array>
        <string>%s</string>
        <string>proxy</string>
        <string>serve</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>%s</string>
    <key>StandardErrorPath</key>
    <string>%s</string>
</dict>
</plist>`, exe, logPath, logPath)

	_ = os.MkdirAll(plistDir, 0o755)
	if err := os.WriteFile(plistPath, []byte(plist), 0o644); err != nil {
		fatal("failed to write plist: %v", err)
	}
	_ = exec.Command("launchctl", "unload", plistPath).Run()
	if exec.Command("launchctl", "load", plistPath).Run() == nil {
		fmt.Printf("%sproxy daemon installed and started%s\n", green, nc)
		fmt.Printf("  plist: %s\n  log:   %s\n", plistPath, logPath)
	} else {
		fatal("failed to load launchd plist")
	}
}

func cmdProxyUninstall() {
	home, _ := os.UserHomeDir()
	plistPath := filepath.Join(home, "Library/LaunchAgents/com.tncli.proxy.plist")
	if _, err := os.Stat(plistPath); err == nil {
		_ = exec.Command("launchctl", "unload", plistPath).Run()
		_ = os.Remove(plistPath)
		fmt.Printf("%sproxy daemon uninstalled%s\n", green, nc)
	} else {
		fmt.Println("proxy daemon not installed")
	}
}

func registerProxyRoutesFromConfig(cfg *config.Config) {
	var entries []services.ProxyEntry
	for _, dir := range cfg.Repos {
		if dir.Alias != "" && dir.ProxyPort != nil {
			entries = append(entries, services.ProxyEntry{Alias: dir.Alias, Port: *dir.ProxyPort})
		}
		for svcName, svc := range dir.Services {
			if svc.ProxyPort != nil {
				entries = append(entries, services.ProxyEntry{Alias: svcName, Port: *svc.ProxyPort})
			}
		}
	}
	if len(entries) == 0 {
		return
	}

	defaultBranch := cfg.GlobalDefaultBranch()
	mainIP := services.MainIP(cfg.Session, defaultBranch)
	branchSafe := services.BranchSafe(defaultBranch)
	for i := range entries {
		entries[i].BindIP = mainIP
	}
	services.RegisterRoutesSimple(cfg.Session, branchSafe, entries)

	// Scan workspace folders
	cwd, _ := os.Getwd()
	dirEntries, _ := os.ReadDir(cwd)
	for _, e := range dirEntries {
		if branch, ok := strings.CutPrefix(e.Name(), "workspace--"); ok && e.IsDir() {
			wsKey := "ws-" + branch
			ip := services.AllocateIP(cfg.Session, wsKey)
			bs := services.BranchSafe(branch)
			wsEntries := make([]services.ProxyEntry, len(entries))
			copy(wsEntries, entries)
			for i := range wsEntries {
				wsEntries[i].BindIP = ip
			}
			services.RegisterRoutesSimple(cfg.Session, bs, wsEntries)
		}
	}
}

func findPGService(cfg *config.Config) *config.SharedServiceDef {
	for _, svc := range cfg.SharedServices {
		if svc.DBUser != "" {
			return svc
		}
	}
	return nil
}
