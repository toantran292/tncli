package services

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
)

// WorktreeInfo holds info about a single worktree instance.
type WorktreeInfo struct {
	Branch    string
	ParentDir string
	BindIP    string
	Path      string
}

// BranchSafe sanitizes branch name for safe use in DB names, env vars, etc.
func BranchSafe(branch string) string {
	s := strings.ReplaceAll(branch, "/", "_")
	return strings.ReplaceAll(s, "-", "_")
}

// ResolveSlotTemplates resolves {{slot:SERVICE_NAME}} templates.
func ResolveSlotTemplates(val, wsKey string) string {
	result := val
	allocs := LoadSlotAllocations()
	for {
		start := strings.Index(result, "{{slot:")
		if start < 0 {
			break
		}
		end := strings.Index(result[start:], "}}")
		if end < 0 {
			break
		}
		end += start + 2
		svcName := result[start+7 : end-2]
		slot := 0
		if svc, ok := allocs[svcName]; ok {
			if a, ok := svc.Slots[wsKey]; ok {
				slot = a.Slot
			}
		}
		result = result[:start] + fmt.Sprintf("%d", slot) + result[end:]
	}
	return result
}

// ResolveConfigTemplates resolves {{host:NAME}}, {{port:NAME}}, {{url:NAME}}, {{conn:NAME}}.
func ResolveConfigTemplates(val string, cfg *config.Config, branchSafe string) string {
	result := val

	// {{host:NAME}}
	for {
		start := strings.Index(result, "{{host:")
		if start < 0 {
			break
		}
		end := strings.Index(result[start:], "}}")
		if end < 0 {
			break
		}
		end += start + 2
		name := result[start+7 : end-2]
		host := "127.0.0.1"
		if svc, ok := cfg.SharedServices[name]; ok && svc.Host != "" {
			host = svc.Host
		}
		result = result[:start] + host + result[end:]
	}

	// {{port:NAME}}
	for {
		start := strings.Index(result, "{{port:")
		if start < 0 {
			break
		}
		end := strings.Index(result[start:], "}}")
		if end < 0 {
			break
		}
		end += start + 2
		name := result[start+7 : end-2]
		var port uint16
		if svc, ok := cfg.SharedServices[name]; ok {
			port = firstPort(svc.Ports)
		} else {
			port = findRepoPort(cfg, name)
		}
		result = result[:start] + fmt.Sprintf("%d", port) + result[end:]
	}

	// {{url:NAME}}
	for {
		start := strings.Index(result, "{{url:")
		if start < 0 {
			break
		}
		end := strings.Index(result[start:], "}}")
		if end < 0 {
			break
		}
		end += start + 2
		name := result[start+6 : end-2]
		host := "127.0.0.1"
		var port uint16
		if svc, ok := cfg.SharedServices[name]; ok {
			if svc.Host != "" {
				host = svc.Host
			}
			port = firstPort(svc.Ports)
		} else {
			port = findRepoPort(cfg, name)
		}
		result = result[:start] + fmt.Sprintf("http://%s:%d", host, port) + result[end:]
	}

	// {{conn:NAME}}
	for {
		start := strings.Index(result, "{{conn:")
		if start < 0 {
			break
		}
		end := strings.Index(result[start:], "}}")
		if end < 0 {
			break
		}
		end += start + 2
		name := result[start+7 : end-2]
		var conn string
		if svc, ok := cfg.SharedServices[name]; ok {
			user := svc.DBUser
			if user == "" {
				user = "postgres"
			}
			pw := svc.DBPassword
			if pw == "" {
				pw = "postgres"
			}
			host := cfg.SharedHost(name)
			port := firstPort(svc.Ports)
			if port == 0 {
				port = 5432
			}
			conn = fmt.Sprintf("%s:%s@%s:%d", user, pw, host, port)
		}
		result = result[:start] + conn + result[end:]
	}

	return result
}

// ResolveDBTemplates resolves {{db:INDEX}} templates.
func ResolveDBTemplates(val string, dbNames []string) string {
	result := val
	for {
		start := strings.Index(result, "{{db:")
		if start < 0 {
			break
		}
		end := strings.Index(result[start:], "}}")
		if end < 0 {
			break
		}
		end += start + 2
		idxStr := result[start+5 : end-2]
		resolved := ""
		var idx int
		if _, err := fmt.Sscanf(idxStr, "%d", &idx); err == nil && idx < len(dbNames) {
			resolved = dbNames[idx]
		}
		result = result[:start] + resolved + result[end:]
	}
	return result
}

// ResolveEnvTemplates resolves template variables in env values.
func ResolveEnvTemplates(env map[string]string, cfg *config.Config, bindIP, branchSafe, branch, wsKey string) []EnvVar {
	var result []EnvVar
	for k, v := range env {
		val := strings.ReplaceAll(v, "{{bind_ip}}", bindIP)
		val = strings.ReplaceAll(val, "{{branch_safe}}", branchSafe)
		val = strings.ReplaceAll(val, "{{branch}}", branch)
		val = ResolveSlotTemplates(val, wsKey)
		val = ResolveConfigTemplates(val, cfg, branchSafe)
		result = append(result, EnvVar{Key: k, Value: val})
	}
	return result
}

// ── Helpers ──

func findRepoPort(cfg *config.Config, name string) uint16 {
	if dir, ok := cfg.Repos[name]; ok && dir.ProxyPort != nil {
		return *dir.ProxyPort
	}
	for _, d := range cfg.Repos {
		if d.Alias == name && d.ProxyPort != nil {
			return *d.ProxyPort
		}
	}
	return 0
}

// FirstPortFromList extracts host port from first port mapping.
func FirstPortFromList(ports []string) uint16 {
	return firstPort(ports)
}

func firstPort(ports []string) uint16 {
	if len(ports) == 0 {
		return 0
	}
	parts := strings.SplitN(ports[0], ":", 2)
	var p uint16
	fmt.Sscanf(parts[0], "%d", &p)
	return p
}

// ExtractPortFromCmd extracts port from command string.
func ExtractPortFromCmd(cmd string) uint16 {
	parts := strings.Fields(cmd)
	for i := 0; i+1 < len(parts); i++ {
		if parts[i] == "--port" || parts[i] == "-p" {
			var p uint16
			if _, err := fmt.Sscanf(parts[i+1], "%d", &p); err == nil {
				return p
			}
		}
	}
	for _, part := range parts {
		if val, ok := strings.CutPrefix(part, "--port="); ok {
			var p uint16
			if _, err := fmt.Sscanf(val, "%d", &p); err == nil {
				return p
			}
		}
	}
	return 0
}

// WorkspaceFolderPath returns the workspace folder path.
func WorkspaceFolderPath(configDir, name string) string {
	return filepath.Join(configDir, "workspace--"+name)
}
