package services

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

const (
	networkVersion = 3

	SharedPortStart = 40000
	SharedPortEnd   = 40099
	WsPortStart     = 40100
	WsPortEnd       = 49999
	BlockSize       = 100
)

// NetworkState is per-project, stored at <project>/.tncli/network.json.
type NetworkState struct {
	Version     int            `json:"version"`
	Blocks      map[string]int `json:"blocks"`       // wsKey → block index
	SharedPorts map[string]int `json:"shared_ports"`  // shared svc name → port
	ServiceMap  map[string]int `json:"service_map"`   // svc key → index within block
}

// ── Paths + I/O ──

func homePath(rel string) string {
	home, _ := os.UserHomeDir()
	if home == "" {
		home = "/tmp"
	}
	return filepath.Join(home, rel)
}

func networkPath(projectDir string) string {
	return filepath.Join(projectDir, ".tncli", "network.json")
}

func lockPath(projectDir string) string {
	return filepath.Join(projectDir, ".tncli", "network.lock")
}

func LoadNetworkState(projectDir string) NetworkState {
	data, err := os.ReadFile(networkPath(projectDir))
	if err != nil {
		return newState()
	}
	var state NetworkState
	if json.Unmarshal(data, &state) != nil {
		return newState()
	}
	if state.Blocks == nil {
		state.Blocks = make(map[string]int)
	}
	if state.SharedPorts == nil {
		state.SharedPorts = make(map[string]int)
	}
	if state.ServiceMap == nil {
		state.ServiceMap = make(map[string]int)
	}
	return state
}

func newState() NetworkState {
	return NetworkState{
		Version:     networkVersion,
		Blocks:      make(map[string]int),
		SharedPorts: make(map[string]int),
		ServiceMap:  make(map[string]int),
	}
}

func saveNetworkState(projectDir string, state *NetworkState) {
	path := networkPath(projectDir)
	_ = os.MkdirAll(filepath.Dir(path), 0o755)
	data, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return
	}
	tmp := path + ".tmp"
	if os.WriteFile(tmp, data, 0o644) == nil {
		_ = os.Rename(tmp, path)
	}
}

func MaxWorkspaceBlocks() int {
	return (WsPortEnd - WsPortStart + 1) / BlockSize
}

// ── File Lock ──

func WithProjectLock(projectDir string, fn func()) {
	lp := lockPath(projectDir)
	_ = os.MkdirAll(filepath.Dir(lp), 0o755)

	deadline := time.Now().Add(30 * time.Second)
	for {
		f, err := os.OpenFile(lp, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
		if err == nil {
			fmt.Fprintf(f, "%d", os.Getpid())
			f.Close()
			break
		}
		if time.Now().After(deadline) {
			_ = os.Remove(lp)
			continue
		}
		if info, err := os.Stat(lp); err == nil {
			if time.Since(info.ModTime()) > 10*time.Second {
				if data, err := os.ReadFile(lp); err == nil {
					var pid int
					if _, err := fmt.Sscanf(string(data), "%d", &pid); err != nil || !isProcessAlive(pid) {
						_ = os.Remove(lp)
						continue
					}
				}
			}
		}
		time.Sleep(50 * time.Millisecond)
	}
	defer os.Remove(lp)
	fn()
}

func isProcessAlive(pid int) bool {
	if pid <= 0 {
		return false
	}
	p, err := os.FindProcess(pid)
	if err != nil {
		return false
	}
	return p.Signal(syscall.Signal(0)) == nil
}

// ── Shared Service Ports ──

// EnsureSharedPort assigns a fixed port for a shared service.
func EnsureSharedPort(projectDir, svcName string) int {
	var port int
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		if p, ok := state.SharedPorts[svcName]; ok {
			port = p
			return
		}
		used := make(map[int]bool)
		for _, p := range state.SharedPorts {
			used[p] = true
		}
		for p := SharedPortStart; p <= SharedPortEnd; p++ {
			if !used[p] {
				state.SharedPorts[svcName] = p
				saveNetworkState(projectDir, &state)
				port = p
				return
			}
		}
		fmt.Fprintf(os.Stderr, "warning: shared port pool exhausted\n")
	})
	return port
}

// GetSharedPort returns existing shared port (0 if not assigned).
func GetSharedPort(projectDir, svcName string) int {
	return LoadNetworkState(projectDir).SharedPorts[svcName]
}

// ── Service Map ──

// EnsureServiceIndex assigns a stable index to a service key.
// svcKey format: "alias~svcname" (e.g., "api~api", "client~portal").
func EnsureServiceIndex(projectDir, svcKey string) int {
	var idx int
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		if i, ok := state.ServiceMap[svcKey]; ok {
			idx = i
			return
		}
		maxIdx := -1
		for _, i := range state.ServiceMap {
			if i > maxIdx {
				maxIdx = i
			}
		}
		idx = maxIdx + 1
		if idx >= BlockSize {
			fmt.Fprintf(os.Stderr, "warning: service map full (%d max)\n", BlockSize)
			return
		}
		state.ServiceMap[svcKey] = idx
		saveNetworkState(projectDir, &state)
	})
	return idx
}

// ── Workspace Port Blocks ──

// AllocateBlock assigns a port block for a workspace.
func AllocateBlock(projectDir, wsKey string) int {
	var base int
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		if blockIdx, ok := state.Blocks[wsKey]; ok {
			base = WsPortStart + blockIdx*BlockSize
			return
		}
		used := make(map[int]bool)
		for _, idx := range state.Blocks {
			used[idx] = true
		}
		for i := 0; i < MaxWorkspaceBlocks(); i++ {
			if !used[i] {
				state.Blocks[wsKey] = i
				saveNetworkState(projectDir, &state)
				base = WsPortStart + i*BlockSize
				return
			}
		}
		fmt.Fprintf(os.Stderr, "warning: workspace port pool exhausted (%d max)\n", MaxWorkspaceBlocks())
	})
	return base
}

// WorkspacePort returns the allocated port for a service in a workspace.
func WorkspacePort(projectDir, wsKey, svcKey string) int {
	state := LoadNetworkState(projectDir)
	blockIdx, ok := state.Blocks[wsKey]
	if !ok {
		return 0
	}
	svcIdx, ok := state.ServiceMap[svcKey]
	if !ok {
		return 0
	}
	return WsPortStart + blockIdx*BlockSize + svcIdx
}

// ReleaseBlock releases a port block.
func ReleaseBlock(projectDir, wsKey string) {
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		delete(state.Blocks, wsKey)
		saveNetworkState(projectDir, &state)
	})
}

// LoadIPAllocations returns workspace → display string for UI.
func LoadIPAllocations(projectDir string) map[string]string {
	state := LoadNetworkState(projectDir)
	result := make(map[string]string)
	for wsKey, blockIdx := range state.Blocks {
		base := WsPortStart + blockIdx*BlockSize
		result[wsKey] = fmt.Sprintf(":%d+", base)
	}
	return result
}

// ── Legacy Compat ──

func MainIP(session, defaultBranch string) string  { return "127.0.0.1" }
func AllocateIP(session, worktreeKey string) string { return "127.0.0.1" }

// ReleaseIP releases port block — needs projectDir now.
func ReleaseIP(projectDir, worktreeKey string) { ReleaseBlock(projectDir, worktreeKey) }

func MigrateLegacyIPs(projectDir string) {
	state := LoadNetworkState(projectDir)
	if state.Version >= networkVersion {
		return
	}
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		if state.Version >= networkVersion {
			return
		}
		state.Version = networkVersion
		saveNetworkState(projectDir, &state)
	})
}

func CheckEtcHosts(hostnames []string) []string {
	content, _ := os.ReadFile("/etc/hosts")
	contentStr := string(content)
	var missing []string
	for _, h := range hostnames {
		if !strings.Contains(contentStr, h) {
			missing = append(missing, h)
		}
	}
	return missing
}
