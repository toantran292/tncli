package commands

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/paths"
	"github.com/toantran292/tncli/internal/services"
)

func Migrate(cfg *config.Config, cfgPath string) error {
	configDir := filepath.Dir(cfgPath)

	fmt.Printf("%s[1/8] XDG directory migration%s\n", Bold, NC)
	migrateXDG()

	tncliDir := paths.StateDir()

	fmt.Printf("\n%s[2/8] Cleaning old state files%s\n", Bold, NC)
	cleaned := cleanOldStateFiles(tncliDir)
	for _, f := range cleaned {
		fmt.Printf("  %sremoved%s %s\n", Dim, NC, f)
	}
	if len(cleaned) == 0 {
		fmt.Printf("  %snothing to clean%s\n", Dim, NC)
	}

	fmt.Printf("\n%s[3/8] Migrating network state%s\n", Bold, NC)
	migrateNetworkState(tncliDir, configDir, cfg)

	fmt.Printf("\n%s[4/8] Cleaning stale slot allocations%s\n", Bold, NC)
	cleanStaleSlots(configDir, cfg)

	fmt.Printf("\n%s[5/8] Cleaning old system config (sudo)%s\n", Bold, NC)
	cleanOldSystemConfig()

	fmt.Printf("\n%s[6/8] /etc/hosts for shared services (sudo)%s\n", Bold, NC)
	if len(cfg.SharedServices) > 0 {
		setupEtcHosts(cfg)
	} else {
		fmt.Printf("  %sno shared services%s\n", Dim, NC)
	}

	fmt.Printf("\n%s[7/8] Regenerating shared services + workspace configs%s\n", Bold, NC)
	if len(cfg.SharedServices) > 0 {
		services.GenerateSharedCompose(configDir, cfg.Session, cfg.SharedServices)
		fmt.Printf("  %s>>>%s docker-compose.shared.yml\n", Green, NC)
	}
	regenerated := regenerateWorkspaceEnvs(configDir, cfg)
	if regenerated == 0 {
		fmt.Printf("  %sno workspaces found%s\n", Dim, NC)
	}

	fmt.Printf("\n%s[8/8] Global gitignore%s\n", Bold, NC)
	services.EnsureGlobalGitignore()
	fmt.Printf("  %s>>>%s configured\n", Green, NC)

	if len(cfg.SharedServices) > 0 {
		fmt.Printf("\n%sRestarting shared services with new ports...%s\n", Bold, NC)
		composeFile := filepath.Join(configDir, "docker-compose.shared.yml")
		project := cfg.Session + "-shared"
		down := exec.Command("docker", "compose", "-f", composeFile, "-p", project, "down")
		down.Dir = configDir
		_ = down.Run()
		up := exec.Command("docker", "compose", "-f", composeFile, "-p", project, "up", "-d")
		up.Dir = configDir
		if err := up.Run(); err != nil {
			fmt.Printf("  %sfailed:%s %v\n", Yellow, NC, err)
		} else {
			fmt.Printf("  %s>>>%s shared services restarted\n", Green, NC)
		}
	}

	fmt.Printf("\n%sMigration complete!%s\n", Green, NC)
	return nil
}

func migrateXDG() {
	if paths.MigrateFromLegacy() {
		fmt.Printf("  %s>>>%s migrated ~/.tncli/ → %s (symlink left)\n", Green, NC, paths.StateDir())
	} else {
		fmt.Printf("  %sstate dir:%s %s\n", Dim, NC, paths.StateDir())
	}
}

func cleanOldStateFiles(tncliDir string) []string {
	var cleaned []string
	oldFiles := []string{
		"Caddyfile",
		"proxy-routes.json",
		"proxy.pid",
		"proxy.log",
		"setup-loopback.sh",
		"network.json",
	}
	for _, f := range oldFiles {
		path := filepath.Join(tncliDir, f)
		if _, err := os.Stat(path); err == nil {
			// For network.json, only remove if v2 format
			if f == "network.json" {
				if !isV2NetworkJSON(path) {
					continue
				}
			}
			_ = os.Remove(path)
			cleaned = append(cleaned, f)
		}
	}

	// Clean stale pipeline files
	entries, _ := os.ReadDir(tncliDir)
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), "pipeline-") && strings.HasSuffix(e.Name(), ".json") {
			_ = os.Remove(filepath.Join(tncliDir, e.Name()))
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

func isV2NetworkJSON(path string) bool {
	data, err := os.ReadFile(path)
	if err != nil {
		return false
	}
	var raw map[string]interface{}
	if json.Unmarshal(data, &raw) != nil {
		return false
	}
	_, hasVersion := raw["version"]
	return hasVersion
}

func migrateNetworkState(tncliDir, configDir string, cfg *config.Config) {
	// Remove project-level network.json to force rebuild with correct offsets
	projectPath := filepath.Join(configDir, ".tncli", "network.json")
	if _, err := os.Stat(projectPath); err == nil {
		_ = os.Remove(projectPath)
		fmt.Printf("  %sremoved%s project network.json (will rebuild)\n", Dim, NC)
	}

	// Reset global slots (stale session leases)
	slotsPath := paths.StatePath("slots.json")
	_ = os.WriteFile(slotsPath, []byte(`{"slots":{}}`+"\n"), 0o644)
	fmt.Printf("  %s>>>%s reset session slots\n", Green, NC)

	// Re-init network with new format
	services.InitNetwork(configDir, cfg.Session, cfg)
	fmt.Printf("  %s>>>%s network state initialized (slot-based ports)\n", Green, NC)
}

func cleanStaleSlots(configDir string, cfg *config.Config) {
	// Find which workspaces actually exist
	existing := make(map[string]bool)
	entries, _ := os.ReadDir(configDir)
	for _, e := range entries {
		if branch, ok := strings.CutPrefix(e.Name(), "workspace--"); ok && e.IsDir() {
			existing["ws-"+strings.ReplaceAll(branch, "/", "-")] = true
		}
	}

	allocs := services.LoadSlotAllocations()
	changed := false
	for svcName, svc := range allocs {
		for wsKey := range svc.Slots {
			if !existing[wsKey] {
				delete(svc.Slots, wsKey)
				changed = true
				fmt.Printf("  %sremoved%s stale slot: %s/%s\n", Dim, NC, svcName, wsKey)
			}
		}
	}
	if changed {
		data, _ := json.MarshalIndent(allocs, "", "  ")
		_ = os.WriteFile(paths.StatePath("shared_slots.json"), data, 0o644)
	}
	if !changed {
		fmt.Printf("  %sno stale slots%s\n", Dim, NC)
	}
}

func cleanOldSystemConfig() {
	cleaned := 0

	// Remove /etc/resolver/tncli.test (dnsmasq)
	if _, err := os.Stat("/etc/resolver/tncli.test"); err == nil {
		cmd := exec.Command("sudo", "rm", "/etc/resolver/tncli.test")
		cmd.Stdin = os.Stdin
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		if cmd.Run() == nil {
			fmt.Printf("  %sremoved%s /etc/resolver/tncli.test (dnsmasq)\n", Dim, NC)
			cleaned++
		}
	}

	// Remove old /etc/hosts entries (.tncli.test, *.local)
	if hasOldHostsEntries() {
		cmd := exec.Command("sudo", "sed", "-i", "", "/.tncli.test/d", "/etc/hosts")
		cmd.Stdin = os.Stdin
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		if cmd.Run() == nil {
			fmt.Printf("  %sremoved%s /etc/hosts *.tncli.test entries\n", Dim, NC)
			cleaned++
		}
	}

	// Remove loopback aliases (127.0.1.x, 127.0.2.x)
	out, _ := exec.Command("ifconfig", "lo0").Output()
	for _, line := range strings.Split(string(out), "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "inet 127.0.") && !strings.HasPrefix(line, "inet 127.0.0.") {
			parts := strings.Fields(line)
			if len(parts) >= 2 {
				ip := parts[1]
				cmd := exec.Command("sudo", "ifconfig", "lo0", "-alias", ip)
				cmd.Stdin = os.Stdin
				cmd.Stdout = os.Stdout
				cmd.Stderr = os.Stderr
				if cmd.Run() == nil {
					fmt.Printf("  %sremoved%s loopback alias %s\n", Dim, NC, ip)
					cleaned++
				}
			}
		}
	}

	if cleaned == 0 {
		fmt.Printf("  %snothing to clean%s\n", Dim, NC)
	}
}

func hasOldHostsEntries() bool {
	data, _ := os.ReadFile("/etc/hosts")
	return strings.Contains(string(data), ".tncli.test")
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

			_ = services.WriteEnvFile(wtPath)

			if dir.WT() != nil {
				wsKey := "ws-" + strings.ReplaceAll(branch, "/", "-")
				migrateApplyEnvFiles(dir.WT(), wtPath, cfg, branch, wsKey)
			}

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
		if !services.ContainsStr(hosts, sref.Name) {
			hosts = append(hosts, sref.Name)
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
