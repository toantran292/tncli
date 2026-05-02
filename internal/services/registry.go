package services

import (
	"encoding/json"
	"os"
	"path/filepath"
)

const registryFile = ".tncli/registry.json"

type Registry struct {
	Projects map[string]string `json:"projects"` // session name → project dir (absolute)
}

func registryPath() string { return homePath(registryFile) }

func LoadRegistry() Registry {
	data, err := os.ReadFile(registryPath())
	if err != nil {
		return Registry{Projects: make(map[string]string)}
	}
	var reg Registry
	if json.Unmarshal(data, &reg) != nil {
		return Registry{Projects: make(map[string]string)}
	}
	if reg.Projects == nil {
		reg.Projects = make(map[string]string)
	}
	return reg
}

func saveRegistry(reg *Registry) {
	path := registryPath()
	_ = os.MkdirAll(filepath.Dir(path), 0o755)
	data, _ := json.MarshalIndent(reg, "", "  ")
	tmp := path + ".tmp"
	if os.WriteFile(tmp, data, 0o644) == nil {
		_ = os.Rename(tmp, path)
	}
}

// RegisterProject registers a session → project dir mapping.
// Called when loading config (auto-register on every run).
func RegisterProject(session, projectDir string) {
	absDir, err := filepath.Abs(projectDir)
	if err != nil {
		return
	}
	reg := LoadRegistry()
	if reg.Projects[session] == absDir {
		return // already registered
	}
	reg.Projects[session] = absDir
	saveRegistry(&reg)
}

// ProjectDir returns the project directory for a session.
func ProjectDir(session string) (string, bool) {
	reg := LoadRegistry()
	dir, ok := reg.Projects[session]
	return dir, ok
}

// ListProjects returns all registered session → dir mappings.
func ListProjects() map[string]string {
	return LoadRegistry().Projects
}

// UnregisterProject removes a session from the registry.
func UnregisterProject(session string) {
	reg := LoadRegistry()
	delete(reg.Projects, session)
	saveRegistry(&reg)
}
