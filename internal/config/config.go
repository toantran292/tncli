package config

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"gopkg.in/yaml.v3"
)

type Config struct {
	Session        string                       `yaml:"session"`
	DefaultBranch  string                       `yaml:"default_branch"`
	Repos          map[string]*Dir              `yaml:"repos"`
	Env            map[string]string            `yaml:"env"`
	Presets        map[string]*PresetConfig     `yaml:"presets"`
	SharedServices map[string]*SharedServiceDef `yaml:"shared_services"`
	GlobalServices map[string]*GlobalService    `yaml:"global_services"`
	Workspaces     map[string][]string          `yaml:"workspaces"`
	Combinations   map[string][]string          `yaml:"combinations"`

	RepoOrder []string `yaml:"-"`
}

type Dir struct {
	Alias         string              `yaml:"alias"`
	PreStart      string              `yaml:"pre_start"`
	ShellEnv      string              `yaml:"shell_env"`
	DefaultBranch string              `yaml:"default_branch"`
	Shortcuts     []Shortcut          `yaml:"shortcuts"`
	Services      map[string]*Service `yaml:"services"`
	ProxyPort     *uint16             `yaml:"proxy_port"`

	// Worktree fields (flat)
	Copy             []string                    `yaml:"copy"`
	ComposeFiles     []string                    `yaml:"compose_files"`
	EnvOutput        []EnvFileEntry              `yaml:"-"`
	RawEnvOutput     yaml.Node                   `yaml:"env_output"`
	Env              map[string]string            `yaml:"env"`
	ServiceOverrides map[string]*ServiceOverride `yaml:"service_overrides"`
	Disable          []string                     `yaml:"disable"`
	SharedSvcRefs    []SharedServiceRef           `yaml:"-"`
	RawSharedSvcs    yaml.Node                    `yaml:"shared_services"`
	Databases        []string                     `yaml:"databases"`
	Preset           string                       `yaml:"preset"`
	Setup            []string                     `yaml:"setup"`
	PreDelete        []string                     `yaml:"pre_delete"`

	ServiceOrder []string `yaml:"-"`
}

type EnvFileEntry struct {
	File string
	Env  map[string]string
}

type PresetConfig struct {
	Setup     []string   `yaml:"setup"`
	PreDelete []string   `yaml:"pre_delete"`
	Shortcuts []Shortcut `yaml:"shortcuts"`
}

type Service struct {
	Cmd       string            `yaml:"cmd"`
	Env       string            `yaml:"env"`
	EnvVars   map[string]string `yaml:"env_vars"`
	PreStart  string            `yaml:"pre_start"`
	ProxyPort *uint16           `yaml:"proxy_port"`
	Shortcuts []Shortcut        `yaml:"shortcuts"`
	DependsOn []string          `yaml:"depends_on"`
	Port      *bool             `yaml:"port"`
}

func (s *Service) HasPort() bool {
	return s.Port == nil || *s.Port
}

func (s *Service) UnmarshalYAML(value *yaml.Node) error {
	if value.Kind == yaml.ScalarNode {
		s.Cmd = value.Value
		return nil
	}
	type serviceAlias Service
	var alias serviceAlias
	if err := value.Decode(&alias); err != nil {
		return err
	}
	*s = Service(alias)
	return nil
}

type GlobalService struct {
	Cmd           string `yaml:"cmd"`
	WorktreeLevel bool   `yaml:"worktree_level"`
}

type ServiceOverride struct {
	Environment map[string]string `yaml:"environment"`
	Profiles    []string          `yaml:"profiles"`
	MemLimit    string            `yaml:"mem_limit"`
}

type SharedServiceDef struct {
	Image       string            `yaml:"image"`
	Host        string            `yaml:"host"`
	Ports       []string          `yaml:"ports"`
	Environment map[string]string `yaml:"environment"`
	Volumes     []string          `yaml:"volumes"`
	Command     string            `yaml:"command"`
	Healthcheck *HealthCheck      `yaml:"healthcheck"`
	DBUser      string            `yaml:"db_user"`
	DBPassword  string            `yaml:"db_password"`
	Capacity    *uint16           `yaml:"capacity"`
}

type HealthCheck struct {
	Test     yaml.Node `yaml:"test"`
	Interval string    `yaml:"interval"`
	Timeout  string    `yaml:"timeout"`
	Retries  int       `yaml:"retries"`
}

type SharedServiceRef struct {
	Name   string
	DBName string
}

type Shortcut struct {
	Cmd  string `yaml:"cmd"`
	Desc string `yaml:"desc"`
}

type ResolvedService struct {
	Cmd      string
	WorkDir  string
	Env      string
	PreStart string
}

// ── Config Methods ──

func (c *Config) SvcSession() string {
	return "tncli_" + c.Session
}

func (c *Config) GlobalDefaultBranch() string {
	if c.DefaultBranch != "" {
		return c.DefaultBranch
	}
	return "main"
}

func (c *Config) DefaultBranchFor(repoName string) string {
	if dir, ok := c.Repos[repoName]; ok && dir.DefaultBranch != "" {
		return dir.DefaultBranch
	}
	return c.GlobalDefaultBranch()
}

func (c *Config) SharedHost(serviceName string) string {
	return serviceName
}

func (c *Config) IsGlobalService(svcName string) bool {
	_, ok := c.GlobalServices[svcName]
	return ok
}

func (c *Config) AllServicesFor(dirName string) []string {
	var svcs []string
	if dir, ok := c.Repos[dirName]; ok {
		for _, name := range dir.ServiceOrder {
			svcs = append(svcs, name)
		}
	}
	for name := range c.GlobalServices {
		found := false
		for _, s := range svcs {
			if s == name {
				found = true
				break
			}
		}
		if !found {
			svcs = append(svcs, name)
		}
	}
	return svcs
}

func (c *Config) WorktreeLevelServices() []struct{ Name, Cmd string } {
	var result []struct{ Name, Cmd string }
	for name, gs := range c.GlobalServices {
		if gs.WorktreeLevel {
			result = append(result, struct{ Name, Cmd string }{name, gs.Cmd})
		}
	}
	return result
}

func (c *Config) AllWorkspaces() map[string][]string {
	result := make(map[string][]string)
	for k, v := range c.Workspaces {
		result[k] = v
	}
	for k, v := range c.Combinations {
		if _, exists := result[k]; !exists {
			result[k] = v
		}
	}
	if len(result) == 0 {
		var entries []string
		for _, dirName := range c.RepoOrder {
			dir := c.Repos[dirName]
			alias := dir.Alias
			for _, svcName := range dir.ServiceOrder {
				if alias == "" {
					entries = append(entries, svcName)
				} else {
					entries = append(entries, alias+"/"+svcName)
				}
			}
		}
		if len(entries) > 0 {
			result[c.Session] = entries
		}
	}
	return result
}

func (c *Config) FindServiceEntry(entry string) (dirName, svcName string, err error) {
	if prefix, svc, ok := strings.Cut(entry, "/"); ok {
		for dn, dir := range c.Repos {
			matches := dn == prefix || dir.Alias == prefix
			if matches {
				if _, exists := dir.Services[svc]; exists {
					return dn, svc, nil
				}
			}
		}
		return "", "", fmt.Errorf("service '%s' not found", entry)
	}

	var matches []string
	for dn, dir := range c.Repos {
		if _, exists := dir.Services[entry]; exists {
			matches = append(matches, dn)
		}
	}
	switch len(matches) {
	case 0:
		return "", "", fmt.Errorf("service '%s' not found in any dir", entry)
	case 1:
		return matches[0], entry, nil
	default:
		return "", "", fmt.Errorf("ambiguous service '%s' — found in: %s", entry, strings.Join(matches, ", "))
	}
}

func (c *Config) FindServiceEntryQuiet(entry string) (string, string, bool) {
	d, s, err := c.FindServiceEntry(entry)
	return d, s, err == nil
}

func (c *Config) ResolveServices(target string) ([][2]string, error) {
	all := c.AllWorkspaces()
	if entries, ok := all[target]; ok {
		var result [][2]string
		for _, entry := range entries {
			d, s, err := c.FindServiceEntry(entry)
			if err != nil {
				return nil, err
			}
			result = append(result, [2]string{d, s})
		}
		return result, nil
	}

	if dir, ok := c.Repos[target]; ok && len(dir.Services) > 0 {
		var result [][2]string
		for _, svc := range dir.ServiceOrder {
			result = append(result, [2]string{target, svc})
		}
		return result, nil
	}

	for dn, dir := range c.Repos {
		if dir.Alias == target && len(dir.Services) > 0 {
			var result [][2]string
			for _, svc := range dir.ServiceOrder {
				result = append(result, [2]string{dn, svc})
			}
			return result, nil
		}
	}

	d, s, err := c.FindServiceEntry(target)
	if err != nil {
		return nil, err
	}
	return [][2]string{{d, s}}, nil
}

func (c *Config) ResolveService(configDir, dirName, svcName string) (*ResolvedService, error) {
	dir, ok := c.Repos[dirName]
	if !ok {
		return nil, fmt.Errorf("dir '%s' not found", dirName)
	}
	svc, ok := dir.Services[svcName]
	if !ok {
		return nil, fmt.Errorf("service '%s' not found in dir '%s'", svcName, dirName)
	}
	if svc.Cmd == "" {
		return nil, fmt.Errorf("service '%s/%s' has no 'cmd'", dirName, svcName)
	}

	workDir := dirName
	if !filepath.IsAbs(dirName) {
		wsPath := filepath.Join(configDir, fmt.Sprintf("workspace--%s", c.GlobalDefaultBranch()), dirName)
		if info, err := os.Stat(wsPath); err == nil && info.IsDir() {
			workDir = wsPath
		} else {
			workDir = filepath.Join(configDir, dirName)
		}
	}

	env := svc.Env
	if env == "" {
		env = dir.ShellEnv
	}
	preStart := svc.PreStart
	if preStart == "" {
		preStart = dir.PreStart
	}

	return &ResolvedService{
		Cmd:      svc.Cmd,
		WorkDir:  workDir,
		Env:      env,
		PreStart: preStart,
	}, nil
}

// ── Dir methods ──

// EnvFileEntries returns env output file entries, falling back to [".env.local"].
func (d *Dir) EnvFileEntries() []EnvFileEntry {
	if len(d.EnvOutput) == 0 {
		return []EnvFileEntry{{File: ".env.local"}}
	}
	return d.EnvOutput
}

// HasWorktreeConfig returns true if any worktree-level config is defined.
func (d *Dir) HasWorktreeConfig() bool {
	return len(d.Copy) > 0 || len(d.Setup) > 0 || len(d.Databases) > 0 ||
		len(d.ComposeFiles) > 0 || len(d.Disable) > 0 || d.Preset != "" ||
		len(d.PreDelete) > 0 || len(d.Env) > 0 || len(d.EnvOutput) > 0 ||
		len(d.SharedSvcRefs) > 0
}

// ── Loading ──

func Load(path string) (*Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read %s: %w", path, err)
	}

	var raw yaml.Node
	if err := yaml.Unmarshal(data, &raw); err != nil {
		return nil, fmt.Errorf("failed to parse %s: %w", path, err)
	}

	cfg := &Config{
		Session: "tncli",
		Repos:   make(map[string]*Dir),
	}
	if err := yaml.Unmarshal(data, cfg); err != nil {
		return nil, fmt.Errorf("failed to parse %s: %w", path, err)
	}
	if cfg.Session == "" {
		cfg.Session = "tncli"
	}

	extractRepoOrder(cfg, &raw)

	for _, dir := range cfg.Repos {
		dir.EnvOutput = parseEnvFiles(&dir.RawEnvOutput)
		dir.SharedSvcRefs = parseSharedRefs(&dir.RawSharedSvcs)
		if dir.Services == nil {
			dir.Services = make(map[string]*Service)
		}
	}

	cfg.applyPresets()
	return cfg, nil
}

func FindConfig() (string, error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", err
	}
	for {
		candidate := filepath.Join(dir, "tncli.yml")
		if info, err := os.Stat(candidate); err == nil && !info.IsDir() {
			return candidate, nil
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", fmt.Errorf("no tncli.yml found (searched from current directory to /)")
		}
		dir = parent
	}
}

// ── Internal ──

func (c *Config) applyPresets() {
	for _, dir := range c.Repos {
		if dir.Preset == "" {
			continue
		}
		preset, ok := c.Presets[dir.Preset]
		if !ok {
			continue
		}
		if len(dir.Setup) == 0 {
			dir.Setup = preset.Setup
		}
		if len(dir.PreDelete) == 0 {
			dir.PreDelete = preset.PreDelete
		}
		if len(dir.Shortcuts) == 0 {
			dir.Shortcuts = preset.Shortcuts
		}
	}
}
