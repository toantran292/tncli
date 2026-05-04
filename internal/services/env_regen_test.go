package services

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/toantran292/tncli/internal/config"
)

func TestRegenerateAndComposePorts(t *testing.T) {
	projectDir := t.TempDir()
	cfg := &config.Config{
		Session:   "test-regen",
		RepoOrder: []string{"my-api"},
		Repos: map[string]*config.Dir{
			"my-api": {
				Alias:        "api",
				ServiceOrder: []string{"server", "worker"},
				Services: map[string]*config.Service{
					"server": {Cmd: "go run ."},
					"worker": {Cmd: "go run ./worker"},
				},
			},
		},
		SharedServices: map[string]*config.SharedServiceDef{
			"postgres": {Ports: []string{"5432"}},
		},
	}
	InitNetwork(projectDir, "test-regen", cfg)
	defer ReleaseSessionSlot("test-regen")

	wsKey := "ws-main"
	ClaimBlock(projectDir, wsKey)

	// Verify ports are allocated
	serverPort := Port(projectDir, wsKey, "api~server")
	workerPort := Port(projectDir, wsKey, "api~worker")
	if serverPort == 0 || workerPort == 0 {
		t.Fatalf("ports not allocated: server=%d worker=%d", serverPort, workerPort)
	}
	if workerPort != serverPort+1 {
		t.Errorf("worker port %d should be server+1 (%d)", workerPort, serverPort+1)
	}
	t.Logf("server=%d worker=%d", serverPort, workerPort)

	// Create a fake workspace + docker-compose.yml
	wsFolder := filepath.Join(projectDir, "workspace--main", "my-api")
	_ = os.MkdirAll(wsFolder, 0o755)
	dcYml := `services:
  app:
    ports:
      - "3000:3000"
  worker:
    ports:
      - "3001:3001"
`
	_ = os.WriteFile(filepath.Join(wsFolder, "docker-compose.yml"), []byte(dcYml), 0o644)
	// Need a .env file for ApplyEnvOverrides to find keys
	_ = os.WriteFile(filepath.Join(wsFolder, ".env"), []byte("PORT=0\n"), 0o644)

	// Test compose override generation
	GenerateComposeOverride(ComposeOverrideOpts{
		RepoDir:      wsFolder,
		WorktreeDir:  wsFolder,
		ComposeFiles: []string{"docker-compose.yml"},
		Branch:       "main",
		WSKey:        wsKey,
		Config:       cfg,
		DirAlias:     "api",
	})

	// Read generated override
	data, err := os.ReadFile(filepath.Join(wsFolder, "docker-compose.override.yml"))
	if err != nil {
		t.Fatalf("no override generated: %v", err)
	}
	content := string(data)
	t.Log(content)

	// Check that dynamic ports are used, not original 3000/3001
	if strings.Contains(content, ":3000:3000") {
		t.Error("override still has original port 3000:3000, expected dynamic")
	}
	if strings.Contains(content, ":3001:3001") {
		t.Error("override still has original port 3001:3001, expected dynamic")
	}
	// Should contain the dynamic port mapping
	expected := fmt.Sprintf(":%d:3000", serverPort)
	if !strings.Contains(content, expected) {
		t.Errorf("expected port mapping %s in override", expected)
	}
}
