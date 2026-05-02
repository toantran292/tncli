package commands

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/pipeline"
	"github.com/toantran292/tncli/internal/services"
)

func WorkspaceCreate(cfg *config.Config, cfgPath, workspace, branch string, fromStage int, repos string) error {
	services.MigrateLegacyIPs()

	skipStages := make(map[int]bool)
	if fromStage > 1 {
		for i := 0; i < fromStage-1; i++ {
			skipStages[i] = true
		}
	}

	var selectedDirs []services.DirBranch
	if repos != "" {
		for _, entry := range strings.Split(repos, ",") {
			parts := strings.SplitN(entry, ":", 2)
			if len(parts) == 2 {
				selectedDirs = append(selectedDirs, services.DirBranch{Name: parts[0], Branch: parts[1]})
			} else {
				selectedDirs = append(selectedDirs, services.DirBranch{Name: parts[0], Branch: branch})
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
		return err
	}

	ch := make(chan pipeline.Event, 16)
	go pipeline.RunCreatePipeline(ctx, ch)

	for evt := range ch {
		switch evt.Type {
		case pipeline.EventStageStarted:
			fmt.Printf("%s>>>%s [%d/%d] %s\n", Blue, NC, evt.Index+1, evt.Total, evt.Name)
		case pipeline.EventStageCompleted:
			fmt.Printf("    %sdone%s\n", Green, NC)
		case pipeline.EventStageSkipped:
			label := pipeline.AllCreateStages[evt.Index].Label()
			fmt.Printf("%s    skipped: %s%s\n", Dim, label, NC)
		case pipeline.EventPipelineCompleted:
			configDir := filepath.Dir(cfgPath)
			fmt.Printf("\n%sWorkspace ready:%s BIND_IP=%s\n", Green, NC, ctx.BindIP)
			fmt.Printf("  cd %s/workspace--%s\n", configDir, branch)
			return nil
		case pipeline.EventPipelineFailed:
			return fmt.Errorf("failed at stage %d: %s\nRetry: tncli workspace create %s %s --from-stage %d",
				evt.Index+1, evt.Error, workspace, branch, evt.Index+1)
		}
	}
	return nil
}

func WorkspaceDelete(cfg *config.Config, cfgPath, branch string) error {
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

		wtPath := filepath.Join(configDir, "workspace--"+branch, dirName)
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
			pgSvc := FindPGService(cfg)
			pgHost := cfg.SharedHost("postgres")
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
		Branch: branch, Config: cfg, ConfigDir: configDir,
		CleanupItems: cleanupItems, DBsToDrop: dbsToDrop,
		Network: "tncli-ws-" + branch,
	}

	ch := make(chan pipeline.Event, 16)
	go pipeline.RunDeletePipeline(ctx, ch)

	for evt := range ch {
		switch evt.Type {
		case pipeline.EventStageStarted:
			fmt.Printf("%s>>>%s [%d/%d] %s\n", Blue, NC, evt.Index+1, evt.Total, evt.Name)
		case pipeline.EventStageCompleted:
			fmt.Printf("    %sdone%s\n", Green, NC)
		case pipeline.EventPipelineCompleted:
			fmt.Printf("\n%sWorkspace '%s' deleted%s\n", Green, branch, NC)
			return nil
		case pipeline.EventPipelineFailed:
			return fmt.Errorf("delete failed at stage %d: %s", evt.Index+1, evt.Error)
		}
	}
	return nil
}

func WorkspaceList(cfg *config.Config, cfgPath string) {
	workspaces := cfg.AllWorkspaces()
	configDir := filepath.Dir(cfgPath)
	ipAllocs := services.LoadIPAllocations()

	fmt.Printf("%sWorkspace definitions:%s\n", Bold, NC)
	for name, entries := range workspaces {
		fmt.Printf("  %s%s%s: %s\n", Bold, name, NC, strings.Join(entries, ", "))
	}

	var wsBranches []string
	entries, _ := os.ReadDir(configDir)
	for _, e := range entries {
		if branch, ok := strings.CutPrefix(e.Name(), "workspace--"); ok {
			wsBranches = append(wsBranches, branch)
		}
	}

	if len(wsBranches) == 0 {
		fmt.Printf("\n%sNo active workspace instances%s\n", Dim, NC)
		return
	}

	for _, branch := range wsBranches {
		wsKey := "ws-" + branch
		ip := ipAllocs[wsKey]
		if ip == "" {
			ip = "?"
		}
		fmt.Printf("\n%sWorkspace: %s%s%s %s(%s)%s\n", Green, Bold, branch, NC, Dim, ip, NC)

		wsFolder := filepath.Join(configDir, "workspace--"+branch)
		for _, dirName := range cfg.RepoOrder {
			dir := cfg.Repos[dirName]
			if _, err := os.Stat(filepath.Join(wsFolder, dirName)); os.IsNotExist(err) {
				continue
			}
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
				if p := services.ExtractPortFromCmd(cmd); p > 0 {
					fmt.Printf("    %s%s%s → %s:%d  %s%s%s\n", Cyan, svcName, NC, ip, p, Dim, cmd, NC)
				} else {
					fmt.Printf("    %s%s%s  %s%s%s\n", Cyan, svcName, NC, Dim, cmd, NC)
				}
			}
		}
	}

	if len(cfg.SharedServices) > 0 {
		fmt.Printf("\n%sShared services:%s\n", Bold, NC)
		for name, svc := range cfg.SharedServices {
			host := svc.Host
			if host == "" {
				host = "localhost"
			}
			fmt.Printf("  %s%s%s: %s [%s] %s(%s)%s\n", Cyan, name, NC, host, strings.Join(svc.Ports, ", "), Dim, svc.Image, NC)
		}
	}
}

func FindPGService(cfg *config.Config) *config.SharedServiceDef {
	for _, svc := range cfg.SharedServices {
		if svc.DBUser != "" {
			return svc
		}
	}
	return nil
}
