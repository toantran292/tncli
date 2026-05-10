package tui

import (
	"fmt"
	"os/exec"
	"strconv"
	"strings"
	"sync"
)

// ProcStat holds aggregated stats for a service's process tree.
type ProcStat struct {
	RSS float64 // MB
	CPU float64 // %
}

type procInfo struct {
	ppid int
	rss  int // KB
}

var statsMu sync.Mutex
var statsCollecting bool

// refreshProcStats kicks off a background goroutine to collect stats.
func (m *Model) refreshProcStats() {
	statsMu.Lock()
	if statsCollecting {
		statsMu.Unlock()
		return
	}
	statsCollecting = true
	statsMu.Unlock()

	svcSession := m.SvcSession()
	prevStats := m.ProcStats

	go func() {
		stats := collectAllStats(svcSession, prevStats)
		statsMu.Lock()
		m.ProcStats = stats
		statsCollecting = false
		statsMu.Unlock()
	}()
}

func collectAllStats(svcSession string, prevStats map[string]ProcStat) map[string]ProcStat {
	stats := make(map[string]ProcStat)

	// Get pane PIDs from tmux
	out, err := exec.Command("tmux", "list-windows", "-t", "="+svcSession,
		"-F", "#{window_name} #{pane_pid}").Output()
	if err != nil {
		return stats
	}

	pidToWin := make(map[int]string)
	for _, line := range strings.Split(strings.TrimSpace(string(out)), "\n") {
		parts := strings.SplitN(line, " ", 2)
		if len(parts) != 2 || parts[0] == "" || parts[1] == "" {
			continue
		}
		pid, err := strconv.Atoi(parts[1])
		if err != nil {
			continue
		}
		pidToWin[pid] = parts[0]
	}

	if len(pidToWin) == 0 {
		return stats
	}

	// Single ps command: get PID, PPID, RSS for all processes
	psOut, err := exec.Command("ps", "-axo", "pid,ppid,rss").Output()
	if err != nil {
		return stats
	}

	// Build parent→children map and RSS map
	procs := make(map[int]procInfo)
	children := make(map[int][]int)

	for _, line := range strings.Split(string(psOut), "\n") {
		fields := strings.Fields(line)
		if len(fields) < 3 {
			continue
		}
		pid, err1 := strconv.Atoi(fields[0])
		ppid, err2 := strconv.Atoi(fields[1])
		rss, err3 := strconv.Atoi(fields[2])
		if err1 != nil || err2 != nil || err3 != nil {
			continue
		}
		procs[pid] = procInfo{ppid: ppid, rss: rss}
		children[ppid] = append(children[ppid], pid)
	}

	// Sum RSS for each pane's process tree
	for panePid, winName := range pidToWin {
		totalKB := sumTree(panePid, procs, children)
		rssMB := float64(totalKB) / 1024
		if rssMB >= 5 {
			stats[winName] = ProcStat{RSS: rssMB}
		} else if prev, ok := prevStats[winName]; ok {
			stats[winName] = prev
		}
	}

	return stats
}

func sumTree(pid int, procs map[int]procInfo, children map[int][]int) int {
	info, ok := procs[pid]
	if !ok {
		return 0
	}
	total := info.rss
	for _, child := range children[pid] {
		total += sumTree(child, procs, children)
	}
	return total
}

// FormatStat returns a compact string like "512M"
func FormatStat(s ProcStat) string {
	if s.RSS < 5 {
		return ""
	}
	if s.RSS >= 1024 {
		return fmt.Sprintf("%.1fG", s.RSS/1024)
	}
	return fmt.Sprintf("%.0fM", s.RSS)
}
