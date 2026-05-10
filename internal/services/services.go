package services

import (
	"fmt"
	"net/url"
	"path/filepath"
	"sort"
	"strings"

	"github.com/toantran292/tncli/internal/config"
)

// WorktreeInfo holds info about a single worktree instance.
type WorktreeInfo struct {
	Branch    string
	ParentDir string
	Path      string
}

// BranchSafe sanitizes branch name for safe use in DB names, env vars, etc.
func BranchSafe(branch string) string {
	return strings.ReplaceAll(branch, "/", "_")
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

// ResolveConfigTemplates resolves {{host:NAME}}, {{port:NAME}}, {{url:NAME}}, {{ws:NAME}}, {{conn:NAME}}.
func ResolveConfigTemplates(val string, cfg *config.Config, branchSafe, wsKey, envName string) string {
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
		host := "localhost"
		if remote, ok := cfg.RemoteURL(envName, name); ok {
			host = extractHost(remote)
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
		var port int
		if remote, ok := cfg.RemoteURL(envName, name); ok {
			port = extractPort(remote)
		} else if _, ok := cfg.SharedServices[name]; ok {
			port = SharedPort(name)
		} else {
			port = findRepoServicePort(cfg, name, wsKey)
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
		var resolved string
		if remote, ok := cfg.RemoteURL(envName, name); ok {
			resolved = remote
		} else {
			var port int
			if _, ok := cfg.SharedServices[name]; ok {
				port = SharedPort(name)
			} else {
				port = findRepoServicePort(cfg, name, wsKey)
			}
			resolved = fmt.Sprintf("http://localhost:%d", port)
		}
		result = result[:start] + resolved + result[end:]
	}

	// {{ws:NAME}}
	for {
		start := strings.Index(result, "{{ws:")
		if start < 0 {
			break
		}
		end := strings.Index(result[start:], "}}")
		if end < 0 {
			break
		}
		end += start + 2
		name := result[start+5 : end-2]
		var resolved string
		if remote, ok := cfg.RemoteURL(envName, name); ok {
			resolved = httpToWs(remote)
		} else {
			var port int
			if _, ok := cfg.SharedServices[name]; ok {
				port = SharedPort(name)
			} else {
				port = findRepoServicePort(cfg, name, wsKey)
			}
			resolved = fmt.Sprintf("ws://localhost:%d", port)
		}
		result = result[:start] + resolved + result[end:]
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
			port := SharedPort(name)
			if port == 0 {
				port = 5432
			}
			conn = fmt.Sprintf("%s:%s@localhost:%d", user, pw, port)
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
func ResolveEnvTemplates(env map[string]string, cfg *config.Config, branchSafe, branch, wsKey, envName string) []EnvVar {
	keys := make([]string, 0, len(env))
	for k := range env {
		keys = append(keys, k)
	}
	sort.Strings(keys)

	result := make([]EnvVar, 0, len(env))
	for _, k := range keys {
		val := strings.ReplaceAll(env[k], "{{bind_ip}}", "localhost")
		val = strings.ReplaceAll(val, "{{branch_safe}}", branchSafe)
		val = strings.ReplaceAll(val, "{{branch}}", branch)
		val = ResolveSlotTemplates(val, wsKey)
		val = ResolveConfigTemplates(val, cfg, branchSafe, wsKey, envName)
		result = append(result, EnvVar{Key: k, Value: val})
	}
	return result
}

// ── Helpers ──

// findRepoServicePort returns the dynamic port for a repo's service.
// name can be "repo" (first service) or "repo/service" (specific service).
func findRepoServicePort(cfg *config.Config, name, wsKey string) int {
	repoName, svcName, _ := strings.Cut(name, "/")

	for dirName, dir := range cfg.Repos {
		if dirName != repoName && dir.Alias != repoName {
			continue
		}
		alias := dir.Alias
		if alias == "" {
			alias = dirName
		}

		if svcName != "" {
			svcKey := alias + "~" + svcName
			if p := Port(currentProjectDir, wsKey, svcKey); p > 0 {
				return p
			}
			return 0
		}

		if len(dir.ServiceOrder) > 0 {
			svcKey := alias + "~" + dir.ServiceOrder[0]
			if p := Port(currentProjectDir, wsKey, svcKey); p > 0 {
				return p
			}
		}
		if dir.ProxyPort != nil {
			return int(*dir.ProxyPort)
		}
		return 0
	}
	return 0
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

func extractHost(rawURL string) string {
	u, err := url.Parse(rawURL)
	if err != nil {
		return rawURL
	}
	return u.Hostname()
}

func extractPort(rawURL string) int {
	u, err := url.Parse(rawURL)
	if err != nil {
		return 0
	}
	if u.Port() != "" {
		var p int
		fmt.Sscanf(u.Port(), "%d", &p)
		return p
	}
	switch u.Scheme {
	case "https", "wss":
		return 443
	default:
		return 80
	}
}

func httpToWs(rawURL string) string {
	u, err := url.Parse(rawURL)
	if err != nil {
		return rawURL
	}
	switch u.Scheme {
	case "http":
		u.Scheme = "ws"
	case "https":
		u.Scheme = "wss"
	}
	return u.String()
}
