package pipeline

import (
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
)

func findDirPath(ctx *CreateContext, dirName string) string {
	for _, dp := range ctx.DirPaths {
		if dp.Name == dirName {
			return dp.Path
		}
	}
	return ""
}

func resolveTargetBranch(ctx *CreateContext, dirName string) string {
	if ctx.SelectedDirs != nil {
		for _, sd := range ctx.SelectedDirs {
			if sd.Name == dirName {
				return sd.Branch
			}
		}
	}
	return ctx.Branch
}

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
		for i := range resolved {
			resolved[i].Value = services.ResolveDBTemplates(resolved[i].Value, dbNames)
		}
		services.ApplyEnvOverrides(dir, resolved, entry.File)
	}
}
