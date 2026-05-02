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

	// Port pool for branch workspaces. Main workspace uses original ports.
	// Each workspace gets a block of BlockSize ports.
	PortPoolStart = 40000
	PortPoolEnd   = 49999
	BlockSize     = 100 // max services per workspace
)

// NetworkState holds port block allocations for all workspaces.
type NetworkState struct {
	Version    int            `json:"version"`
	Blocks     map[string]int `json:"blocks"`      // wsKey → block index (0, 1, 2, ...)
	NextBlock  int            `json:"next_block"`

	// Legacy (v2) — kept for migration
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
		return newNetworkState()
	}
	var state NetworkState
	if json.Unmarshal(data, &state) != nil {
		return newNetworkState()
	}
	if state.Blocks == nil {
		state.Blocks = make(map[string]int)
	}
	return state
}

func newNetworkState() NetworkState {
	return NetworkState{
		Version: networkVersion,
		Blocks:  make(map[string]int),
	}
}

// MaxBlocks returns how many workspace blocks fit in the port pool.
func MaxBlocks() int {
	return (PortPoolEnd - PortPoolStart + 1) / BlockSize
}

// saveNetworkState writes state atomically (write tmp → rename).
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

// LoadIPAllocations returns workspace → display string for UI (no lock).
func LoadIPAllocations() map[string]string {
	state := LoadNetworkState()
	result := make(map[string]string)
	for wsKey, blockIdx := range state.Blocks {
		base := PortPoolStart + blockIdx*BlockSize
		result[wsKey] = fmt.Sprintf(":%d-%d", base, base+BlockSize-1)
	}
	return result
}

// ── File Lock ──

// WithIPLock provides file-lock protected operation.
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

// ── Port Block Allocation ──

// AllocateBlock assigns a port block to a workspace. Returns block base port.
// Main workspace returns 0 (uses original ports).
func AllocateBlock(wsKey string, defaultBranch string) int {
	// Main workspace uses original ports
	if isMainWs(wsKey, defaultBranch) {
		return 0
	}

	var base int
	WithIPLock(func() {
		state := LoadNetworkState()

		if blockIdx, ok := state.Blocks[wsKey]; ok {
			base = PortPoolStart + blockIdx*BlockSize
			return
		}

		// Find next free block
		usedBlocks := make(map[int]bool)
		for _, idx := range state.Blocks {
			usedBlocks[idx] = true
		}
		for i := 0; i < MaxBlocks(); i++ {
			if !usedBlocks[i] {
				state.Blocks[wsKey] = i
				saveNetworkState(&state)
				base = PortPoolStart + i*BlockSize
				return
			}
		}

		fmt.Fprintf(os.Stderr, "warning: port pool exhausted (%d blocks) for %s\n", MaxBlocks(), wsKey)
	})
	return base
}

// WorkspacePort returns the port for a service in a workspace.
// svcIndex = position of service in config (0-based). originalPort = configured port.
// Main workspace → originalPort. Branch workspace → blockBase + svcIndex.
func WorkspacePort(wsKey string, svcIndex int, originalPort int, defaultBranch string) int {
	if isMainWs(wsKey, defaultBranch) {
		return originalPort
	}
	state := LoadNetworkState()
	blockIdx, ok := state.Blocks[wsKey]
	if !ok {
		return originalPort // not allocated yet
	}
	return PortPoolStart + blockIdx*BlockSize + svcIndex
}

// ReleaseBlock releases a port block for a workspace.
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

// ── Legacy compat (callers still use these names) ──

// MainIP returns 127.0.0.1 (all services bind to localhost now).
func MainIP(session, defaultBranch string) string {
	return "127.0.0.1"
}

// AllocateIP is legacy compat — returns 127.0.0.1.
// Port allocation is now separate via AllocateBlock/WorkspacePort.
func AllocateIP(session, worktreeKey string) string {
	return "127.0.0.1"
}

// ReleaseIP releases the port block for a workspace.
func ReleaseIP(worktreeKey string) {
	ReleaseBlock(worktreeKey)
}

// MigrateLegacyIPs migrates v2 (loopback IPs) to v3 (port allocation).
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
		// Clear legacy data
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
