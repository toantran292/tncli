package pipeline

import (
	"fmt"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
)

// CreateContext holds all data needed for workspace creation pipeline.
type CreateContext struct {
	WorkspaceName   string
	Branch          string
	Config          *config.Config
	ConfigDir       string
	Session         string
	TmuxSession     string
	UniqueDirs      []string
	DirPaths        []services.DirMapping
	DirBranches     []services.DirBranch
	SharedOverrides []SharedOverrideEntry
	BindIP          string
	SkipStages      map[int]bool
	SelectedDirs    []services.DirBranch // (Name=dir, Branch=target) — nil if not set
}

type SharedOverrideEntry struct {
	DirName   string
	Overrides map[string]*config.ServiceOverride
	Hosts     []string
}

// DeleteContext holds all data needed for workspace deletion pipeline.
type DeleteContext struct {
	Branch       string
	Config       *config.Config
	ConfigDir    string
	CleanupItems []CleanupItem
	DBsToDrop    []DBDropItem
	Network      string
	SkipStages   map[int]bool
}

type CleanupItem struct {
	DirPath   string
	WtPath    string
	WtBranch  string
	PreDelete []string
}

type DBDropItem struct {
	Host     string
	Port     uint16
	DBName   string
	User     string
	Password string
}

// FromConfig builds CreateContext from Config.
func FromConfig(cfg *config.Config, configPath, wsName, branch string, skipStages map[int]bool) (*CreateContext, error) {
	workspaces := cfg.AllWorkspaces()
	entries, ok := workspaces[wsName]
	if !ok {
		return nil, fmt.Errorf("workspace '%s' not found", wsName)
	}

	var uniqueDirs []string
	seen := make(map[string]bool)
	for _, entry := range entries {
		d, _, err := cfg.FindServiceEntry(entry)
		if err != nil {
			continue
		}
		if !seen[d] {
			seen[d] = true
			uniqueDirs = append(uniqueDirs, d)
		}
	}
	if len(uniqueDirs) == 0 {
		return nil, fmt.Errorf("no dirs found in workspace '%s'", wsName)
	}

	configDir := filepath.Dir(configPath)
	defaultBranch := cfg.GlobalDefaultBranch()

	// Resolve dir paths
	var dirPaths []services.DirMapping
	for _, d := range uniqueDirs {
		resolved := d
		if !filepath.IsAbs(d) {
			wsPath := filepath.Join(configDir, "workspace--"+defaultBranch, d)
			if isDir(wsPath) {
				resolved = wsPath
			} else {
				resolved = filepath.Join(configDir, d)
			}
		}
		dirPaths = append(dirPaths, services.DirMapping{Name: d, Path: resolved})
	}

	// Resolve dir branches
	var dirBranches []services.DirBranch
	for _, d := range uniqueDirs {
		dirPath := ""
		for _, dp := range dirPaths {
			if dp.Name == d {
				dirPath = dp.Path
				break
			}
		}
		b := gitBranch(dirPath)
		if b == "" {
			b = "main"
		}
		dirBranches = append(dirBranches, services.DirBranch{Name: d, Branch: b})
	}

	// Resolve shared overrides
	var sharedOverrides []SharedOverrideEntry
	for _, d := range uniqueDirs {
		ov, hosts := ResolveSharedOverrides(cfg, d)
		sharedOverrides = append(sharedOverrides, SharedOverrideEntry{DirName: d, Overrides: ov, Hosts: hosts})
	}

	return &CreateContext{
		WorkspaceName:   wsName,
		Branch:          branch,
		Config:          cfg,
		ConfigDir:       configDir,
		Session:         cfg.Session,
		TmuxSession:     cfg.SvcSession(),
		UniqueDirs:      uniqueDirs,
		DirPaths:        dirPaths,
		DirBranches:     dirBranches,
		SharedOverrides: sharedOverrides,
		BindIP:          "",
		SkipStages:      skipStages,
	}, nil
}

// FromConfigWithSelection builds CreateContext with specific repo selection.
func FromConfigWithSelection(cfg *config.Config, configPath, wsName, branch string, selected []services.DirBranch) (*CreateContext, error) {
	ctx, err := FromConfig(cfg, configPath, wsName, branch, nil)
	if err != nil {
		return nil, err
	}

	selectedNames := make(map[string]bool)
	for _, s := range selected {
		selectedNames[s.Name] = true
	}

	// Filter
	var filteredDirs []string
	for _, d := range ctx.UniqueDirs {
		if selectedNames[d] {
			filteredDirs = append(filteredDirs, d)
		}
	}
	ctx.UniqueDirs = filteredDirs

	var filteredPaths []services.DirMapping
	for _, dp := range ctx.DirPaths {
		if selectedNames[dp.Name] {
			filteredPaths = append(filteredPaths, dp)
		}
	}
	ctx.DirPaths = filteredPaths

	var filteredBranches []services.DirBranch
	for _, db := range ctx.DirBranches {
		if selectedNames[db.Name] {
			filteredBranches = append(filteredBranches, db)
		}
	}
	ctx.DirBranches = filteredBranches

	var filteredOverrides []SharedOverrideEntry
	for _, so := range ctx.SharedOverrides {
		if selectedNames[so.DirName] {
			filteredOverrides = append(filteredOverrides, so)
		}
	}
	ctx.SharedOverrides = filteredOverrides

	ctx.SelectedDirs = selected
	ctx.SkipStages = nil
	return ctx, nil
}

// ResolveSharedOverrides resolves shared service overrides for a dir.
func ResolveSharedOverrides(cfg *config.Config, dirName string) (map[string]*config.ServiceOverride, []string) {
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
		host := cfg.SharedHost(sref.Name)
		if !contains(hosts, host) {
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

func gitBranch(dirPath string) string {
	out, err := exec.Command("git", "-C", dirPath, "rev-parse", "--abbrev-ref", "HEAD").Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

func isDir(path string) bool {
	return services.DirExists(path)
}

func contains(ss []string, s string) bool {
	return services.ContainsStr(ss, s)
}
