package services

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
)


// RegenerateWorkspaceEnv regenerates all env files + compose overrides for a workspace.
// Called automatically before starting services to ensure config changes are applied.
func RegenerateWorkspaceEnv(configDir string, cfg *config.Config, branch string) {
	wsKey := "ws-" + strings.ReplaceAll(branch, "/", "-")
	wsFolder := filepath.Join(configDir, "workspace--"+branch)

	if _, err := os.Stat(wsFolder); os.IsNotExist(err) {
		return
	}

	wsState := LoadWorkspaceState(wsFolder)

	// Ensure shared service slots are allocated for this workspace
	allocateSlots(cfg, wsKey)

	for _, dirName := range cfg.RepoOrder {
		dir := cfg.Repos[dirName]
		if dir == nil {
			continue
		}
		wtPath := filepath.Join(wsFolder, dirName)
		if _, err := os.Stat(wtPath); os.IsNotExist(err) {
			continue
		}

		if !dir.HasWorktreeConfig() {
			continue
		}

		alias := dir.Alias
		if alias == "" {
			alias = dirName
		}
		envName := ""
		if wsState.ServiceEnvs != nil {
			envName = wsState.ServiceEnvs[alias]
		}

		branchSafe := BranchSafe(branch)

		dbNames := make([]string, 0, len(dir.Databases))
		for _, tpl := range dir.Databases {
			name := strings.ReplaceAll(tpl, "{{branch_safe}}", branchSafe)
			name = strings.ReplaceAll(name, "{{branch}}", branch)
			dbNames = append(dbNames, cfg.Session+"_"+name)
		}

		baseEnv := make(map[string]string)
		for k, v := range dir.Env {
			baseEnv[k] = v
		}

		var skipDirs []string
		for _, svcName := range dir.ServiceOrder {
			if svc := dir.Services[svcName]; svc != nil && svc.Dir != "" {
				skipDirs = append(skipDirs, svc.Dir)
			}
		}

		for _, entry := range dir.EnvFileEntries() {
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
			resolved := ResolveEnvTemplates(envSrc, cfg, branchSafe, branch, wsKey, envName)
			for i := range resolved {
				resolved[i].Value = ResolveDBTemplates(resolved[i].Value, dbNames)
			}
			ApplyEnvOverrides(wtPath, resolved, entry.File, skipDirs...)
		}

		// Per-service env for monorepo services with dir (repo env + service env, no global)
		envFile := ".env.local"
		if entries := dir.EnvFileEntries(); len(entries) > 0 {
			envFile = entries[0].File
		}
		for _, svcName := range dir.ServiceOrder {
			svc := dir.Services[svcName]
			if svc == nil || svc.Dir == "" {
				continue
			}
			svcEnv := make(map[string]string)
			for k, v := range dir.Env {
				svcEnv[k] = v
			}
			for k, v := range svc.Env {
				svcEnv[k] = v
			}
			if len(svcEnv) == 0 {
				continue
			}
			// Per-service env override: check "alias/svc", fallback to "alias"
			svcEnvName := envName
			if wsState.ServiceEnvs != nil {
				if e, ok := wsState.ServiceEnvs[alias+"/"+svcName]; ok {
					svcEnvName = e
				} else if e, ok := wsState.ServiceEnvs[alias]; ok {
					svcEnvName = e
				}
			}
			resolved := ResolveEnvTemplates(svcEnv, cfg, branchSafe, branch, wsKey, svcEnvName)
			for i := range resolved {
				resolved[i].Value = ResolveDBTemplates(resolved[i].Value, dbNames)
			}
			svcDir := filepath.Join(wtPath, svc.Dir)
			ApplyEnvOverridesToDir(svcDir, resolved, envFile)
		}
	}
}

func allocateSlots(cfg *config.Config, wsKey string) {
	allocated := make(map[string]bool)

	// Scan all env sources for {{slot:SERVICE}} — global + per-repo
	var allEnvValues []string
	for _, dir := range cfg.Repos {
		if dir == nil {
			continue
		}
		for _, sref := range dir.SharedSvcRefs {
			if allocated[sref.Name] {
				continue
			}
			if svcDef, ok := cfg.SharedServices[sref.Name]; ok && svcDef.Capacity != nil {
				basePort := uint16(SharedPort(sref.Name))
				AllocateSlot(sref.Name, wsKey, *svcDef.Capacity, basePort)
				allocated[sref.Name] = true
			}
		}
		for _, v := range dir.Env {
			allEnvValues = append(allEnvValues, v)
		}
	}

	for _, val := range allEnvValues {
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
				if svcDef, ok := cfg.SharedServices[svcName]; ok && svcDef.Capacity != nil {
					basePort := uint16(SharedPort(svcName))
					AllocateSlot(svcName, wsKey, *svcDef.Capacity, basePort)
					allocated[svcName] = true
				}
			}
			s = s[start+end+2:]
		}
	}
}

