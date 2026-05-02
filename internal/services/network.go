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

	// Port pool layout:
	//   40000-40099 → shared services (postgres, redis, minio, etc.)
	//   40100-49999 → workspace blocks (99 blocks × 100 ports each)
	SharedPortStart = 40000
	SharedPortEnd   = 40099
	WsPortStart     = 40100
	WsPortEnd       = 49999
	BlockSize       = 100
)

type NetworkState struct {
	Version int            `json:"version"`
	Blocks  map[string]int `json:"blocks"` // wsKey → block index (0-98)

	// Legacy (v2)
	Subnets     map[string]int    `json:"subnets,omitempty"`
	Allocations map[string]string `json:"allocations,omitempty"`
}

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
	return state
}

func newState() NetworkState {
	return NetworkState{Version: networkVersion, Blocks: make(map[string]int)}
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

// MaxWorkspaceBlocks returns how many branch workspaces can be created.
func MaxWorkspaceBlocks() int {
	return (WsPortEnd - WsPortStart + 1) / BlockSize // 99
}

// LoadIPAllocations returns workspace → display string for UI (no lock).
func LoadIPAllocations() map[string]string {
	state := LoadNetworkState()
	result := make(map[string]string)
	for wsKey, blockIdx := range state.Blocks {
		base := WsPortStart + blockIdx*BlockSize
		result[wsKey] = fmt.Sprintf(":%d+", base)
	}
	return result
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

// SharedServicePort returns a fixed port for a shared service (40000+index).
func SharedServicePort(svcIndex int) int {
	return SharedPortStart + svcIndex
}

// ── Workspace Port Blocks ──

// AllocateBlock assigns a port block for a branch workspace.
// Returns block base port (e.g., 40100, 40200, ...).
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

// WorkspacePort returns the allocated port for a service in a workspace.
// Main → originalPort. Branch → blockBase + svcIndex.
func WorkspacePort(wsKey string, svcIndex, originalPort int, defaultBranch string) int {
	if isMainWs(wsKey, defaultBranch) {
		return originalPort
	}
	state := LoadNetworkState()
	blockIdx, ok := state.Blocks[wsKey]
	if !ok {
		return originalPort
	}
	return WsPortStart + blockIdx*BlockSize + svcIndex
}

// ReleaseBlock releases a port block.
func ReleaseBlock(wsKey string) {
	WithIPLock(func() {
		state := LoadNetworkState()
		delete(state.Blocks, wsKey)
		saveNetworkState(&state)
	})
}

func isMainWs(wsKey, defaultBranch string) bool {
	return wsKey == "ws-"+defaultBranch || wsKey == "ws-main" || wsKey == "ws-master"
}

// ── Legacy Compat ──

func MainIP(session, defaultBranch string) string { return "127.0.0.1" }
func AllocateIP(session, worktreeKey string) string { return "127.0.0.1" }
func ReleaseIP(worktreeKey string) { ReleaseBlock(worktreeKey) }

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

// CheckEtcHosts returns hostnames missing from /etc/hosts.
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
