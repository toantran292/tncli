package services

import (
	"encoding/json"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

const (
	networkVersion = 3

	PoolStart    = 40000
	PoolEnd      = 49999
	SessionSize  = 1000
	BlockSize    = 100
	MaxSessions  = (PoolEnd - PoolStart + 1) / SessionSize // 10
	MaxBlocks    = SessionSize / BlockSize                   // 10 (but shared eats from top)
)

// NetworkState stored at <project>/.tncli/network.json.
type NetworkState struct {
	Version    int            `json:"version"`
	SessionIdx int            `json:"session_idx"`
	Blocks     map[string]int `json:"blocks"`      // wsKey → block index (0 = first block from bottom)
	ServiceMap map[string]int `json:"service_map"` // svcKey → slot within block
	SharedMap  map[string]int `json:"shared_map"`  // shared svc → offset from top (0 = last port, 1 = second last, ...)
}

// ── I/O ──

func networkPath(projectDir string) string {
	return filepath.Join(projectDir, ".tncli", "network.json")
}
func netLockPath(projectDir string) string {
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
	if state.ServiceMap == nil {
		state.ServiceMap = make(map[string]int)
	}
	if state.SharedMap == nil {
		state.SharedMap = make(map[string]int)
	}
	return state
}

func newState() NetworkState {
	return NetworkState{
		Version:    networkVersion,
		SessionIdx: -1,
		Blocks:     make(map[string]int),
		ServiceMap: make(map[string]int),
		SharedMap:  make(map[string]int),
	}
}

func saveNetworkState(projectDir string, state *NetworkState) {
	path := networkPath(projectDir)
	_ = os.MkdirAll(filepath.Dir(path), 0o755)
	data, _ := json.MarshalIndent(state, "", "  ")
	tmp := path + ".tmp"
	if os.WriteFile(tmp, data, 0o644) == nil {
		_ = os.Rename(tmp, path)
	}
}

// ── File Lock ──

func WithProjectLock(projectDir string, fn func()) {
	lp := netLockPath(projectDir)
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

// ── Session Index ──

func EnsureSessionIdx(projectDir string) int {
	state := LoadNetworkState(projectDir)
	if state.SessionIdx >= 0 {
		return state.SessionIdx
	}
	used := make(map[int]bool)
	for _, dir := range ListProjects() {
		if dir == projectDir {
			continue
		}
		other := LoadNetworkState(dir)
		if other.SessionIdx >= 0 {
			used[other.SessionIdx] = true
		}
	}
	var idx int
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		if state.SessionIdx >= 0 {
			idx = state.SessionIdx
			return
		}
		for i := 0; i < MaxSessions; i++ {
			if !used[i] {
				idx = i
				state.SessionIdx = i
				saveNetworkState(projectDir, &state)
				return
			}
		}
		fmt.Fprintf(os.Stderr, "warning: all %d session slots taken\n", MaxSessions)
	})
	return idx
}

func sessionTop(sessionIdx int) int {
	return PoolStart + sessionIdx*SessionSize + SessionSize - 1
}

func sessionBase(sessionIdx int) int {
	return PoolStart + sessionIdx*SessionSize
}

// ── Shared Services (allocated from TOP of session range, growing DOWN) ──

// EnsureSharedPort assigns a port for a shared service.
// Shared ports count down from the top of the session range.
// Session 0: postgres=40999, redis=40998, minio=40997, ...
func EnsureSharedPort(projectDir, svcName string) int {
	var port int
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		if state.SessionIdx < 0 {
			state.SessionIdx = EnsureSessionIdx(projectDir)
		}
		if offset, ok := state.SharedMap[svcName]; ok {
			port = sessionTop(state.SessionIdx) - offset
			return
		}
		// Find next free offset, skipping ports occupied by other apps
		top := sessionTop(state.SessionIdx)
		usedOffsets := make(map[int]bool)
		for _, o := range state.SharedMap {
			usedOffsets[o] = true
		}
		for offset := 0; offset < SessionSize; offset++ {
			if usedOffsets[offset] {
				continue
			}
			candidate := top - offset
			if IsPortFree(candidate) {
				state.SharedMap[svcName] = offset
				saveNetworkState(projectDir, &state)
				port = candidate
				return
			}
		}
		fmt.Fprintf(os.Stderr, "warning: no free shared port for %s\n", svcName)
	})
	return port
}

func GetSharedPort(projectDir, svcName string) int {
	state := LoadNetworkState(projectDir)
	if state.SessionIdx < 0 {
		return 0
	}
	offset, ok := state.SharedMap[svcName]
	if !ok {
		return 0
	}
	return sessionTop(state.SessionIdx) - offset
}

// ── Service Map (slot index within workspace block) ──

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

// ── Workspace Blocks (allocated from BOTTOM of session range, growing UP) ──

// AllocateBlock assigns a block for a workspace.
// Block 0 starts at session base, block 1 at base+100, etc.
func AllocateBlock(projectDir, wsKey string) int {
	var blockIdx int
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		if state.SessionIdx < 0 {
			state.SessionIdx = EnsureSessionIdx(projectDir)
		}
		if bi, ok := state.Blocks[wsKey]; ok {
			blockIdx = bi
			return
		}
		used := make(map[int]bool)
		for _, bi := range state.Blocks {
			used[bi] = true
		}
		// Check collision with shared ports growing from top
		sharedCount := len(state.SharedMap)
		maxBlock := (SessionSize - sharedCount) / BlockSize
		base := sessionBase(state.SessionIdx)
		for i := 0; i < maxBlock; i++ {
			if used[i] {
				continue
			}
			// Quick check: first port of block free?
			if isBlockFree(base + i*BlockSize) {
				blockIdx = i
				state.Blocks[wsKey] = i
				saveNetworkState(projectDir, &state)
				return
			}
		}
		fmt.Fprintf(os.Stderr, "warning: no free workspace blocks (all occupied)\n")
	})
	return blockIdx
}

// IsPortFree checks if a TCP port is available on 127.0.0.1.
func IsPortFree(port int) bool {
	ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", port))
	if err != nil {
		return false
	}
	ln.Close()
	return true
}

// isBlockFree checks if the first port in a block is available (fast check).
func isBlockFree(base int) bool {
	return IsPortFree(base)
}

func ReleaseBlock(projectDir, wsKey string) {
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		delete(state.Blocks, wsKey)
		saveNetworkState(projectDir, &state)
	})
}

// ── Port Resolution ──

// Port returns the actual port for a workspace service.
// port = sessionBase + blockIdx*100 + svcSlot
func Port(projectDir, wsKey, svcKey string) int {
	state := LoadNetworkState(projectDir)
	if state.SessionIdx < 0 {
		return 0
	}
	blockIdx, ok := state.Blocks[wsKey]
	if !ok {
		return 0
	}
	svcSlot, ok := state.ServiceMap[svcKey]
	if !ok {
		return 0
	}
	return sessionBase(state.SessionIdx) + blockIdx*BlockSize + svcSlot
}

// ── Display ──

func LoadIPAllocations(projectDir string) map[string]string {
	state := LoadNetworkState(projectDir)
	result := make(map[string]string)
	if state.SessionIdx < 0 {
		return result
	}
	base := sessionBase(state.SessionIdx)
	for wsKey, blockIdx := range state.Blocks {
		wsBase := base + blockIdx*BlockSize
		result[wsKey] = fmt.Sprintf(":%d+", wsBase)
	}
	return result
}

// ── Legacy Compat ──

func MainIP(session, defaultBranch string) string  { return "127.0.0.1" }
func AllocateIP(session, worktreeKey string) string { return "127.0.0.1" }
func ReleaseIP(projectDir, worktreeKey string)      { ReleaseBlock(projectDir, worktreeKey) }

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

func homePath(rel string) string {
	home, _ := os.UserHomeDir()
	if home == "" {
		home = "/tmp"
	}
	return filepath.Join(home, rel)
}
