package services

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/toantran292/tncli/internal/config"
)

func TestRegenerateWorkspaceEnv(t *testing.T) {
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

	// Create a fake workspace with .env file
	wsFolder := filepath.Join(projectDir, "workspace--main", "my-api")
	_ = os.MkdirAll(wsFolder, 0o755)
	_ = os.WriteFile(filepath.Join(wsFolder, ".env"), []byte("PORT=0\n"), 0o644)

	// Regenerate env should not panic or error
	RegenerateWorkspaceEnv(projectDir, cfg, "main")
}
