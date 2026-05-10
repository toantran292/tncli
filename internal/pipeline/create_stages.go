package pipeline

import (
	"fmt"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tmux"
)

func stageValidate(ctx *CreateContext) error {
	// Validate config is usable — currently a no-op since port allocation
	// handles everything. Kept as pipeline stage for future validation.
	return nil
}

func stageProvision(ctx *CreateContext, state *CreateState) error {
	if len(ctx.Config.SharedServices) > 0 {
		allocateSharedSlots(ctx)
	}

	state.WsFolder = services.EnsureWorkspaceFolder(ctx.ConfigDir, ctx.Branch)
	if ctx.Environment != "" {
		svcEnvs := make(map[string]string)
		for _, dirName := range ctx.UniqueDirs {
			alias := dirName
			if dir, ok := ctx.Config.Repos[dirName]; ok && dir.Alias != "" {
				alias = dir.Alias
			}
			svcEnvs[alias] = ctx.Environment
		}
		services.SaveWorkspaceState(state.WsFolder, &services.WorkspaceState{
			ServiceEnvs: svcEnvs,
		})
	}
	return nil
}

func allocateSharedSlots(ctx *CreateContext) {
	wsKey := "ws-" + ctx.Branch
	allocated := make(map[string]bool)

	for _, dirName := range ctx.UniqueDirs {
		dir, ok := ctx.Config.Repos[dirName]
		if !ok || !dir.HasWorktreeConfig() {
			continue
		}
		// Explicit shared_services refs
		for _, sref := range dir.SharedSvcRefs {
			if allocated[sref.Name] {
				continue
			}
			if svcDef, ok := ctx.Config.SharedServices[sref.Name]; ok && svcDef.Capacity != nil {
				basePort := uint16(services.SharedPort(sref.Name))
				services.AllocateSlot(sref.Name, wsKey, *svcDef.Capacity, basePort)
				allocated[sref.Name] = true
			}
		}
		// Auto-detect {{slot:SERVICE}} in env values
		for _, val := range dir.Env {
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
						basePort := uint16(services.SharedPort(svcName))
						services.AllocateSlot(svcName, wsKey, *svcDef.Capacity, basePort)
						allocated[svcName] = true
					}
				}
				s = s[start+end+2:]
			}
		}
	}
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

	createDatabases(ctx, state.BranchSafe, ctx.Branch)
	return nil
}

func stageSourceParallel(ctx *CreateContext, state *CreateState) error {
	var mu sync.Mutex
	var errs []error
	var wg sync.WaitGroup

	for _, db := range ctx.DirBranches {
		dirName, baseBranch := db.Name, db.Branch
		dirPath := findDirPath(ctx, dirName)
		if dirPath == "" {
			continue
		}

		targetBranch := resolveTargetBranch(ctx, dirName)

		dir := ctx.Config.Repos[dirName]
		var copyFiles []string
		if dir != nil && dir.HasWorktreeConfig() {
			copyFiles = dir.Copy
		}

		checkoutBranch := ""
		if targetBranch != ctx.Branch {
			checkoutBranch = targetBranch
			targetBranch = ctx.Branch // create worktree with workspace branch first
		}

		wg.Add(1)
		go func(dn, dp, tb, bb, cb string, cf []string) {
			defer wg.Done()
			wtPath, err := services.CreateWorktreeFromBase(dp, tb, bb, cf, state.WsFolder)
			if err != nil {
				mu.Lock()
				errs = append(errs, fmt.Errorf("failed to create worktree for %s: %w", dn, err))
				mu.Unlock()
				return
			}
			// Checkout different branch if requested
			if cb != "" {
				if chErr := services.Checkout(wtPath, cb); chErr != nil {
					// Try fetching first
					_ = exec.Command("git", "-C", wtPath, "fetch", "origin", cb).Run()
					_ = services.Checkout(wtPath, cb)
				}
			}
			mu.Lock()
			state.WtDirs = append(state.WtDirs, services.DirMapping{Name: dn, Path: wtPath})
			mu.Unlock()
		}(dirName, dirPath, targetBranch, baseBranch, checkoutBranch, copyFiles)
	}
	wg.Wait()

	if len(errs) > 0 {
		for _, dm := range state.WtDirs {
			_ = services.RemoveWorktree(dm.Path+"/..", dm.Path, ctx.Branch)
		}
		state.WtDirs = nil
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
			if services.DirExists(wtPath) {
				state.WtDirs = append(state.WtDirs, services.DirMapping{Name: d, Path: wtPath})
			}
		}
	}

	var wg sync.WaitGroup
	for _, wd := range state.WtDirs {
		dirName, wtPath := wd.Name, wd.Path
		dir := ctx.Config.Repos[dirName]
		if dir == nil || !dir.HasWorktreeConfig() {
			continue
		}

		wg.Add(1)
		go func(wp string, dn string) {
			defer wg.Done()
			wsKey := "ws-" + strings.ReplaceAll(ctx.Branch, "/", "-")
			applyAllEnvFiles(ctx.Config.Repos[dn], wp, ctx.Config, ctx.Branch, wsKey, ctx.Environment)
		}(wtPath, dirName)
	}
	wg.Wait()

	services.EnsureGlobalGitignore()
	return nil
}

func stageSetupParallel(ctx *CreateContext, state *CreateState) error {
	tmux.CreateSessionIfNeeded(ctx.TmuxSession)

	var tmuxWindows []string
	for _, wd := range state.WtDirs {
		dirName, wtPath := wd.Name, wd.Path
		dir := ctx.Config.Repos[dirName]
		if dir == nil || !dir.HasWorktreeConfig() || len(dir.Setup) == 0 {
			continue
		}

		alias := dir.Alias
		if alias == "" {
			alias = dirName
		}
		branchSafe := services.BranchSafe(ctx.Branch)
		winName := fmt.Sprintf("setup~%s~%s", alias, branchSafe)

		var setupCmds []string
		for _, s := range dir.Setup {
			setupCmds = append(setupCmds, ctx.Config.TransformInstallCmd(s, true))
		}
		combined := strings.Join(setupCmds, " && ")
		cmd := fmt.Sprintf("cd '%s' && %s", wtPath, combined)
		tmux.NewWindowAutoclose(ctx.TmuxSession, winName, cmd)
		_ = exec.Command("tmux", "set-option", "-t",
			fmt.Sprintf("=%s:%s", ctx.TmuxSession, winName), "remain-on-exit", "on").Run()
		tmuxWindows = append(tmuxWindows, winName)
	}

	waitForSetupWindows(ctx.TmuxSession, tmuxWindows)
	return nil
}

func waitForSetupWindows(session string, windows []string) {
	if len(windows) == 0 {
		return
	}
	for {
		time.Sleep(2 * time.Second)
		stillRunning := false
		for _, w := range windows {
			out, err := exec.Command("tmux", "list-panes", "-t",
				fmt.Sprintf("=%s:%s", session, w), "-F", "#{pane_dead}").Output()
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
	for _, w := range windows {
		_ = exec.Command("tmux", "kill-window", "-t",
			fmt.Sprintf("=%s:%s", session, w)).Run()
	}
}

