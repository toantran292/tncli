package commands

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
)

func Disk() {
	home, _ := os.UserHomeDir()

	fmt.Printf("%sDisk (actual via df)%s\n", Bold, NC)
	if out, err := exec.Command("df", "-h", home).Output(); err == nil {
		lines := strings.Split(strings.TrimSpace(string(out)), "\n")
		if len(lines) >= 2 {
			fields := strings.Fields(lines[1])
			if len(fields) >= 4 {
				fmt.Printf("  Used: %s  Avail: %s\n", fields[2], fields[3])
			}
		}
	}

	if out, err := exec.Command("pnpm", "store", "path").Output(); err == nil {
		storePath := strings.TrimSpace(string(out))
		storeSize := dirSizeMB(storePath)
		fmt.Printf("  pnpm store: %s (%s)\n", formatMB(storeSize), storePath)
	}

	projects := services.ListProjects()
	if len(projects) == 0 {
		fmt.Printf("\n%sNo registered projects%s\n", Dim, NC)
		return
	}

	for session, projectDir := range projects {
		cfgPath := filepath.Join(projectDir, "tncli.yml")
		cfg, err := config.Load(cfgPath)
		if err != nil {
			continue
		}
		diskForProject(session, projectDir, cfg)
	}
}

type repoStats struct {
	name      string
	instances int
	totalMB   int
	avgMB     int
}

func diskForProject(session, projectDir string, cfg *config.Config) {
	entries, err := os.ReadDir(projectDir)
	if err != nil {
		return
	}

	// Count workspaces
	wsCount := 0
	for _, e := range entries {
		if e.IsDir() && strings.HasPrefix(e.Name(), "workspace--") {
			wsCount++
		}
	}

	fmt.Printf("\n%s━━ %s%s (%d workspaces)\n", Bold, session, NC, wsCount)

	// Aggregate by repo
	stats := make(map[string]*repoStats)

	for _, e := range entries {
		if !e.IsDir() || !strings.HasPrefix(e.Name(), "workspace--") {
			continue
		}
		wsPath := filepath.Join(projectDir, e.Name())

		repoEntries, _ := os.ReadDir(wsPath)
		for _, re := range repoEntries {
			if !re.IsDir() {
				continue
			}
			nmPath := filepath.Join(wsPath, re.Name(), "node_modules")
			if !dirExists(nmPath) {
				continue
			}
			mb := dirSizeMB(nmPath)
			alias := re.Name()
			if dir, ok := cfg.Repos[re.Name()]; ok && dir.Alias != "" {
				alias = dir.Alias
			}
			s, ok := stats[alias]
			if !ok {
				s = &repoStats{name: alias}
				stats[alias] = s
			}
			s.instances++
			s.totalMB += mb
		}
	}

	if len(stats) == 0 {
		fmt.Printf("  %sno node_modules found%s\n", Dim, NC)
		return
	}

	// Sort by total desc
	var sorted []repoStats
	for _, s := range stats {
		s.avgMB = s.totalMB / s.instances
		sorted = append(sorted, *s)
	}
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].totalMB > sorted[j].totalMB })

	// Print table
	fmt.Printf("  %-15s %8s %6s %10s\n", "Repo", "Per-ws", "Count", "Apparent")
	fmt.Printf("  %s\n", strings.Repeat("─", 43))

	grandTotal := 0
	for _, s := range sorted {
		fmt.Printf("  %-15s %8s %5d  %9s\n", s.name, formatMB(s.avgMB), s.instances, formatMB(s.totalMB))
		grandTotal += s.totalMB
	}
	fmt.Printf("  %s\n", strings.Repeat("─", 43))
	fmt.Printf("  %-15s %8s %5d  %9s\n", "", "", len(sorted), formatMB(grandTotal))
	if cfg.LocalPM == "pnpm" && grandTotal > 0 {
		fmt.Printf("\n  %spnpm + APFS CoW: actual disk ≈ 1 copy + store overhead%s\n", Dim, NC)
	}
}

func dirSizeMB(path string) int {
	out, err := exec.Command("du", "-sm", path).Output()
	if err != nil {
		return 0
	}
	fields := strings.Fields(string(out))
	if len(fields) == 0 {
		return 0
	}
	mb, _ := strconv.Atoi(fields[0])
	return mb
}

func formatMB(mb int) string {
	if mb >= 1024 {
		return fmt.Sprintf("%.1fG", float64(mb)/1024)
	}
	return fmt.Sprintf("%dM", mb)
}

func dirExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.IsDir()
}
