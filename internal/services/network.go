package services

import (
	"encoding/json"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
)

const (
	PoolStart   = 40000
	PoolEnd     = 49999
	SlotSize    = 5000 // ports per session slot
	BlockSize   = 100  // ports per workspace block
	MaxSlots    = 2    // max concurrent sessions
	MaxBlocks   = SlotSize / BlockSize // 50 concurrent workspaces per session
)

// ── Global Slot Leasing (max 2 concurrent sessions) ──

type SlotLease struct {
	Slots map[string]string `json:"slots"` // slot index ("0","1") → session name
}

func globalSlotsPath() string { return homePath(".tncli/slots.json") }

func loadSlotLease() SlotLease {
	data, err := os.ReadFile(globalSlotsPath())
	if err != nil {
		return SlotLease{Slots: make(map[string]string)}
	}
	var lease SlotLease
	if json.Unmarshal(data, &lease) != nil {
		return SlotLease{Slots: make(map[string]string)}
	}
	if lease.Slots == nil {
		lease.Slots = make(map[string]string)
	}
	return lease
}

func saveSlotLease(lease *SlotLease) {
	path := globalSlotsPath()
	_ = os.MkdirAll(filepath.Dir(path), 0o755)
	data, _ := json.MarshalIndent(lease, "", "  ")
	tmp := path + ".tmp"
	if os.WriteFile(tmp, data, 0o644) == nil {
		_ = os.Rename(tmp, path)
	}
}

// ClaimSessionSlot claims a runtime slot for this session. Returns slot index (0 or 1).
// Returns -1 if both slots taken.
func ClaimSessionSlot(session string) int {
	var slot int = -1
	WithProjectLock(homePath(".tncli"), func() {
		lease := loadSlotLease()
		// Already claimed?
		for k, v := range lease.Slots {
			if v == session {
				fmt.Sscanf(k, "%d", &slot)
				return
			}
		}
		// Find free slot
		for i := 0; i < MaxSlots; i++ {
			key := fmt.Sprintf("%d", i)
			if _, taken := lease.Slots[key]; !taken {
				lease.Slots[key] = session
				saveSlotLease(&lease)
				slot = i
				return
			}
		}
	})
	return slot
}

// ReleaseSessionSlot releases the slot held by this session.
func ReleaseSessionSlot(session string) {
	WithProjectLock(homePath(".tncli"), func() {
		lease := loadSlotLease()
		for k, v := range lease.Slots {
			if v == session {
				delete(lease.Slots, k)
				saveSlotLease(&lease)
				return
			}
		}
	})
}

// SessionSlot returns the current slot for a session (-1 if not claimed).
func SessionSlot(session string) int {
	lease := loadSlotLease()
	for k, v := range lease.Slots {
		if v == session {
			var slot int
			fmt.Sscanf(k, "%d", &slot)
			return slot
		}
	}
	return -1
}

// ── Per-Project State ──

type NetworkState struct {
	Slot       int            `json:"slot"`        // current runtime slot (may change between runs)
	Blocks     map[string]int `json:"blocks"`      // wsKey → block index (runtime lease)
	ServiceMap map[string]int `json:"service_map"` // svcKey → slot within block (stable)
	SharedMap  map[string]int `json:"shared_map"`  // shared svc → offset from top (stable)
}

func networkPath(projectDir string) string {
	return filepath.Join(projectDir, ".tncli", "network.json")
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
		Slot:       -1,
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

// ── Init (called on every config load) ──

func InitNetwork(projectDir, session string, cfg *config.Config) {
	RegisterProject(session, projectDir)

	// Claim session slot
	slot := ClaimSessionSlot(session)
	if slot < 0 {
		fmt.Fprintf(os.Stderr, "warning: max %d concurrent sessions — cannot claim slot\n", MaxSlots)
		return
	}

	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		state.Slot = slot

		// Build service map from config (stable — doesn't change with slot)
		for _, dirName := range cfg.RepoOrder {
			dir := cfg.Repos[dirName]
			alias := dir.Alias
			if alias == "" {
				alias = dirName
			}
			for _, svcName := range dir.ServiceOrder {
				key := alias + "~" + svcName
				if _, ok := state.ServiceMap[key]; !ok {
					state.ServiceMap[key] = nextServiceIdx(state.ServiceMap)
				}
			}
		}

		// Build shared map from config (stable)
		for name := range cfg.SharedServices {
			if _, ok := state.SharedMap[name]; !ok {
				state.SharedMap[name] = len(state.SharedMap)
			}
		}

		saveNetworkState(projectDir, &state)
	})
}

func nextServiceIdx(m map[string]int) int {
	max := -1
	for _, v := range m {
		if v > max {
			max = v
		}
	}
	return max + 1
}

// ── Workspace Block Leasing ──

// ClaimBlock leases a block for a workspace. Returns block index.
func ClaimBlock(projectDir, wsKey string) int {
	var blockIdx int = -1
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		// Already claimed?
		if bi, ok := state.Blocks[wsKey]; ok {
			blockIdx = bi
			return
		}
		// Find free block, skip occupied ports
		used := make(map[int]bool)
		for _, bi := range state.Blocks {
			used[bi] = true
		}
		base := slotBase(state.Slot)
		for i := 0; i < MaxBlocks; i++ {
			if used[i] {
				continue
			}
			if IsPortFree(base + i*BlockSize) {
				state.Blocks[wsKey] = i
				saveNetworkState(projectDir, &state)
				blockIdx = i
				return
			}
		}
		fmt.Fprintf(os.Stderr, "warning: no free workspace blocks (max %d concurrent)\n", MaxBlocks)
	})
	return blockIdx
}

// ReleaseBlock frees a workspace block.
func ReleaseBlock(projectDir, wsKey string) {
	WithProjectLock(projectDir, func() {
		state := LoadNetworkState(projectDir)
		delete(state.Blocks, wsKey)
		saveNetworkState(projectDir, &state)
	})
}

// ── Port Resolution ──

func slotBase(slot int) int {
	return PoolStart + slot*SlotSize
}

func slotTop(slot int) int {
	return PoolStart + slot*SlotSize + SlotSize - 1
}

// Port returns the port for a workspace service.
func Port(projectDir, wsKey, svcKey string) int {
	state := LoadNetworkState(projectDir)
	if state.Slot < 0 {
		return 0
	}
	bi, ok := state.Blocks[wsKey]
	if !ok {
		return 0
	}
	si, ok := state.ServiceMap[svcKey]
	if !ok {
		return 0
	}
	return slotBase(state.Slot) + bi*BlockSize + si
}

// SharedPort returns the port for a shared service.
func SharedPort(projectDir, svcName string) int {
	state := LoadNetworkState(projectDir)
	if state.Slot < 0 {
		return 0
	}
	offset, ok := state.SharedMap[svcName]
	if !ok {
		return 0
	}
	return slotTop(state.Slot) - offset
}

// EnsurePortFree checks and auto-reallocates if port is occupied.
func EnsurePortFree(projectDir, wsKey, svcKey string, port int) int {
	if IsPortFree(port) {
		return port
	}
	state := LoadNetworkState(projectDir)
	bi, ok := state.Blocks[wsKey]
	if !ok {
		return port
	}
	base := slotBase(state.Slot) + bi*BlockSize
	for offset := 0; offset < BlockSize; offset++ {
		candidate := base + offset
		if candidate != port && IsPortFree(candidate) {
			WithProjectLock(projectDir, func() {
				s := LoadNetworkState(projectDir)
				s.ServiceMap[svcKey] = offset
				saveNetworkState(projectDir, &s)
			})
			fmt.Fprintf(os.Stderr, "port %d occupied, reallocated %s to :%d\n", port, svcKey, candidate)
			return candidate
		}
	}
	return port
}

func IsPortFree(port int) bool {
	ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", port))
	if err != nil {
		return false
	}
	ln.Close()
	return true
}

// ── Display ──

func LoadIPAllocations(projectDir string) map[string]string {
	state := LoadNetworkState(projectDir)
	result := make(map[string]string)
	if state.Slot < 0 {
		return result
	}
	base := slotBase(state.Slot)
	for wsKey, bi := range state.Blocks {
		result[wsKey] = fmt.Sprintf(":%d+", base+bi*BlockSize)
	}
	return result
}

// ── Legacy Compat ──

func MainIP(session, defaultBranch string) string  { return "127.0.0.1" }
func AllocateIP(session, worktreeKey string) string { return "127.0.0.1" }
func ReleaseIP(projectDir, worktreeKey string)      { ReleaseBlock(projectDir, worktreeKey) }

func MigrateLegacyIPs(projectDir string) {
	// No-op for v3+ — legacy migration handled by InitNetwork
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
