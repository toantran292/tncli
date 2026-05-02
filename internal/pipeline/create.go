package pipeline

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tmux"
)

type CreateState struct {
	WsFolder    string
	NetworkName string
	BranchSafe  string
	BindIP      string
	WtDirs      [][2]string // (dir_name, wt_path)
}

func NewCreateState(ctx *CreateContext) *CreateState {
	return &CreateState{
		NetworkName: "tncli-ws-" + ctx.Branch,
		BranchSafe:  services.BranchSafe(ctx.Branch),
		BindIP:      ctx.BindIP,
	}
}

func ExecuteCreateStage(stage CreateStage, ctx *CreateContext, state *CreateState) error {
	switch stage {
	case StageValidate:
		return stageValidate(ctx)
	case StageProvision:
		return stageProvision(ctx, state)
	case StageInfra:
		return stageInfra(ctx, state)
	case StageSource:
		return stageSourceParallel(ctx, state)
	case StageConfigure:
		return stageConfigureParallel(ctx, state)
	case StageSetup:
		return stageSetupParallel(ctx, state)
	case StageNetwork:
		return stageNetworkCreate(ctx, state)
	}
	return nil
}

func stageValidate(ctx *CreateContext) error {
	if len(ctx.Config.SharedServices) == 0 {
		return nil
	}
	var hostnames []string
	for _, svc := range ctx.Config.SharedServices {
		if svc.Host != "" && !strings.HasSuffix(svc.Host, ".tncli.test") {
			hostnames = append(hostnames, svc.Host)
		}
	}
	if len(hostnames) > 0 {
		missing := services.CheckEtcHosts(hostnames)
		if len(missing) > 0 {
			return fmt.Errorf("missing hosts in /etc/hosts: %s. Run: tncli setup", strings.Join(missing, ", "))
		}
	}
	return nil
}

func stageProvision(ctx *CreateContext, state *CreateState) error {
	if state.BindIP == "" {
		state.BindIP = services.AllocateIP(ctx.Session, "ws-"+ctx.Branch)
	}

	if len(ctx.Config.SharedServices) > 0 {
		wsKey := "ws-" + ctx.Branch
		allocated := make(map[string]bool)

		for _, dirName := range ctx.UniqueDirs {
			dir, ok := ctx.Config.Repos[dirName]
			if !ok || dir.WT() == nil {
				continue
			}
			wt := dir.WT()
			for _, sref := range wt.SharedServices {
				if allocated[sref.Name] {
					continue
				}
				if svcDef, ok := ctx.Config.SharedServices[sref.Name]; ok && svcDef.Capacity != nil {
					basePort := services.FirstPortFromList(svcDef.Ports)
					services.AllocateSlot(sref.Name, wsKey, *svcDef.Capacity, basePort)
					allocated[sref.Name] = true
				}
			}
			// Auto-detect {{slot:SERVICE}}
			for _, val := range wt.Env {
				s := val
				for {
					start := strings.Index(s, "{{slot:")
					if start < 0 {
						break
					}
					end := strings.Index(s[start:], "}}")
					if end < 0 {
						break
					}
					svcName := s[start+7 : start+end]
					if !allocated[svcName] {
						if svcDef, ok := ctx.Config.SharedServices[svcName]; ok && svcDef.Capacity != nil {
							basePort := services.FirstPortFromList(svcDef.Ports)
							services.AllocateSlot(svcName, wsKey, *svcDef.Capacity, basePort)
							allocated[svcName] = true
						}
					}
					s = s[start+end+2:]
				}
			}
		}
	}

	state.WsFolder = services.EnsureWorkspaceFolder(ctx.ConfigDir, ctx.Branch)
	return nil
}

func stageInfra(ctx *CreateContext, state *CreateState) error {
	if len(ctx.Config.SharedServices) == 0 {
		return nil
	}

	var allServices []string
	for name := range ctx.Config.SharedServices {
		allServices = append(allServices, name)
	}
	services.GenerateSharedCompose(ctx.ConfigDir, ctx.Session, ctx.Config.SharedServices)
	services.StartSharedServices(ctx.ConfigDir, ctx.Session, allServices)

	// Create databases for worktree branch
	createDatabases(ctx, state.BranchSafe, ctx.Branch)
	return nil
}

func stageSourceParallel(ctx *CreateContext, state *CreateState) error {
	var mu sync.Mutex
	var errs []error
	var wg sync.WaitGroup

	for _, db := range ctx.DirBranches {
		dirName, baseBranch := db[0], db[1]
		dirPath := ""
		for _, dp := range ctx.DirPaths {
			if dp[0] == dirName {
				dirPath = dp[1]
				break
			}
		}
		if dirPath == "" {
			continue
		}

		targetBranch := ctx.Branch
		if ctx.SelectedDirs != nil {
			for _, sd := range ctx.SelectedDirs {
				if sd[0] == dirName {
					targetBranch = sd[1]
					break
				}
			}
		}

		dir := ctx.Config.Repos[dirName]
		var copyFiles []string
		if dir != nil && dir.WT() != nil {
			copyFiles = dir.WT().Copy
		}

		wg.Add(1)
		go func(dn, dp, tb, bb string, cf []string) {
			defer wg.Done()
			wtPath, err := services.CreateWorktreeFromBase(dp, tb, bb, cf, state.WsFolder)
			mu.Lock()
			defer mu.Unlock()
			if err != nil {
				errs = append(errs, fmt.Errorf("failed to create worktree for %s: %w", dn, err))
			} else {
				state.WtDirs = append(state.WtDirs, [2]string{dn, wtPath})
			}
		}(dirName, dirPath, targetBranch, baseBranch, copyFiles)
	}
	wg.Wait()

	if len(errs) > 0 {
		return errs[0]
	}
	return nil
}

func stageConfigureParallel(ctx *CreateContext, state *CreateState) error {
	if state.WsFolder == "" {
		state.WsFolder = filepath.Join(ctx.ConfigDir, "workspace--"+ctx.Branch)
	}
	if len(state.WtDirs) == 0 {
		for _, d := range ctx.UniqueDirs {
			wtPath := filepath.Join(state.WsFolder, d)
			if isDir(wtPath) {
				state.WtDirs = append(state.WtDirs, [2]string{d, wtPath})
			}
		}
	}

	var wg sync.WaitGroup
	for _, wd := range state.WtDirs {
		dirName, wtPath := wd[0], wd[1]
		dirPath := ""
		for _, dp := range ctx.DirPaths {
			if dp[0] == dirName {
				dirPath = dp[1]
				break
			}
		}

		dir := ctx.Config.Repos[dirName]
		if dir == nil || dir.WT() == nil {
			continue
		}
		wt := dir.WT()

		var svcOverrides map[string]*config.ServiceOverride
		var sharedHosts []string
		for _, so := range ctx.SharedOverrides {
			if so.DirName == dirName {
				svcOverrides = so.Overrides
				sharedHosts = so.Hosts
				break
			}
		}

		wg.Add(1)
		go func(dp, wp string) {
			defer wg.Done()
			wsKey := "ws-" + strings.ReplaceAll(ctx.Branch, "/", "-")
			_ = services.WriteEnvFile(wp, state.BindIP)
			// Apply env files
			applyAllEnvFiles(wt, wp, ctx.Config, state.BindIP, ctx.Branch, wsKey)
			_ = dp
			_ = svcOverrides
			_ = sharedHosts
		}(dirPath, wtPath)
	}
	wg.Wait()

	services.EnsureGlobalGitignore()
	services.EnsureNodeBindHost()
	return nil
}

func stageSetupParallel(ctx *CreateContext, state *CreateState) error {
	tmux.CreateSessionIfNeeded(ctx.TmuxSession)

	var tmuxWindows []string
	for _, wd := range state.WtDirs {
		dirName, wtPath := wd[0], wd[1]
		dir := ctx.Config.Repos[dirName]
		if dir == nil || dir.WT() == nil || len(dir.WT().Setup) == 0 {
			continue
		}

		alias := dir.Alias
		if alias == "" {
			alias = dirName
		}
		branchSafe := services.BranchSafe(ctx.Branch)
		winName := fmt.Sprintf("setup~%s~%s", alias, branchSafe)

		combined := strings.Join(dir.WT().Setup, " && ")
		home, _ := os.UserHomeDir()
		patch := filepath.Join(home, ".tncli/node-bind-host.js")
		nodeOpts := ""
		if fileExists(patch) {
			nodeOpts = fmt.Sprintf(`export NODE_OPTIONS="--dns-result-order=ipv4first --require %s ${NODE_OPTIONS:-}" && `, patch)
		}
		cmd := fmt.Sprintf("cd '%s' && set -a && source .env.local 2>/dev/null; set +a && %s%s", wtPath, nodeOpts, combined)
		tmux.NewWindowAutoclose(ctx.TmuxSession, winName, cmd)
		// Set remain-on-exit
		_ = exec.Command("tmux", "set-option", "-t",
			fmt.Sprintf("=%s:%s", ctx.TmuxSession, winName), "remain-on-exit", "on").Run()
		tmuxWindows = append(tmuxWindows, winName)
	}

	// Wait for all setup commands
	if len(tmuxWindows) > 0 {
		for {
			time.Sleep(2 * time.Second)
			stillRunning := false
			for _, w := range tmuxWindows {
				out, err := exec.Command("tmux", "list-panes", "-t",
					fmt.Sprintf("=%s:%s", ctx.TmuxSession, w), "-F", "#{pane_dead}").Output()
				if err != nil {
					continue
				}
				if strings.TrimSpace(string(out)) == "0" {
					stillRunning = true
					break
				}
			}
			if !stillRunning {
				break
			}
		}
		for _, w := range tmuxWindows {
			_ = exec.Command("tmux", "kill-window", "-t",
				fmt.Sprintf("=%s:%s", ctx.TmuxSession, w)).Run()
		}
	}

	return nil
}

func stageNetworkCreate(ctx *CreateContext, state *CreateState) error {
	if err := services.CreateDockerNetwork(state.NetworkName); err != nil {
		return err
	}

	// Register proxy routes
	branchSafe := services.BranchSafe(ctx.Branch)
	var proxyEntries []services.ProxyEntry
	for _, dir := range ctx.Config.Repos {
		if dir.Alias != "" && dir.ProxyPort != nil {
			proxyEntries = append(proxyEntries, services.ProxyEntry{
				Alias: dir.Alias, Port: *dir.ProxyPort, BindIP: state.BindIP,
			})
		}
		for svcName, svc := range dir.Services {
			if svc.ProxyPort != nil {
				proxyEntries = append(proxyEntries, services.ProxyEntry{
					Alias: svcName, Port: *svc.ProxyPort, BindIP: state.BindIP,
				})
			}
		}
	}
	if len(proxyEntries) > 0 {
		services.RegisterRoutesSimple(ctx.Session, branchSafe, proxyEntries)
		services.ReloadCaddy()
	}

	return nil
}

// ── Helpers ──

func createDatabases(ctx *CreateContext, branchSafe, branch string) {
	var dbNames []string
	pgSvc := findPGService(ctx.Config)
	host := ctx.Config.SharedHost("postgres")
	port := uint16(5432)
	user := "postgres"
	pw := "postgres"
	if pgSvc != nil {
		if pgSvc.Host != "" {
			host = pgSvc.Host
		}
		port = services.FirstPortFromList(pgSvc.Ports)
		if port == 0 {
			port = 5432
		}
		if pgSvc.DBUser != "" {
			user = pgSvc.DBUser
		}
		if pgSvc.DBPassword != "" {
			pw = pgSvc.DBPassword
		}
	}

	for _, dirName := range ctx.UniqueDirs {
		dir := ctx.Config.Repos[dirName]
		if dir == nil || dir.WT() == nil {
			continue
		}
		wt := dir.WT()
		for _, sref := range wt.SharedServices {
			if sref.DBName != "" {
				dbName := strings.ReplaceAll(sref.DBName, "{{branch_safe}}", branchSafe)
				dbName = strings.ReplaceAll(dbName, "{{branch}}", branch)
				dbNames = append(dbNames, dbName)
			}
		}
		for _, dbTpl := range wt.Databases {
			dbName := strings.ReplaceAll(dbTpl, "{{branch_safe}}", branchSafe)
			dbName = strings.ReplaceAll(dbName, "{{branch}}", branch)
			dbNames = append(dbNames, ctx.Session+"_"+dbName)
		}
	}

	if len(dbNames) > 0 {
		services.CreateSharedDBsBatch(host, port, dbNames, user, pw)
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

func applyAllEnvFiles(wt *config.WorktreeConfig, dir string, cfg *config.Config, bindIP, branch, wsKey string) {
	branchSafe := services.BranchSafe(branch)
	dbNames := make([]string, 0, len(wt.Databases))
	for _, tpl := range wt.Databases {
		name := strings.ReplaceAll(tpl, "{{branch_safe}}", branchSafe)
		name = strings.ReplaceAll(name, "{{branch}}", branch)
		dbNames = append(dbNames, cfg.Session+"_"+name)
	}

	baseEnv := make(map[string]string)
	for k, v := range cfg.Env {
		baseEnv[k] = v
	}
	for k, v := range wt.Env {
		baseEnv[k] = v
	}

	for _, entry := range wt.EnvFileEntries() {
		envSrc := baseEnv
		if len(entry.Env) > 0 {
			envSrc = make(map[string]string)
			for k, v := range baseEnv {
				envSrc[k] = v
			}
			for k, v := range entry.Env {
				envSrc[k] = v
			}
		}
		resolved := services.ResolveEnvTemplates(envSrc, cfg, bindIP, branchSafe, branch, wsKey)
		for i, kv := range resolved {
			resolved[i] = [2]string{kv[0], services.ResolveDBTemplates(kv[1], dbNames)}
		}
		services.ApplyEnvOverrides(dir, resolved, entry.File)
	}
}

func fileExists(path string) bool {
	return services.FileExists(path)
}
