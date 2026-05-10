package config

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"gopkg.in/yaml.v3"
)

type Config struct {
	Session        string                       `yaml:"session"`
	DefaultBranch  string                       `yaml:"default_branch"`
	LocalPM        string                       `yaml:"local_pm"`
	Repos          map[string]*Dir              `yaml:"repos"`
	Presets        map[string]*PresetConfig     `yaml:"presets"`
	SharedServices map[string]*SharedServiceDef `yaml:"shared_services"`
	GlobalServices map[string]*GlobalService    `yaml:"global_services"`
	Workspaces     map[string][]string          `yaml:"workspaces"`
	Combinations   map[string][]string          `yaml:"combinations"`
	Environments   map[string]*EnvironmentDef   `yaml:"environments"`
	RawPreset      yaml.Node                    `yaml:"preset"`

	RepoOrder       []string            `yaml:"-"`
	WsServices      map[string]*Service `yaml:"-"` // workspace-level services from preset
	WsServiceOrder  []string            `yaml:"-"`
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
	EnvOutput        []EnvFileEntry              `yaml:"-"`
	RawEnvOutput     yaml.Node                   `yaml:"env_output"`
	Env              map[string]string            `yaml:"env"`
	SharedSvcRefs    []SharedServiceRef           `yaml:"-"`
	RawSharedSvcs    yaml.Node                    `yaml:"shared_services"`
	Databases        []string                     `yaml:"databases"`
	Preset           yaml.Node                    `yaml:"preset"`
	Presets_         []string                     `yaml:"-"`
	Setup            []string                     `yaml:"setup"`
	PreDelete        []string                     `yaml:"pre_delete"`

	ServiceOrder []string `yaml:"-"`
}

type EnvFileEntry struct {
	File string
	Env  map[string]string
}

type PresetConfig struct {
	Env       map[string]string   `yaml:"env"`
	Setup     []string            `yaml:"setup"`
	PreDelete []string            `yaml:"pre_delete"`
	Shortcuts []Shortcut          `yaml:"shortcuts"`
	Services  map[string]*Service `yaml:"services"`
}

type Service struct {
	Cmd       string            `yaml:"cmd"`
	Dir       string            `yaml:"dir"`
	ShellEnv  string            `yaml:"shell_env"`
	Env       map[string]string `yaml:"env"`
	PreStart  string            `yaml:"pre_start"`
	ProxyPort *uint16           `yaml:"proxy_port"`
	Shortcuts []Shortcut        `yaml:"shortcuts"`
	DependsOn []string          `yaml:"depends_on"`
	Port      *bool             `yaml:"port"`
	Modes     map[string]string `yaml:"modes"`
	Mode      string            `yaml:"mode"`
}

// ActiveCmd returns the cmd for the given mode, or the default cmd.
func (s *Service) ActiveCmd(modeOverride string) string {
	mode := modeOverride
	if mode == "" {
		mode = s.Mode
	}
	if mode != "" && s.Modes != nil {
		if cmd, ok := s.Modes[mode]; ok {
			return cmd
		}
	}
	return s.Cmd
}

// ModeNames returns available mode names sorted.
func (s *Service) ModeNames() []string {
	if len(s.Modes) == 0 {
		return nil
	}
	names := make([]string, 0, len(s.Modes))
	for k := range s.Modes {
		names = append(names, k)
	}
	sort.Strings(names)
	return names
}

func (s *Service) HasPort() bool {
	return s.Port != nil && *s.Port
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

type EnvironmentDef struct {
	Services map[string]string
}

func (e *EnvironmentDef) UnmarshalYAML(value *yaml.Node) error {
	if value.Kind != yaml.MappingNode {
		return fmt.Errorf("environment must be a mapping")
	}
	e.Services = make(map[string]string)
	for i := 0; i+1 < len(value.Content); i += 2 {
		e.Services[value.Content[i].Value] = value.Content[i+1].Value
	}
	return nil
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
	return "localhost"
}

func (c *Config) RemoteURL(envName, serviceName string) (string, bool) {
	if envName == "" || c.Environments == nil {
		return "", false
	}
	env, ok := c.Environments[envName]
	if !ok {
		return "", false
	}
	url, ok := env.Services[serviceName]
	return url, ok
}

func (c *Config) ValidateEnvironment(envName string) error {
	if envName == "" {
		return nil
	}
	if c.Environments == nil || c.Environments[envName] == nil {
		var names []string
		for n := range c.Environments {
			names = append(names, n)
		}
		if len(names) == 0 {
			return fmt.Errorf("environment '%s' not found (no environments defined)", envName)
		}
		return fmt.Errorf("environment '%s' not found (available: %s)", envName, strings.Join(names, ", "))
	}
	return nil
}

func (c *Config) EnvironmentNames() []string {
	var names []string
	for n := range c.Environments {
		names = append(names, n)
	}
	return names
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
	activeCmd := svc.ActiveCmd("")
	if activeCmd == "" {
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

	env := svc.ShellEnv
	if env == "" {
		env = dir.ShellEnv
	}
	preStart := svc.PreStart
	if preStart == "" {
		preStart = dir.PreStart
	}

	return &ResolvedService{
		Cmd:      activeCmd,
		WorkDir:  workDir,
		Env:      env,
		PreStart: preStart,
	}, nil
}

// TransformInstallCmd rewrites npm/yarn install commands to use the local
// package manager (e.g. pnpm) when local_pm is set. Setup commands get
// pnpm import prepended; shortcuts skip the import step.
func (c *Config) TransformInstallCmd(cmd string, withImport bool) string {
	if c.LocalPM != "pnpm" {
		return cmd
	}
	t := strings.TrimSpace(cmd)
	switch t {
	case "npm install", "npm i", "npm ci", "yarn", "yarn install":
		if withImport {
			return "pnpm import 2>/dev/null; pnpm install --shamefully-hoist"
		}
		return "pnpm install --shamefully-hoist"
	}
	return cmd
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
		len(d.Presets_) > 0 ||
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
		dir.Presets_ = parsePresetField(&dir.Preset)
		if dir.Services == nil {
			dir.Services = make(map[string]*Service)
		}
	}

	cfg.applyPresets()
	cfg.applyWsPresets()
	return cfg, nil
}

// applyWsPresets resolves top-level preset into workspace-level services.
func (c *Config) applyWsPresets() {
	names := parsePresetField(&c.RawPreset)
	c.WsServices = make(map[string]*Service)
	for _, name := range names {
		preset, ok := c.Presets[name]
		if !ok {
			continue
		}
		for svcName, svc := range preset.Services {
			if _, exists := c.WsServices[svcName]; !exists {
				c.WsServices[svcName] = svc
				c.WsServiceOrder = append(c.WsServiceOrder, svcName)
			}
		}
	}
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
		for _, presetName := range dir.Presets_ {
			preset, ok := c.Presets[presetName]
			if !ok {
				continue
			}
			for k, v := range preset.Env {
				if _, exists := dir.Env[k]; !exists {
					if dir.Env == nil {
						dir.Env = make(map[string]string)
					}
					dir.Env[k] = v
				}
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
			for name, svc := range preset.Services {
				if _, exists := dir.Services[name]; !exists {
					dir.Services[name] = svc
					dir.ServiceOrder = append(dir.ServiceOrder, name)
				}
			}
		}
	}
}
