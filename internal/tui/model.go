package tui

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/paths"
	"github.com/toantran292/tncli/internal/pipeline"
	"github.com/toantran292/tncli/internal/services"
	"github.com/toantran292/tncli/internal/tmux"
)

// ComboItem types in the flattened tree view.
type ItemKind int

const (
	KindCombo ItemKind = iota
	KindInstance
	KindInstanceDir
	KindInstanceService
)

type ComboItem struct {
	Kind     ItemKind
	Name     string // combo name or service name
	Branch   string
	Dir      string
	WtKey    string
	Svc      string
	TmuxName string
	IsMain   bool
}

type PipelineDisplay struct {
	Operation    string
	Branch       string
	CurrentStage int
	TotalStages  int
	StageName    string
	Failed       *struct{ Stage int; Error string }
}

type Model struct {
	ConfigPath string
	Config     *config.Config
	Session    string
	DirNames   []string
	Worktrees  map[string]*services.WorktreeInfo

	Combos         []string
	ComboItems     []ComboItem
	ComboCollapsed map[string]bool
	WtCollapsed    map[string]bool
	ServiceModes   map[string]string // dir/svc → mode name

	Cursor         int
	ScrollOffset   int
	ComboLogIdx    int
	RunningWindows map[string]bool
	Stopping       map[string]bool
	Starting       map[string]bool

	Message     string
	MessageTime time.Time

	ActivePipelines  []PipelineDisplay
	CreatingWs       map[string]bool
	DeletingWs       map[string]bool

	// Popup state
	pendingPopup   *PendingPopup
	popupStack     []PendingPopup
	shortcutItems  []config.Shortcut
	wsCreating     bool
	wsName         string
	wsSourceBranch string

	// Process stats (RAM/CPU per service)
	ProcStats    map[string]ProcStat
	LastStatScan time.Time

	// tmux split state
	SplitPct     int
	TuiWindowID  string
	TuiSession   string
	TuiPaneID    string
	RightPaneID  string
	JoinedSvc    string
	SwapPending  bool

	Width, Height int
	LastScan      time.Time
}

func NewModel(configPath string) (*Model, error) {
	cfg, err := config.Load(configPath)
	if err != nil {
		return nil, err
	}

	session := cfg.Session
	dirNames := cfg.RepoOrder
	combos := make([]string, 0)
	for name := range cfg.AllWorkspaces() {
		combos = append(combos, name)
	}

	configDir := filepath.Dir(configPath)
	services.InitNetwork(configDir, cfg.Session, cfg)
	services.EnsureMainWorkspace(configDir, cfg)
	// Background: regenerate env + compose overrides, start shared services
	go func() {
		services.RegenerateWorkspaceEnv(configDir, cfg, cfg.GlobalDefaultBranch())
		if len(cfg.SharedServices) > 0 {
			services.GenerateSharedCompose(configDir, cfg.Session, cfg.SharedServices)
			var all []string
			for name := range cfg.SharedServices {
				all = append(all, name)
			}
			services.StartSharedServices(configDir, cfg.Session, all)
		}
	}()

	splitPct, wtCollapsed, comboCollapsed, serviceModes := loadCollapseState(session)

	// Apply saved modes to config
	for key, mode := range serviceModes {
		parts := strings.SplitN(key, "/", 2)
		if len(parts) == 2 {
			if dir, ok := cfg.Repos[parts[0]]; ok {
				if svc, ok := dir.Services[parts[1]]; ok {
					svc.Mode = mode
				}
			}
		}
	}

	m := &Model{
		ConfigPath:     configPath,
		Config:         cfg,
		Session:        session,
		SplitPct:       splitPct,
		DirNames:       dirNames,
		Worktrees:      make(map[string]*services.WorktreeInfo),
		Combos:         combos,
		ComboCollapsed: comboCollapsed,
		WtCollapsed:    wtCollapsed,
		ServiceModes:   serviceModes,
		RunningWindows: make(map[string]bool),
		Stopping:       make(map[string]bool),
		Starting:       make(map[string]bool),
		CreatingWs:     make(map[string]bool),
		DeletingWs:     make(map[string]bool),
		ProcStats:      make(map[string]ProcStat),
		LastScan:       time.Now(),
	}

	m.scanWorktrees()
	return m, nil
}

func (m *Model) SvcSession() string {
	return "tncli_" + m.Session
}

func (m *Model) IsRunning(svc string) bool {
	return m.RunningWindows[svc]
}

func (m *Model) CurrentItem() *ComboItem {
	if m.Cursor >= 0 && m.Cursor < len(m.ComboItems) {
		return &m.ComboItems[m.Cursor]
	}
	return nil
}

func (m *Model) SetMessage(msg string) {
	m.Message = msg
	m.MessageTime = time.Now()
}

func (m *Model) ClampCursor() {
	if len(m.ComboItems) > 0 && m.Cursor >= len(m.ComboItems) {
		m.Cursor = len(m.ComboItems) - 1
	}
}

func (m *Model) DirPath(dirName string) string {
	configDir := filepath.Dir(m.ConfigPath)
	if filepath.IsAbs(dirName) {
		return dirName
	}
	branch := m.Config.GlobalDefaultBranch()
	wsPath := filepath.Join(configDir, "workspace--"+branch, dirName)
	if info, err := os.Stat(wsPath); err == nil && info.IsDir() {
		return wsPath
	}
	return filepath.Join(configDir, dirName)
}

func (m *Model) WtTmuxName(dirName, svcName, branch string) string {
	alias := dirName
	if dir, ok := m.Config.Repos[dirName]; ok && dir.Alias != "" {
		alias = dir.Alias
	}
	branchSafe := strings.ReplaceAll(branch, "/", "-")
	return fmt.Sprintf("%s~%s~%s", alias, svcName, branchSafe)
}

// ── Refresh ──

func (m *Model) RefreshStatus() {
	svcSess := m.SvcSession()
	if tmux.SessionExists(svcSess) {
		m.RunningWindows = tmux.ListWindows(svcSess)
	} else {
		m.RunningWindows = make(map[string]bool)
	}
	if m.JoinedSvc != "" {
		m.RunningWindows[m.JoinedSvc] = true
	}
	// Filter internal windows
	for w := range m.RunningWindows {
		if strings.HasPrefix(w, "cmd~") || w == "_tncli_init" || w == "_blank" {
			delete(m.RunningWindows, w)
		}
	}
	// Clean up stopping/starting + force stats refresh on state change
	stateChanged := false
	for svc := range m.Stopping {
		if !m.RunningWindows[svc] {
			delete(m.Stopping, svc)
			delete(m.ProcStats, svc)
			stateChanged = true
		}
	}
	for svc := range m.Starting {
		if m.RunningWindows[svc] {
			delete(m.Starting, svc)
			stateChanged = true
		}
	}
	if stateChanged {
		m.LastStatScan = time.Time{} // force immediate stats refresh
	}

	// Detect active pipelines from markers
	markers := pipeline.ListActivePipelines()
	changed := false
	for _, ap := range markers {
		branch := strings.ReplaceAll(ap.BranchSafe, "_", "-")
		// Check if pipeline tmux window exists
		hasPipelineWindow := false
		for w := range m.RunningWindows {
			if (strings.HasPrefix(w, "pipeline~") || strings.HasPrefix(w, "setup~")) &&
				strings.HasSuffix(w, "~"+ap.BranchSafe) {
				hasPipelineWindow = true
				break
			}
		}
		if hasPipelineWindow {
			if !m.CreatingWs[branch] && !m.DeletingWs[branch] {
				m.CreatingWs[branch] = true
				changed = true
			}
		} else {
			pipeline.MarkPipelineDone(ap.BranchSafe)
			if m.CreatingWs[branch] {
				delete(m.CreatingWs, branch)
				changed = true
			}
		}
	}
	if changed {
		m.RebuildComboTree()
	}

	// Refresh process stats every second
	if time.Since(m.LastStatScan) > time.Second {
		m.refreshProcStats()
		m.LastStatScan = time.Now()
	}
}

// ── Worktree Scanning ──

func (m *Model) scanWorktrees() {
	m.Worktrees = make(map[string]*services.WorktreeInfo)
	for _, dirName := range m.DirNames {
		dirPath := m.DirPath(dirName)
		wts := services.ListWorktrees(dirPath)
		if len(wts) <= 1 {
			continue
		}
		for _, wt := range wts[1:] { // skip main worktree (index 0)
			wtPath, branch := wt.Path, wt.Branch
			if _, err := os.Stat(wtPath); os.IsNotExist(err) {
				continue
			}
			wtKey := fmt.Sprintf("%s--%s", dirName, strings.ReplaceAll(branch, "/", "-"))
			m.Worktrees[wtKey] = &services.WorktreeInfo{
				Branch:    branch,
				ParentDir: dirName,
				Path:      wtPath,
			}
		}
	}
	m.RebuildComboTree()
}

// ── Collapse State ──

func loadCollapseState(session string) (int, map[string]bool, map[string]bool, map[string]string) {
	path := paths.StatePath(fmt.Sprintf("collapse-%s.json", session))
	data, err := os.ReadFile(path)
	if err != nil {
		return 75, make(map[string]bool), make(map[string]bool), make(map[string]string)
	}
	var raw map[string]interface{}
	if json.Unmarshal(data, &raw) != nil {
		return 75, make(map[string]bool), make(map[string]bool), make(map[string]string)
	}

	wtCollapsed := make(map[string]bool)
	if wt, ok := raw["wt"].(map[string]interface{}); ok {
		for k, v := range wt {
			if b, ok := v.(bool); ok {
				wtCollapsed[k] = b
			}
		}
	}
	comboCollapsed := make(map[string]bool)
	if cb, ok := raw["combo"].(map[string]interface{}); ok {
		for k, v := range cb {
			if b, ok := v.(bool); ok {
				comboCollapsed[k] = b
			}
		}
	}
	serviceModes := make(map[string]string)
	if sm, ok := raw["service_modes"].(map[string]interface{}); ok {
		for k, v := range sm {
			if s, ok := v.(string); ok {
				serviceModes[k] = s
			}
		}
	}
	splitPct := 75
	if sp, ok := raw["split_pct"].(float64); ok && sp >= 20 && sp <= 90 {
		splitPct = int(sp)
	}
	return splitPct, wtCollapsed, comboCollapsed, serviceModes
}

func (m *Model) saveCollapseState() {
	path := paths.StatePath(fmt.Sprintf("collapse-%s.json", m.Session))
	wt := make(map[string]bool)
	for k, v := range m.WtCollapsed {
		if v {
			wt[k] = true
		}
	}
	combo := make(map[string]bool)
	for k, v := range m.ComboCollapsed {
		if v {
			combo[k] = true
		}
	}
	data, _ := json.MarshalIndent(map[string]interface{}{"wt": wt, "combo": combo, "split_pct": m.SplitPct, "service_modes": m.ServiceModes}, "", "  ")
	_ = os.MkdirAll(filepath.Dir(path), 0o755)
	_ = os.WriteFile(path, data, 0o644)
}

// ── Monitor helpers ──

func (m *Model) comboRunningCount(name string) (int, int) {
	entries := m.Config.AllWorkspaces()[name]
	running, total := 0, len(entries)
	for _, entry := range entries {
		d, s, ok := m.Config.FindServiceEntryQuiet(entry)
		if !ok {
			continue
		}
		alias := d
		if dir, ok := m.Config.Repos[d]; ok && dir.Alias != "" {
			alias = dir.Alias
		}
		if m.IsRunning(fmt.Sprintf("%s~%s", alias, s)) {
			running++
		}
	}
	return running, total
}

func (m *Model) totalRunningCount() (int, int) {
	running, total := 0, 0
	for _, name := range m.Combos {
		r, t := m.comboRunningCount(name)
		running += r
		total += t
	}
	return running, total
}

// WorkspaceBranch extracts workspace branch from worktree path.
func WorkspaceBranch(wt *services.WorktreeInfo) string {
	parent := filepath.Base(filepath.Dir(wt.Path))
	if ws, ok := strings.CutPrefix(parent, "workspace--"); ok {
		return ws
	}
	return ""
}
