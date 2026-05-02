package services

import (
	"encoding/json"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

const (
	networkStateFile = ".tncli/network.json"
	currentVersion   = 2
)

type NetworkState struct {
	Version     int               `json:"version"`
	Subnets     map[string]int    `json:"subnets"`
	Allocations map[string]string `json:"allocations"`
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
		return NetworkState{Version: currentVersion, Subnets: make(map[string]int), Allocations: make(map[string]string)}
	}
	var state NetworkState
	if err := json.Unmarshal(data, &state); err != nil {
		return NetworkState{Version: currentVersion, Subnets: make(map[string]int), Allocations: make(map[string]string)}
	}
	if state.Subnets == nil {
		state.Subnets = make(map[string]int)
	}
	if state.Allocations == nil {
		state.Allocations = make(map[string]string)
	}
	return state
}

// saveNetworkState writes state atomically (write tmp → rename).
// Prevents corruption if process crashes mid-write.
func saveNetworkState(state *NetworkState) {
	path := statePath()
	_ = os.MkdirAll(filepath.Dir(path), 0o755)
	data, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return
	}
	tmp := path + ".tmp"
	if os.WriteFile(tmp, data, 0o644) == nil {
		_ = os.Rename(tmp, path) // atomic on POSIX
	}
}

// LoadIPAllocations returns current allocations for display only (no lock).
// Do NOT use for allocate/release decisions — use AllocateIP/ReleaseIP instead.
func LoadIPAllocations() map[string]string {
	return LoadNetworkState().Allocations
}

// WithIPLock provides file-lock protected operation.
// Uses defer to guarantee lock cleanup even on panic.
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

	// defer ensures lock cleanup even if fn() panics
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
	// Signal 0 = check existence without killing
	return p.Signal(syscall.Signal(0)) == nil
}

// MigrateLegacyIPs migrates from v1 to v2 format.
func MigrateLegacyIPs() {
	state := LoadNetworkState()
	if state.Version >= currentVersion {
		return
	}

	WithIPLock(func() {
		state := LoadNetworkState()
		if state.Version >= currentVersion {
			return
		}

		// Import from old files
		oldLoopback := homePath(".tncli/loopback.json")
		if data, err := os.ReadFile(oldLoopback); err == nil {
			var allocs map[string]string
			if json.Unmarshal(data, &allocs) == nil {
				for k, v := range allocs {
					if !strings.HasPrefix(v, "127.0.0.") {
						state.Allocations[k] = v
					}
				}
			}
			_ = os.Remove(oldLoopback)
		}

		oldSubnets := homePath(".tncli/subnets.json")
		if data, err := os.ReadFile(oldSubnets); err == nil {
			var subs map[string]int
			if json.Unmarshal(data, &subs) == nil {
				state.Subnets = subs
			}
			_ = os.Remove(oldSubnets)
		}

		// Clear legacy 127.0.0.x
		for k, ip := range state.Allocations {
			if strings.HasPrefix(ip, "127.0.0.") {
				delete(state.Allocations, k)
			}
		}

		state.Version = currentVersion
		saveNetworkState(&state)
		_ = os.Remove(homePath(".tncli/.migrated-subnet"))
	})

	// Clean legacy proxy routes (outside IP lock — proxy has own save logic)
	migrateLegacyProxyRoutes()
}

func migrateLegacyProxyRoutes() {
	routes := LoadRoutes()
	changed := false
	for k, target := range routes.Routes {
		if strings.HasPrefix(target, "127.0.0.") {
			delete(routes.Routes, k)
			changed = true
		}
	}
	if changed {
		recalcListenPorts(&routes)
		SaveRoutes(&routes)
	}
}

// MainIP gets IP for main workspace.
func MainIP(session, defaultBranch string) string {
	key := "ws-" + strings.ReplaceAll(defaultBranch, "/", "-")
	return AllocateIP(session, key)
}

// AllocateIP allocates next available loopback IP within the session's subnet.
// Format: 127.0.{subnet}.{2..254} — one subnet per session, one IP per workspace.
// Thread-safe via file lock. Ensures loopback alias exists after allocation.
func AllocateIP(session, worktreeKey string) string {
	var result string
	WithIPLock(func() {
		state := LoadNetworkState()

		// Each session gets its own /24 subnet (127.0.{N}.0/24)
		subnet, ok := state.Subnets[session]
		if !ok {
			used := make(map[int]bool)
			for _, s := range state.Subnets {
				used[s] = true
			}
			for n := 1; n <= 254; n++ {
				if !used[n] {
					subnet = n
					break
				}
			}
			if subnet == 0 {
				subnet = 254
			}
			state.Subnets[session] = subnet
		}

		// Return existing allocation
		if ip, ok := state.Allocations[worktreeKey]; ok {
			result = ip
			return
		}

		// Pick next available IP in subnet
		prefix := fmt.Sprintf("127.0.%d.", subnet)
		used := make(map[string]bool)
		for _, ip := range state.Allocations {
			used[ip] = true
		}
		for n := 2; n < 255; n++ {
			ip := fmt.Sprintf("%s%d", prefix, n)
			if !used[ip] {
				state.Allocations[worktreeKey] = ip
				saveNetworkState(&state)
				result = ip
				return
			}
		}

		// Subnet full (253 workspaces) — should not happen in practice.
		// Log warning but don't silently reuse an IP (would cause port conflict).
		fmt.Fprintf(os.Stderr, "warning: subnet 127.0.%d.0/24 full — no free IPs for %s\n", subnet, worktreeKey)
	})

	// Ensure loopback alias exists (outside lock — calls daemon via socket)
	if result != "" {
		ensureLoopbackAlias(result)
	}
	return result
}

// ensureLoopbackAlias creates a loopback alias via the loopback daemon (no sudo needed).
// Falls back to sudo -n if daemon is not running.
func ensureLoopbackAlias(ip string) {
	// Fast check: try TCP connect to loopback (faster than ping, no ICMP needed)
	conn, err := net.DialTimeout("tcp", ip+":1", 50*time.Millisecond)
	if err == nil {
		conn.Close()
		return // IP is reachable — alias exists
	}
	// Connection refused = alias exists but no service on port 1 (expected)
	if strings.Contains(err.Error(), "connection refused") {
		return
	}
	// "no route" or timeout = alias doesn't exist → create it
	if RequestLoopbackAlias(ip) {
		return
	}
	_ = exec.Command("sudo", "-n", "ifconfig", "lo0", "alias", ip).Run()
}

// ReleaseIP releases an allocated IP.
func ReleaseIP(worktreeKey string) {
	WithIPLock(func() {
		state := LoadNetworkState()
		delete(state.Allocations, worktreeKey)
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
