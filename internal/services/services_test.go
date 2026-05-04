package services

import (
	"fmt"
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

func TestFirstPort(t *testing.T) {
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
			if got := firstPortFromList(tt.ports); got != tt.want {
				t.Errorf("firstPortFromList(%v) = %d, want %d", tt.ports, got, tt.want)
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
	// Set up a temp project dir with InitNetwork so SharedPort works
	projectDir := t.TempDir()
	port := uint16(8080)
	cfg := &config.Config{
		Session: "test-resolve",
		SharedServices: map[string]*config.SharedServiceDef{
			"postgres": {
				Host:       "db.local",
				Ports:      []string{"5432"},
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
	InitNetwork(projectDir, "test-resolve", cfg)
	defer ReleaseSessionSlot("test-resolve")
	pgPort := SharedPort("postgres")

	tests := []struct {
		name  string
		input string
		want  string
	}{
		{"host shared", "{{host:postgres}}", "postgres"},
		{"host unknown", "{{host:unknown}}", "localhost"},
		{"port shared", "{{port:postgres}}", fmt.Sprintf("%d", pgPort)},
		{"port repo", "{{port:api}}", "8080"},
		{"url shared", "{{url:postgres}}", fmt.Sprintf("http://postgres:%d", pgPort)},
		{"conn", "{{conn:postgres}}", fmt.Sprintf("admin:secret@postgres:%d", pgPort)},
		{"multiple", "host={{host:postgres}} port={{port:postgres}}", fmt.Sprintf("host=postgres port=%d", pgPort)},
		{"no template", "plain text", "plain text"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := ResolveConfigTemplates(tt.input, cfg, "main", "ws-main")
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
	if MaxBlocks != 48 {
		t.Errorf("MaxBlocks = %d, want 48", MaxBlocks)
	}
	if (SlotSize-SharedReserve)/BlockSize != MaxBlocks {
		t.Error("MaxBlocks should equal (SlotSize-SharedReserve)/BlockSize")
	}
}

func TestSharedPort(t *testing.T) {
	projectDir := t.TempDir()
	cfg := &config.Config{
		Session: "test-shared",
		SharedServices: map[string]*config.SharedServiceDef{
			"postgres": {Ports: []string{"5432"}},
			"redis":    {Ports: []string{"6379"}},
			"minio":    {Ports: []string{"9000", "9090"}},
		},
	}
	InitNetwork(projectDir, "test-shared", cfg)
	defer ReleaseSessionSlot("test-shared")

	pgPort := SharedPort("postgres")
	slot := SessionSlot("test-shared")
	top := slotTop(slot)
	if pgPort > top || pgPort <= top-SharedReserve {
		t.Errorf("postgres port %d outside shared range (%d, %d]", pgPort, top-SharedReserve, top)
	}

	// Minio should have 2 consecutive ports
	minioPort0 := SharedPortAt("minio", 0)
	minioPort1 := SharedPortAt("minio", 1)
	if minioPort1 != minioPort0-1 {
		t.Errorf("minio ports not consecutive: %d, %d", minioPort0, minioPort1)
	}

	// All ports should be unique
	ports := map[int]string{
		pgPort:                   "postgres",
		SharedPort("redis"):      "redis",
		minioPort0:               "minio:0",
		minioPort1:               "minio:1",
	}
	if len(ports) != 4 {
		t.Errorf("shared ports have collisions: %v", ports)
	}
}

func TestContainerPort(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"19305:5432", "5432"},
		{"5432", "5432"},
		{"9000", "9000"},
		{"19309:9000", "9000"},
	}
	for _, tt := range tests {
		if got := ContainerPort(tt.input); got != tt.want {
			t.Errorf("ContainerPort(%q) = %q, want %q", tt.input, got, tt.want)
		}
	}
}
