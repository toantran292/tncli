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
	networkStateFile = ".tncli/network.json"
	networkVersion   = 3

	SharedPortStart = 40000
	SharedPortEnd   = 40099
	WsPortStart     = 40100
	WsPortEnd       = 49999
	BlockSize       = 100
)

type NetworkState struct {
	Version     int            `json:"version"`
	Blocks      map[string]int `json:"blocks"`       // wsKey → block index
	SharedPorts map[string]int `json:"shared_ports"`  // svc name → port
	ServiceMap  map[string]int `json:"service_map"`   // svc key → index within block

	// Legacy v2
	Subnets     map[string]int    `json:"subnets,omitempty"`
	Allocations map[string]string `json:"allocations,omitempty"`
}

// ── Paths + I/O ──

func homePath(rel string) string {
	home, _ := os.UserHomeDir()
	if home == "" {
		home = "/tmp"
	}
	return filepath.Join(home, rel)
}

func statePath() string { return homePath(networkStateFile) }

func LoadNetworkState() NetworkState {
	data, err := os.ReadFile(statePath())
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

func saveNetworkState(state *NetworkState) {
	path := statePath()
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

func WithIPLock(fn func()) {
	lockPath := homePath(".tncli/network.lock")
	_ = os.MkdirAll(filepath.Dir(lockPath), 0o755)

	deadline := time.Now().Add(30 * time.Second)
	for {
		f, err := os.OpenFile(lockPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
		if err == nil {
			fmt.Fprintf(f, "%d", os.Getpid())
			f.Close()
			break
		}
		if time.Now().After(deadline) {
			_ = os.Remove(lockPath)
			continue
		}
		if info, err := os.Stat(lockPath); err == nil {
			if time.Since(info.ModTime()) > 10*time.Second {
				if data, err := os.ReadFile(lockPath); err == nil {
					var pid int
					if _, err := fmt.Sscanf(string(data), "%d", &pid); err != nil || !isProcessAlive(pid) {
						_ = os.Remove(lockPath)
						continue
					}
				}
			}
		}
		time.Sleep(50 * time.Millisecond)
	}
	defer os.Remove(lockPath)
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

// EnsureSharedPort assigns a fixed port for a shared service (40000+).
// Idempotent — returns existing port if already assigned.
func EnsureSharedPort(svcName string) int {
	var port int
	WithIPLock(func() {
		state := LoadNetworkState()
		if p, ok := state.SharedPorts[svcName]; ok {
			port = p
			return
		}
		// Next available in shared range
		used := make(map[int]bool)
		for _, p := range state.SharedPorts {
			used[p] = true
		}
		for p := SharedPortStart; p <= SharedPortEnd; p++ {
			if !used[p] {
				state.SharedPorts[svcName] = p
				saveNetworkState(&state)
				port = p
				return
			}
		}
		fmt.Fprintf(os.Stderr, "warning: shared port pool exhausted\n")
	})
	return port
}

// GetSharedPort returns existing shared port (0 if not assigned).
func GetSharedPort(svcName string) int {
	return LoadNetworkState().SharedPorts[svcName]
}

// ── Service Map (fixed index per service key) ──

// EnsureServiceIndex assigns a stable index to a service key (repo~svc).
// Used to compute: workspace_port = block_base + service_index.
func EnsureServiceIndex(svcKey string) int {
	var idx int
	WithIPLock(func() {
		state := LoadNetworkState()
		if i, ok := state.ServiceMap[svcKey]; ok {
			idx = i
			return
		}
		// Next available index
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
		saveNetworkState(&state)
	})
	return idx
}

// GetServiceIndex returns existing index (-1 if not assigned).
func GetServiceIndex(svcKey string) int {
	if i, ok := LoadNetworkState().ServiceMap[svcKey]; ok {
		return i
	}
	return -1
}

// ── Workspace Port Blocks ──

// AllocateBlock assigns a port block for a branch workspace.
// Main workspace returns 0 (uses original ports).
func AllocateBlock(wsKey, defaultBranch string) int {
	if isMainWs(wsKey, defaultBranch) {
		return 0
	}
	var base int
	WithIPLock(func() {
		state := LoadNetworkState()
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
				saveNetworkState(&state)
				base = WsPortStart + i*BlockSize
				return
			}
		}
		fmt.Fprintf(os.Stderr, "warning: workspace port pool exhausted (%d max)\n", MaxWorkspaceBlocks())
	})
	return base
}

// WorkspacePort returns the port for a service in a workspace.
// Main → originalPort. Branch → blockBase + serviceIndex.
func WorkspacePort(wsKey, svcKey string, originalPort int, defaultBranch string) int {
	if isMainWs(wsKey, defaultBranch) {
		return originalPort
	}
	state := LoadNetworkState()
	blockIdx, ok := state.Blocks[wsKey]
	if !ok {
		return originalPort
	}
	svcIdx, ok := state.ServiceMap[svcKey]
	if !ok {
		return originalPort
	}
	return WsPortStart + blockIdx*BlockSize + svcIdx
}

// ReleaseBlock releases a port block.
func ReleaseBlock(wsKey string) {
	WithIPLock(func() {
		state := LoadNetworkState()
		delete(state.Blocks, wsKey)
		saveNetworkState(&state)
	})
}

// LoadIPAllocations returns workspace → display string for UI.
func LoadIPAllocations() map[string]string {
	state := LoadNetworkState()
	result := make(map[string]string)
	for wsKey, blockIdx := range state.Blocks {
		base := WsPortStart + blockIdx*BlockSize
		result[wsKey] = fmt.Sprintf(":%d+", base)
	}
	return result
}

func isMainWs(wsKey, defaultBranch string) bool {
	return wsKey == "ws-"+defaultBranch || wsKey == "ws-main" || wsKey == "ws-master"
}

// ── Legacy Compat ──

func MainIP(session, defaultBranch string) string  { return "127.0.0.1" }
func AllocateIP(session, worktreeKey string) string { return "127.0.0.1" }
func ReleaseIP(worktreeKey string)                  { ReleaseBlock(worktreeKey) }

func MigrateLegacyIPs() {
	state := LoadNetworkState()
	if state.Version >= networkVersion {
		return
	}
	WithIPLock(func() {
		state := LoadNetworkState()
		if state.Version >= networkVersion {
			return
		}
		state.Subnets = nil
		state.Allocations = nil
		state.Version = networkVersion
		saveNetworkState(&state)
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
