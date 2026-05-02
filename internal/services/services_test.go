package services

import (
	"testing"

	"github.com/toantran292/tncli/internal/config"
)

func TestBranchSafe(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"main", "main"},
		{"feature/login", "feature_login"},
		{"task-524", "task_524"},
		{"feature/task-524-fix", "feature_task_524_fix"},
		{"release/v1.0.0", "release_v1.0.0"},
	}
	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			if got := BranchSafe(tt.input); got != tt.want {
				t.Errorf("BranchSafe(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestExtractPortFromCmd(t *testing.T) {
	tests := []struct {
		cmd  string
		want uint16
	}{
		{"go run . --port 3000", 3000},
		{"npm run dev --port 8080", 8080},
		{"rails server -p 4000", 4000},
		{"./server --port=9090", 9090},
		{"go run .", 0},
		{"", 0},
	}
	for _, tt := range tests {
		t.Run(tt.cmd, func(t *testing.T) {
			if got := ExtractPortFromCmd(tt.cmd); got != tt.want {
				t.Errorf("ExtractPortFromCmd(%q) = %d, want %d", tt.cmd, got, tt.want)
			}
		})
	}
}

func TestFirstPortFromList(t *testing.T) {
	tests := []struct {
		name  string
		ports []string
		want  uint16
	}{
		{"host:container", []string{"5432:5432"}, 5432},
		{"host only", []string{"6379"}, 6379},
		{"multiple", []string{"3000:3000", "3001:3001"}, 3000},
		{"empty", nil, 0},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := FirstPortFromList(tt.ports); got != tt.want {
				t.Errorf("FirstPortFromList(%v) = %d, want %d", tt.ports, got, tt.want)
			}
		})
	}
}

func TestWorkspaceFolderPath(t *testing.T) {
	got := WorkspaceFolderPath("/home/user/project", "task-524")
	want := "/home/user/project/workspace--task-524"
	if got != want {
		t.Errorf("WorkspaceFolderPath() = %q, want %q", got, want)
	}
}

func TestResolveConfigTemplates(t *testing.T) {
	port := uint16(8080)
	cfg := &config.Config{
		SharedServices: map[string]*config.SharedServiceDef{
			"postgres": {
				Host:       "db.local",
				Ports:      []string{"5432:5432"},
				DBUser:     "admin",
				DBPassword: "secret",
			},
		},
		Repos: map[string]*config.Dir{
			"api": {
				Alias:     "api",
				ProxyPort: &port,
			},
		},
	}

	tests := []struct {
		name  string
		input string
		want  string
	}{
		{"host shared", "{{host:postgres}}", "db.local"},
		{"host unknown", "{{host:unknown}}", "127.0.0.1"},
		{"port shared", "{{port:postgres}}", "5432"},
		{"port repo", "{{port:api}}", "8080"},
		{"url shared", "{{url:postgres}}", "http://db.local:5432"},
		{"conn", "{{conn:postgres}}", "admin:secret@db.local:5432"},
		{"multiple", "host={{host:postgres}} port={{port:postgres}}", "host=db.local port=5432"},
		{"no template", "plain text", "plain text"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := ResolveConfigTemplates(tt.input, cfg, "main")
			if got != tt.want {
				t.Errorf("ResolveConfigTemplates(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestResolveDBTemplates(t *testing.T) {
	dbNames := []string{"myapp_main_api", "myapp_main_cache"}

	tests := []struct {
		input string
		want  string
	}{
		{"{{db:0}}", "myapp_main_api"},
		{"{{db:1}}", "myapp_main_cache"},
		{"{{db:2}}", ""},
		{"DB_NAME={{db:0}}", "DB_NAME=myapp_main_api"},
		{"no template", "no template"},
	}
	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got := ResolveDBTemplates(tt.input, dbNames)
			if got != tt.want {
				t.Errorf("ResolveDBTemplates(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestValidateBranchName(t *testing.T) {
	tests := []struct {
		branch  string
		wantErr bool
	}{
		{"main", false},
		{"feature/login", false},
		{"task-524", false},
		{"", true},
		{"..", true},
		{"../etc/passwd", true},
		{"/absolute", true},
		{"~/home", true},
	}
	for _, tt := range tests {
		t.Run(tt.branch, func(t *testing.T) {
			err := ValidateBranchName(tt.branch)
			if (err != nil) != tt.wantErr {
				t.Errorf("ValidateBranchName(%q) error = %v, wantErr %v", tt.branch, err, tt.wantErr)
			}
		})
	}
}

func TestFileExistsAndDirExists(t *testing.T) {
	dir := t.TempDir()
	if !DirExists(dir) {
		t.Error("DirExists should return true for temp dir")
	}
	if FileExists(dir) {
		t.Error("FileExists should return false for directory")
	}
	if FileExists(dir + "/nonexistent") {
		t.Error("FileExists should return false for nonexistent")
	}
}

func TestContainsStr(t *testing.T) {
	ss := []string{"a", "b", "c"}
	if !ContainsStr(ss, "b") {
		t.Error("expected true for 'b'")
	}
	if ContainsStr(ss, "d") {
		t.Error("expected false for 'd'")
	}
	if ContainsStr(nil, "a") {
		t.Error("expected false for nil slice")
	}
}

func TestNetworkConstants(t *testing.T) {
	if PoolEnd-PoolStart+1 != 10000 {
		t.Error("port pool should be 10000 ports")
	}
	if MaxBlocks != 50 {
		t.Errorf("MaxBlocks = %d, want 50", MaxBlocks)
	}
	if SlotSize/BlockSize != MaxBlocks {
		t.Error("MaxBlocks should equal SlotSize/BlockSize")
	}
}
