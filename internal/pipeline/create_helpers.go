package pipeline

import (
	"path/filepath"
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
	// SelectedDirs.Branch overrides the TARGET branch for a specific repo
	// e.g., workspace "feat-x" but client uses branch "main" directly
	if ctx.SelectedDirs != nil {
		for _, sd := range ctx.SelectedDirs {
			if sd.Name == dirName && sd.Branch != "" && sd.Branch != ctx.Branch {
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
	port := uint16(services.SharedPort("postgres"))
	if port == 0 {
		port = 5432
	}
	user := "postgres"
	pw := "postgres"
	if pgSvc != nil {
		if pgSvc.Host != "" {
			host = pgSvc.Host
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
		if dir == nil || !dir.HasWorktreeConfig() {
			continue
		}
		for _, sref := range dir.SharedSvcRefs {
			if sref.DBName != "" {
				dbName := strings.ReplaceAll(sref.DBName, "{{branch_safe}}", branchSafe)
				dbName = strings.ReplaceAll(dbName, "{{branch}}", branch)
				dbNames = append(dbNames, dbName)
			}
		}
		for _, dbTpl := range dir.Databases {
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

func applyAllEnvFiles(d *config.Dir, dirPath string, cfg *config.Config, branch, wsKey, envName string) {
	branchSafe := services.BranchSafe(branch)
	dbNames := make([]string, 0, len(d.Databases))
	for _, tpl := range d.Databases {
		name := strings.ReplaceAll(tpl, "{{branch_safe}}", branchSafe)
		name = strings.ReplaceAll(name, "{{branch}}", branch)
		dbNames = append(dbNames, cfg.Session+"_"+name)
	}

	baseEnv := make(map[string]string)
	for k, v := range d.Env {
		baseEnv[k] = v
	}

	// Collect service dirs to skip from findEnvDirs
	var skipDirs []string
	for _, svcName := range d.ServiceOrder {
		if svc := d.Services[svcName]; svc != nil && svc.Dir != "" {
			skipDirs = append(skipDirs, svc.Dir)
		}
	}

	for _, entry := range d.EnvFileEntries() {
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
		resolved := services.ResolveEnvTemplates(envSrc, cfg, branchSafe, branch, wsKey, envName)
		for i := range resolved {
			resolved[i].Value = services.ResolveDBTemplates(resolved[i].Value, dbNames)
		}
		services.ApplyEnvOverrides(dirPath, resolved, entry.File, skipDirs...)
	}

	// Per-service env for monorepo services with dir (repo env + service env, no global)
	envFile := ".env.local"
	if entries := d.EnvFileEntries(); len(entries) > 0 {
		envFile = entries[0].File
	}
	for _, svcName := range d.ServiceOrder {
		svc := d.Services[svcName]
		if svc == nil || svc.Dir == "" {
			continue
		}
		svcEnv := make(map[string]string)
		for k, v := range d.Env {
			svcEnv[k] = v
		}
		for k, v := range svc.Env {
			svcEnv[k] = v
		}
		if len(svcEnv) == 0 {
			continue
		}
		resolved := services.ResolveEnvTemplates(svcEnv, cfg, branchSafe, branch, wsKey, envName)
		for i := range resolved {
			resolved[i].Value = services.ResolveDBTemplates(resolved[i].Value, dbNames)
		}
		svcDir := filepath.Join(dirPath, svc.Dir)
		services.ApplyEnvOverridesToDir(svcDir, resolved, envFile)
	}
}
