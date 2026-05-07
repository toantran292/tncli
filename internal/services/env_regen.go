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

		_ = WriteEnvFile(wtPath)

		if !dir.HasWorktreeConfig() {
			continue
		}

		branchSafe := BranchSafe(branch)

		dbNames := make([]string, 0, len(dir.Databases))
		for _, tpl := range dir.Databases {
			name := strings.ReplaceAll(tpl, "{{branch_safe}}", branchSafe)
			name = strings.ReplaceAll(name, "{{branch}}", branch)
			dbNames = append(dbNames, cfg.Session+"_"+name)
		}

		baseEnv := make(map[string]string)
		for k, v := range cfg.Env {
			baseEnv[k] = v
		}
		for k, v := range dir.Env {
			baseEnv[k] = v
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
			resolved := ResolveEnvTemplates(envSrc, cfg, branchSafe, branch, wsKey)
			for i := range resolved {
				resolved[i].Value = ResolveDBTemplates(resolved[i].Value, dbNames)
			}
			ApplyEnvOverrides(wtPath, resolved, entry.File)
		}

		// Regenerate docker-compose.override.yml
		if len(dir.ComposeFiles) > 0 {
			alias := dir.Alias
			if alias == "" {
				alias = dirName
			}
			repoDir := findMainRepoDir(configDir, dirName, cfg)
			ov, hosts := resolveOverrides(cfg, dirName)
			GenerateComposeOverride(ComposeOverrideOpts{
				RepoDir:          repoDir,
				WorktreeDir:      wtPath,
				ComposeFiles:     dir.ComposeFiles,
				WorktreeEnv:      dir.Env,
				Branch:           branch,
				NetworkName:      "",
				ServiceOverrides: ov,
				SharedHosts:      hosts,
				WSKey:            wsKey,
				Config:           cfg,
				Databases:        dir.Databases,
				DirAlias:         alias,
			})
		}
	}
}

func allocateSlots(cfg *config.Config, wsKey string) {
	allocated := make(map[string]bool)

	// Scan all env sources for {{slot:SERVICE}} — global + per-repo
	var allEnvValues []string
	for _, v := range cfg.Env {
		allEnvValues = append(allEnvValues, v)
	}
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

func findMainRepoDir(configDir, dirName string, cfg *config.Config) string {
	defaultBranch := cfg.GlobalDefaultBranch()
	wsPath := filepath.Join(configDir, "workspace--"+defaultBranch, dirName)
	if info, err := os.Stat(wsPath); err == nil && info.IsDir() {
		return wsPath
	}
	return filepath.Join(configDir, dirName)
}

func resolveOverrides(cfg *config.Config, dirName string) (map[string]*config.ServiceOverride, []string) {
	dir, ok := cfg.Repos[dirName]
	if !ok || dir == nil {
		return nil, nil
	}
	overrides := make(map[string]*config.ServiceOverride)
	for k, v := range dir.ServiceOverrides {
		overrides[k] = v
	}
	var hosts []string
	for _, sref := range dir.SharedSvcRefs {
		if _, ok := overrides[sref.Name]; !ok {
			overrides[sref.Name] = &config.ServiceOverride{
				Profiles: []string{"disabled"},
			}
		}
		if !ContainsStr(hosts, sref.Name) {
			hosts = append(hosts, sref.Name)
		}
	}
	for _, svcName := range dir.Disable {
		if _, ok := overrides[svcName]; !ok {
			overrides[svcName] = &config.ServiceOverride{
				Profiles: []string{"disabled"},
			}
		}
	}
	return overrides, hosts
}
