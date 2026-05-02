package commands

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/paths"
	"github.com/toantran292/tncli/internal/services"
)

func Migrate(cfg *config.Config, cfgPath string) error {
	configDir := filepath.Dir(cfgPath)
	tncliDir := paths.StateDir()

	fmt.Printf("%s[1/6] Cleaning old state files%s\n", Bold, NC)
	cleaned := cleanOldStateFiles(tncliDir)
	for _, f := range cleaned {
		fmt.Printf("  %sremoved%s %s\n", Dim, NC, f)
	}
	if len(cleaned) == 0 {
		fmt.Printf("  %snothing to clean%s\n", Dim, NC)
	}

	fmt.Printf("\n%s[2/6] Migrating network state%s\n", Bold, NC)
	migrateNetworkState(tncliDir, configDir, cfg)

	fmt.Printf("\n%s[3/6] /etc/hosts for shared services%s\n", Bold, NC)
	if len(cfg.SharedServices) > 0 {
		setupEtcHosts(cfg)
	} else {
		fmt.Printf("  %sno shared services%s\n", Dim, NC)
	}

	fmt.Printf("\n%s[4/6] Regenerating shared services compose%s\n", Bold, NC)
	if len(cfg.SharedServices) > 0 {
		services.GenerateSharedCompose(configDir, cfg.Session, cfg.SharedServices)
		fmt.Printf("  %s>>>%s docker-compose.shared.yml (dynamic ports + tncli-shared network)\n", Green, NC)
	} else {
		fmt.Printf("  %sno shared services%s\n", Dim, NC)
	}

	fmt.Printf("\n%s[5/6] Regenerating env files for existing workspaces%s\n", Bold, NC)
	regenerated := regenerateWorkspaceEnvs(configDir, cfg)
	if regenerated == 0 {
		fmt.Printf("  %sno workspaces found%s\n", Dim, NC)
	}

	fmt.Printf("\n%s[6/6] Global gitignore%s\n", Bold, NC)
	services.EnsureGlobalGitignore()
	fmt.Printf("  %s>>>%s configured\n", Green, NC)

	fmt.Printf("\n%sMigration complete!%s\n", Green, NC)
	if len(cfg.SharedServices) > 0 {
		fmt.Printf("\nRestart shared services to apply new ports:\n")
		fmt.Printf("  docker compose -f %s/docker-compose.shared.yml -p %s-shared up -d\n", configDir, cfg.Session)
	}
	return nil
}

func cleanOldStateFiles(tncliDir string) []string {
	var cleaned []string
	oldFiles := []string{
		"Caddyfile",
		"proxy-routes.json",
		"proxy.pid",
		"proxy.log",
		"setup-loopback.sh",
	}
	for _, f := range oldFiles {
		path := filepath.Join(tncliDir, f)
		if _, err := os.Stat(path); err == nil {
			_ = os.Remove(path)
			cleaned = append(cleaned, f)
		}
	}

	// Clean stale pipeline files
	entries, _ := os.ReadDir(tncliDir)
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), "pipeline-") && strings.HasSuffix(e.Name(), ".json") {
			path := filepath.Join(tncliDir, e.Name())
			_ = os.Remove(path)
			cleaned = append(cleaned, e.Name())
		}
	}

	// Clean active directory
	activeDir := filepath.Join(tncliDir, "active")
	if entries, err := os.ReadDir(activeDir); err == nil {
		for _, e := range entries {
			_ = os.Remove(filepath.Join(activeDir, e.Name()))
			cleaned = append(cleaned, "active/"+e.Name())
		}
	}

	return cleaned
}

func migrateNetworkState(tncliDir, configDir string, cfg *config.Config) {
	globalPath := filepath.Join(tncliDir, "network.json")

	// Check if global network.json is v2 (old format with "version" key)
	if data, err := os.ReadFile(globalPath); err == nil {
		var raw map[string]interface{}
		if json.Unmarshal(data, &raw) == nil {
			if _, hasVersion := raw["version"]; hasVersion {
				_ = os.Remove(globalPath)
				fmt.Printf("  %sremoved%s old global network.json (v2 IP-based)\n", Dim, NC)
			}
		}
	}

	// Check project-level network.json
	projectPath := filepath.Join(configDir, ".tncli", "network.json")
	if data, err := os.ReadFile(projectPath); err == nil {
		var raw map[string]interface{}
		if json.Unmarshal(data, &raw) == nil {
			if _, hasVersion := raw["version"]; hasVersion {
				_ = os.Remove(projectPath)
				fmt.Printf("  %sremoved%s old project network.json (v2 IP-based)\n", Dim, NC)
			}
		}
	}

	// Re-init network with new format
	services.InitNetwork(configDir, cfg.Session, cfg)
	fmt.Printf("  %s>>>%s new network state initialized (slot-based ports)\n", Green, NC)
}

func regenerateWorkspaceEnvs(configDir string, cfg *config.Config) int {
	count := 0
	entries, _ := os.ReadDir(configDir)
	for _, e := range entries {
		branch, ok := strings.CutPrefix(e.Name(), "workspace--")
		if !ok || !e.IsDir() {
			continue
		}

		wsFolder := filepath.Join(configDir, e.Name())
		repoCount := 0
		for _, dirName := range cfg.RepoOrder {
			dir := cfg.Repos[dirName]
			if dir == nil {
				continue
			}
			wtPath := filepath.Join(wsFolder, dirName)
			if _, err := os.Stat(wtPath); os.IsNotExist(err) {
				continue
			}

			// Regenerate .env.tncli
			_ = services.WriteEnvFile(wtPath)

			// Regenerate env files
			if dir.WT() != nil {
				wsKey := "ws-" + strings.ReplaceAll(branch, "/", "-")
				migrateApplyEnvFiles(dir.WT(), wtPath, cfg, branch, wsKey)
			}

			// Regenerate docker-compose.override.yml
			if dir.WT() != nil && len(dir.WT().ComposeFiles) > 0 {
				repoDir := findRepoDirForMigrate(configDir, dirName, cfg)
				ov, hosts := findSharedOverridesForMigrate(cfg, dirName)
				services.GenerateComposeOverride(services.ComposeOverrideOpts{
					RepoDir:          repoDir,
					WorktreeDir:      wtPath,
					ComposeFiles:     dir.WT().ComposeFiles,
					WorktreeEnv:      dir.WT().Env,
					Branch:           branch,
					NetworkName:      "tncli-ws-" + branch,
					ServiceOverrides: ov,
					SharedHosts:      hosts,
					WSKey:            "ws-" + strings.ReplaceAll(branch, "/", "-"),
					Config:           cfg,
					Databases:        dir.WT().Databases,
				})
			}

			repoCount++
			count++
		}
		fmt.Printf("  %s>>>%s %s (%d repos)\n", Green, NC, branch, repoCount)
	}
	return count
}

func migrateApplyEnvFiles(wt *config.WorktreeConfig, dir string, cfg *config.Config, branch, wsKey string) {
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
		resolved := services.ResolveEnvTemplates(envSrc, cfg, branchSafe, branch, wsKey)
		for i := range resolved {
			resolved[i].Value = services.ResolveDBTemplates(resolved[i].Value, dbNames)
		}
		services.ApplyEnvOverrides(dir, resolved, entry.File)
	}
}

func findRepoDirForMigrate(configDir, dirName string, cfg *config.Config) string {
	defaultBranch := cfg.GlobalDefaultBranch()
	wsPath := filepath.Join(configDir, "workspace--"+defaultBranch, dirName)
	if info, err := os.Stat(wsPath); err == nil && info.IsDir() {
		return wsPath
	}
	return filepath.Join(configDir, dirName)
}

func findSharedOverridesForMigrate(cfg *config.Config, dirName string) (map[string]*config.ServiceOverride, []string) {
	dir, ok := cfg.Repos[dirName]
	if !ok || dir.WT() == nil {
		return nil, nil
	}
	wt := dir.WT()
	overrides := make(map[string]*config.ServiceOverride)
	for k, v := range wt.ServiceOverrides {
		overrides[k] = v
	}
	var hosts []string
	for _, sref := range wt.SharedServices {
		if _, ok := overrides[sref.Name]; !ok {
			overrides[sref.Name] = &config.ServiceOverride{
				Profiles: []string{"disabled"},
			}
		}
		host := sref.Name
		if !services.ContainsStr(hosts, host) {
			hosts = append(hosts, host)
		}
	}
	for _, svcName := range wt.Disable {
		if _, ok := overrides[svcName]; !ok {
			overrides[svcName] = &config.ServiceOverride{
				Profiles: []string{"disabled"},
			}
		}
	}
	return overrides, hosts
}

func countDirs(wsFolder string, cfg *config.Config) int {
	count := 0
	for _, dirName := range cfg.RepoOrder {
		if _, err := os.Stat(filepath.Join(wsFolder, dirName)); err == nil {
			count++
		}
	}
	return count
}
