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
	Version  int                        `json:"version"`
	Sessions map[string]*SessionState   `json:"sessions"` // session name → state

	// Legacy v2
	Subnets     map[string]int    `json:"subnets,omitempty"`
	Allocations map[string]string `json:"allocations,omitempty"`
}

type SessionState struct {
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
	if state.Sessions == nil {
		state.Sessions = make(map[string]*SessionState)
	}
	return state
}

func newState() NetworkState {
	return NetworkState{
		Version:  networkVersion,
		Sessions: make(map[string]*SessionState),
	}
}

func (s *NetworkState) session(name string) *SessionState {
	if ss, ok := s.Sessions[name]; ok {
		return ss
	}
	ss := &SessionState{
		Blocks:      make(map[string]int),
		SharedPorts: make(map[string]int),
		ServiceMap:  make(map[string]int),
	}
	s.Sessions[name] = ss
	return ss
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

// EnsureSharedPort assigns a fixed port for a shared service within a session.
func EnsureSharedPort(session, svcName string) int {
	var port int
	WithIPLock(func() {
		state := LoadNetworkState()
		ss := state.session(session)
		if p, ok := ss.SharedPorts[svcName]; ok {
			port = p
			return
		}
		used := make(map[int]bool)
		for _, p := range ss.SharedPorts {
			used[p] = true
		}
		for p := SharedPortStart; p <= SharedPortEnd; p++ {
			if !used[p] {
				ss.SharedPorts[svcName] = p
				saveNetworkState(&state)
				port = p
				return
			}
		}
		fmt.Fprintf(os.Stderr, "warning: shared port pool exhausted for session '%s'\n", session)
	})
	return port
}

// GetSharedPort returns existing shared port (0 if not assigned).
func GetSharedPort(session, svcName string) int {
	state := LoadNetworkState()
	if ss, ok := state.Sessions[session]; ok {
		return ss.SharedPorts[svcName]
	}
	return 0
}

// ── Service Map ──

// EnsureServiceIndex assigns a stable index to a service key within a session.
// svcKey format: "alias~svcname" (e.g., "api~api", "client~portal").
func EnsureServiceIndex(session, svcKey string) int {
	var idx int
	WithIPLock(func() {
		state := LoadNetworkState()
		ss := state.session(session)
		if i, ok := ss.ServiceMap[svcKey]; ok {
			idx = i
			return
		}
		maxIdx := -1
		for _, i := range ss.ServiceMap {
			if i > maxIdx {
				maxIdx = i
			}
		}
		idx = maxIdx + 1
		if idx >= BlockSize {
			fmt.Fprintf(os.Stderr, "warning: service map full (%d max) for session '%s'\n", BlockSize, session)
			return
		}
		ss.ServiceMap[svcKey] = idx
		saveNetworkState(&state)
	})
	return idx
}

// ── Workspace Port Blocks ──

// AllocateBlock assigns a port block for a branch workspace.
func AllocateBlock(session, wsKey, defaultBranch string) int {
	if isMainWs(wsKey, defaultBranch) {
		return 0
	}
	var base int
	WithIPLock(func() {
		state := LoadNetworkState()
		ss := state.session(session)
		if blockIdx, ok := ss.Blocks[wsKey]; ok {
			base = WsPortStart + blockIdx*BlockSize
			return
		}
		used := make(map[int]bool)
		for _, idx := range ss.Blocks {
			used[idx] = true
		}
		for i := 0; i < MaxWorkspaceBlocks(); i++ {
			if !used[i] {
				ss.Blocks[wsKey] = i
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
func WorkspacePort(session, wsKey, svcKey string, originalPort int, defaultBranch string) int {
	if isMainWs(wsKey, defaultBranch) {
		return originalPort
	}
	state := LoadNetworkState()
	ss, ok := state.Sessions[session]
	if !ok {
		return originalPort
	}
	blockIdx, ok := ss.Blocks[wsKey]
	if !ok {
		return originalPort
	}
	svcIdx, ok := ss.ServiceMap[svcKey]
	if !ok {
		return originalPort
	}
	return WsPortStart + blockIdx*BlockSize + svcIdx
}

// ReleaseBlock releases a port block for a workspace.
func ReleaseBlock(session, wsKey string) {
	WithIPLock(func() {
		state := LoadNetworkState()
		if ss, ok := state.Sessions[session]; ok {
			delete(ss.Blocks, wsKey)
			saveNetworkState(&state)
		}
	})
}

// LoadIPAllocations returns workspace → display string for UI (all sessions).
func LoadIPAllocations() map[string]string {
	state := LoadNetworkState()
	result := make(map[string]string)
	for _, ss := range state.Sessions {
		for wsKey, blockIdx := range ss.Blocks {
			base := WsPortStart + blockIdx*BlockSize
			result[wsKey] = fmt.Sprintf(":%d+", base)
		}
	}
	return result
}

func isMainWs(wsKey, defaultBranch string) bool {
	return wsKey == "ws-"+defaultBranch || wsKey == "ws-main" || wsKey == "ws-master"
}

// ── Legacy Compat ──

func MainIP(session, defaultBranch string) string  { return "127.0.0.1" }
func AllocateIP(session, worktreeKey string) string { return "127.0.0.1" }
func ReleaseIP(session, worktreeKey string)         { ReleaseBlock(session, worktreeKey) }

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
