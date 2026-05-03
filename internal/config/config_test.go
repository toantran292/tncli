package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestSvcSession(t *testing.T) {
	cfg := &Config{Session: "myapp"}
	if got := cfg.SvcSession(); got != "tncli_myapp" {
		t.Errorf("SvcSession() = %q, want %q", got, "tncli_myapp")
	}
}

func TestGlobalDefaultBranch(t *testing.T) {
	tests := []struct {
		name   string
		branch string
		want   string
	}{
		{"empty defaults to main", "", "main"},
		{"explicit branch", "develop", "develop"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := &Config{DefaultBranch: tt.branch}
			if got := cfg.GlobalDefaultBranch(); got != tt.want {
				t.Errorf("GlobalDefaultBranch() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestDefaultBranchFor(t *testing.T) {
	cfg := &Config{
		DefaultBranch: "develop",
		Repos: map[string]*Dir{
			"api":    {DefaultBranch: "staging"},
			"client": {},
		},
	}
	tests := []struct {
		repo string
		want string
	}{
		{"api", "staging"},
		{"client", "develop"},
		{"unknown", "develop"},
	}
	for _, tt := range tests {
		t.Run(tt.repo, func(t *testing.T) {
			if got := cfg.DefaultBranchFor(tt.repo); got != tt.want {
				t.Errorf("DefaultBranchFor(%q) = %q, want %q", tt.repo, got, tt.want)
			}
		})
	}
}

func TestSharedHost(t *testing.T) {
	cfg := &Config{
		SharedServices: map[string]*SharedServiceDef{
			"postgres": {},
			"redis":    {},
		},
	}
	tests := []struct {
		name string
		want string
	}{
		{"postgres", "postgres"},
		{"redis", "redis"},
		{"unknown", "unknown"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := cfg.SharedHost(tt.name); got != tt.want {
				t.Errorf("SharedHost(%q) = %q, want %q", tt.name, got, tt.want)
			}
		})
	}
}

func TestFindServiceEntry(t *testing.T) {
	cfg := &Config{
		Repos: map[string]*Dir{
			"myapp-api": {
				Alias:    "api",
				Services: map[string]*Service{"server": {Cmd: "go run ."}},
			},
			"myapp-client": {
				Alias:    "client",
				Services: map[string]*Service{"dev": {Cmd: "npm run dev"}},
			},
		},
	}

	tests := []struct {
		entry   string
		wantDir string
		wantSvc string
		wantErr bool
	}{
		{"api/server", "myapp-api", "server", false},
		{"myapp-api/server", "myapp-api", "server", false},
		{"server", "myapp-api", "server", false},
		{"dev", "myapp-client", "dev", false},
		{"api/unknown", "", "", true},
		{"nonexistent", "", "", true},
	}
	for _, tt := range tests {
		t.Run(tt.entry, func(t *testing.T) {
			d, s, err := cfg.FindServiceEntry(tt.entry)
			if (err != nil) != tt.wantErr {
				t.Fatalf("FindServiceEntry(%q) error = %v, wantErr %v", tt.entry, err, tt.wantErr)
			}
			if d != tt.wantDir || s != tt.wantSvc {
				t.Errorf("FindServiceEntry(%q) = (%q, %q), want (%q, %q)", tt.entry, d, s, tt.wantDir, tt.wantSvc)
			}
		})
	}
}

func TestFindServiceEntryAmbiguous(t *testing.T) {
	cfg := &Config{
		Repos: map[string]*Dir{
			"repo-a": {Services: map[string]*Service{"server": {Cmd: "a"}}},
			"repo-b": {Services: map[string]*Service{"server": {Cmd: "b"}}},
		},
	}
	_, _, err := cfg.FindServiceEntry("server")
	if err == nil {
		t.Fatal("expected ambiguous error")
	}
}

func TestAllWorkspaces(t *testing.T) {
	t.Run("uses workspaces field", func(t *testing.T) {
		cfg := &Config{
			Session:    "test",
			Workspaces: map[string][]string{"dev": {"api/server", "client/dev"}},
		}
		ws := cfg.AllWorkspaces()
		if _, ok := ws["dev"]; !ok {
			t.Fatal("expected 'dev' workspace")
		}
	})

	t.Run("merges combinations", func(t *testing.T) {
		cfg := &Config{
			Session:      "test",
			Workspaces:   map[string][]string{"dev": {"a"}},
			Combinations: map[string][]string{"full": {"a", "b"}},
		}
		ws := cfg.AllWorkspaces()
		if len(ws) != 2 {
			t.Fatalf("expected 2 workspaces, got %d", len(ws))
		}
	})

	t.Run("auto-generates from repos", func(t *testing.T) {
		cfg := &Config{
			Session:   "myapp",
			RepoOrder: []string{"api"},
			Repos: map[string]*Dir{
				"api": {
					Alias:        "api",
					ServiceOrder: []string{"server"},
					Services:     map[string]*Service{"server": {Cmd: "go run ."}},
				},
			},
		}
		ws := cfg.AllWorkspaces()
		if entries, ok := ws["myapp"]; !ok {
			t.Fatal("expected auto-generated workspace")
		} else if len(entries) != 1 || entries[0] != "api/server" {
			t.Errorf("unexpected entries: %v", entries)
		}
	})
}

func TestIsGlobalService(t *testing.T) {
	cfg := &Config{
		GlobalServices: map[string]*GlobalService{"proxy": {Cmd: "caddy run"}},
	}
	if !cfg.IsGlobalService("proxy") {
		t.Error("expected proxy to be global")
	}
	if cfg.IsGlobalService("server") {
		t.Error("expected server to not be global")
	}
}

func TestApplyPresets(t *testing.T) {
	cfg := &Config{
		Presets: map[string]*PresetConfig{
			"node": {
				Setup:     []string{"npm install"},
				PreDelete: []string{"rm -rf node_modules"},
				Shortcuts: []Shortcut{{Cmd: "npm test", Desc: "run tests"}},
			},
		},
		Repos: map[string]*Dir{
			"client": {
				Preset:   "node",
				Services: map[string]*Service{},
			},
		},
	}
	cfg.applyPresets()

	dir := cfg.Repos["client"]
	if len(dir.Setup) != 1 || dir.Setup[0] != "npm install" {
		t.Errorf("expected setup from preset, got %v", dir.Setup)
	}
	if len(dir.PreDelete) != 1 || dir.PreDelete[0] != "rm -rf node_modules" {
		t.Errorf("expected pre_delete from preset, got %v", dir.PreDelete)
	}
	if len(dir.Shortcuts) != 1 {
		t.Errorf("expected shortcuts from preset, got %v", dir.Shortcuts)
	}
}

func TestEnvFileEntries(t *testing.T) {
	t.Run("default", func(t *testing.T) {
		d := &Dir{}
		entries := d.EnvFileEntries()
		if len(entries) != 1 || entries[0].File != ".env.local" {
			t.Errorf("expected default .env.local, got %v", entries)
		}
	})

	t.Run("custom", func(t *testing.T) {
		d := &Dir{
			EnvOutput: []EnvFileEntry{{File: ".env"}, {File: ".env.test"}},
		}
		entries := d.EnvFileEntries()
		if len(entries) != 2 {
			t.Errorf("expected 2 entries, got %d", len(entries))
		}
	})
}

func TestLoadYAML(t *testing.T) {
	yml := `
session: test-project
default_branch: main
repos:
  api:
    alias: api
    shell_env: .env
    services:
      server:
        cmd: "go run ./cmd/server"
      worker:
        cmd: "go run ./cmd/worker"
  client:
    alias: web
    services:
      dev:
        cmd: "npm run dev"
combinations:
  full:
    - api/server
    - api/worker
    - web/dev
shared_services:
  postgres:
    image: postgres:16
    host: 127.0.0.1
    ports:
      - "5432:5432"
    db_user: admin
    db_password: secret
`
	dir := t.TempDir()
	path := filepath.Join(dir, "tncli.yml")
	if err := os.WriteFile(path, []byte(yml), 0o644); err != nil {
		t.Fatal(err)
	}

	cfg, err := Load(path)
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}

	if cfg.Session != "test-project" {
		t.Errorf("Session = %q, want %q", cfg.Session, "test-project")
	}
	if cfg.GlobalDefaultBranch() != "main" {
		t.Errorf("DefaultBranch = %q, want %q", cfg.GlobalDefaultBranch(), "main")
	}
	if len(cfg.Repos) != 2 {
		t.Errorf("expected 2 repos, got %d", len(cfg.Repos))
	}
	if cfg.Repos["api"].Alias != "api" {
		t.Errorf("api alias = %q", cfg.Repos["api"].Alias)
	}
	if len(cfg.Repos["api"].Services) != 2 {
		t.Errorf("expected 2 api services, got %d", len(cfg.Repos["api"].Services))
	}

	// Check ordering preserved
	if len(cfg.RepoOrder) != 2 || cfg.RepoOrder[0] != "api" || cfg.RepoOrder[1] != "client" {
		t.Errorf("RepoOrder = %v, want [api, client]", cfg.RepoOrder)
	}
	if len(cfg.Repos["api"].ServiceOrder) != 2 {
		t.Errorf("api ServiceOrder = %v", cfg.Repos["api"].ServiceOrder)
	}

	// Check combinations
	if combo, ok := cfg.Combinations["full"]; !ok || len(combo) != 3 {
		t.Errorf("Combinations[full] = %v", combo)
	}

	// Check shared services
	pg := cfg.SharedServices["postgres"]
	if pg == nil {
		t.Fatal("postgres shared service not found")
	}
	if pg.DBUser != "admin" || pg.DBPassword != "secret" {
		t.Errorf("postgres creds = %s:%s", pg.DBUser, pg.DBPassword)
	}
}

func TestLoadYAMLWithWorktree(t *testing.T) {
	yml := `
session: test
repos:
  api:
    services:
      server:
        cmd: "go run ."
    copy:
      - .env.local
    databases:
      - mydb
    env_output:
      - .env
      - file: .env.local
        env:
          DB_HOST: localhost
    shared_services:
      - postgres
      - redis:
          db_name: cache_db
    setup:
      - npm install
presets:
  node:
    setup:
      - yarn install
`
	dir := t.TempDir()
	path := filepath.Join(dir, "tncli.yml")
	if err := os.WriteFile(path, []byte(yml), 0o644); err != nil {
		t.Fatal(err)
	}

	cfg, err := Load(path)
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}

	apiDir := cfg.Repos["api"]
	if !apiDir.HasWorktreeConfig() {
		t.Fatal("worktree config is empty")
	}
	if len(apiDir.Copy) != 1 || apiDir.Copy[0] != ".env.local" {
		t.Errorf("copy = %v", apiDir.Copy)
	}
	if len(apiDir.Databases) != 1 || apiDir.Databases[0] != "mydb" {
		t.Errorf("databases = %v", apiDir.Databases)
	}

	// env_output parsed
	if len(apiDir.EnvOutput) != 2 {
		t.Fatalf("expected 2 env_output, got %d", len(apiDir.EnvOutput))
	}
	if apiDir.EnvOutput[0].File != ".env" {
		t.Errorf("env_output[0] = %q", apiDir.EnvOutput[0].File)
	}
	if apiDir.EnvOutput[1].File != ".env.local" || apiDir.EnvOutput[1].Env["DB_HOST"] != "localhost" {
		t.Errorf("env_output[1] = %+v", apiDir.EnvOutput[1])
	}

	// shared_services parsed
	if len(apiDir.SharedSvcRefs) != 2 {
		t.Fatalf("expected 2 shared_services, got %d", len(apiDir.SharedSvcRefs))
	}
	if apiDir.SharedSvcRefs[0].Name != "postgres" || apiDir.SharedSvcRefs[0].DBName != "" {
		t.Errorf("shared_services[0] = %+v", apiDir.SharedSvcRefs[0])
	}
	if apiDir.SharedSvcRefs[1].Name != "redis" || apiDir.SharedSvcRefs[1].DBName != "cache_db" {
		t.Errorf("shared_services[1] = %+v", apiDir.SharedSvcRefs[1])
	}
}
