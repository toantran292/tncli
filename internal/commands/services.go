package commands

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/lock"
	"github.com/toantran292/tncli/internal/tmux"
)

func Start(cfg *config.Config, cfgPath, target string) error {
	pairs, err := cfg.ResolveServices(target)
	if err != nil {
		return err
	}
	pairs = orderByDeps(cfg, pairs, false)
	configDir := filepath.Dir(cfgPath)

	lock.EnsureDir()
	createdSession := tmux.CreateSessionIfNeeded(cfg.SvcSession())
	started, skipped := 0, 0

	for _, pair := range pairs {
		dirName, svcName := pair[0], pair[1]
		if tmux.WindowExists(cfg.SvcSession(), svcName) {
			fmt.Fprintf(os.Stderr, "%swarning:%s '%s' is already running — skipping\n", Yellow, NC, svcName)
			skipped++
			continue
		}

		resolved, err := cfg.ResolveService(configDir, dirName, svcName)
		if err != nil {
			return err
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
		fmt.Printf("%s>>>%s started %s%s%s (%s%s%s)\n", Green, NC, Bold, svcName, NC, Dim, dirName, NC)
		started++
	}

	if createdSession {
		tmux.CleanupInitWindow(cfg.SvcSession())
	}
	if started > 0 {
		fmt.Printf("\n%s%d service(s) started%s in session %s%s%s\n", Green, started, NC, Cyan, cfg.Session, NC)
		fmt.Printf("%sattach: tncli attach%s\n", Dim, NC)
	}
	if skipped > 0 {
		fmt.Printf("%s%d service(s) skipped (already running)%s\n", Yellow, skipped, NC)
	}
	return nil
}

func Stop(cfg *config.Config, target string) error {
	lock.EnsureDir()

	if target == "" {
		if tmux.SessionExists(cfg.SvcSession()) {
			tmux.KillSession(cfg.SvcSession())
			lock.ReleaseAll(cfg.SvcSession())
			fmt.Printf("%s>>>%s stopped all services (session %s%s%s killed)\n", Green, NC, Cyan, cfg.Session, NC)
		} else {
			fmt.Printf("%s>>>%s no running session '%s'\n", Blue, NC, cfg.Session)
		}
		return nil
	}

	pairs, err := cfg.ResolveServices(target)
	if err != nil {
		return err
	}
	pairs = orderByDeps(cfg, pairs, true)
	stopped := 0
	for _, pair := range pairs {
		svcName := pair[1]
		if tmux.WindowExists(cfg.SvcSession(), svcName) {
			tmux.GracefulStop(cfg.SvcSession(), svcName)
			lock.Release(cfg.SvcSession(), svcName)
			fmt.Printf("%s>>>%s stopped %s%s%s\n", Green, NC, Bold, svcName, NC)
			stopped++
		} else {
			fmt.Fprintf(os.Stderr, "%swarning:%s '%s' is not running\n", Yellow, NC, svcName)
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
	fmt.Printf("%s%d service(s) stopped%s\n", Green, stopped, NC)
	return nil
}

func Restart(cfg *config.Config, cfgPath, target string) error {
	if err := Stop(cfg, target); err != nil {
		return err
	}
	return Start(cfg, cfgPath, target)
}

func Status(cfg *config.Config) {
	if !tmux.SessionExists(cfg.SvcSession()) {
		fmt.Printf("%sno active session '%s'%s\n", Dim, cfg.Session, NC)
		return
	}
	fmt.Printf("%sSession:%s %s%s%s\n\n", Bold, NC, Cyan, cfg.Session, NC)
	windows := tmux.ListWindows(cfg.SvcSession())
	for _, dirName := range cfg.RepoOrder {
		dir := cfg.Repos[dirName]
		fmt.Printf("%s%s%s\n", Bold, dirName, NC)
		for _, svcName := range dir.ServiceOrder {
			if windows[svcName] {
				fmt.Printf("  %s●%s %s\n", Green, NC, svcName)
			} else {
				fmt.Printf("  %s○ %s%s\n", Dim, svcName, NC)
			}
		}
	}
	fmt.Printf("\n%sattach: tncli attach%s\n", Dim, NC)
}

func Attach(cfg *config.Config, target string) error {
	if !tmux.SessionExists(cfg.SvcSession()) {
		return fmt.Errorf("no active session '%s'", cfg.Session)
	}
	return tmux.Attach(cfg.SvcSession(), target)
}

func Logs(cfg *config.Config, target string) error {
	if !tmux.WindowExists(cfg.SvcSession(), target) {
		return fmt.Errorf("service '%s' is not running", target)
	}
	for _, line := range tmux.CapturePane(cfg.SvcSession(), target, 100) {
		fmt.Println(line)
	}
	return nil
}

func List(cfg *config.Config) {
	fmt.Printf("%sServices:%s\n", Bold, NC)
	for _, dirName := range cfg.RepoOrder {
		dir := cfg.Repos[dirName]
		alias := ""
		if dir.Alias != "" {
			alias = " (" + dir.Alias + ")"
		}
		fmt.Printf("  %s%s%s%s\n", Bold, dirName, alias, NC)
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
		fmt.Printf("\n%sWorkspaces:%s\n", Bold, NC)
		for name, entries := range workspaces {
			fmt.Printf("  %s: %s\n", name, strings.Join(entries, ", "))
		}
	}
}

// orderByDeps reorders service pairs based on depends_on within each dir.
func orderByDeps(cfg *config.Config, pairs [][2]string, reverse bool) [][2]string {
	// Group by dir
	byDir := make(map[string][]string)
	var dirOrder []string
	for _, pair := range pairs {
		d := pair[0]
		if _, ok := byDir[d]; !ok {
			dirOrder = append(dirOrder, d)
		}
		byDir[d] = append(byDir[d], pair[1])
	}

	var result [][2]string
	for _, dirName := range dirOrder {
		dir, ok := cfg.Repos[dirName]
		if !ok {
			for _, svc := range byDir[dirName] {
				result = append(result, [2]string{dirName, svc})
			}
			continue
		}
		graph := config.BuildDepGraph(dir)
		var ordered []string
		var err error
		if reverse {
			ordered, err = graph.StopOrder(byDir[dirName])
		} else {
			ordered, err = graph.StartOrder(byDir[dirName])
		}
		if err != nil {
			ordered = byDir[dirName]
		}
		for _, svc := range ordered {
			result = append(result, [2]string{dirName, svc})
		}
	}
	return result
}
