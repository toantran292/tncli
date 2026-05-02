package services

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

const (
	proxyRoutesFile = ".tncli/proxy-routes.json"
	proxyPIDFile    = ".tncli/proxy.pid"
	caddyfilePath   = ".tncli/Caddyfile"
)

type ProxyRoutes struct {
	ListenPorts []uint16          `json:"listen_ports"`
	Routes      map[string]string `json:"routes"`
}

func routesPath() string { return homePath(proxyRoutesFile) }
func pidPath() string    { return homePath(proxyPIDFile) }

func LoadRoutes() ProxyRoutes {
	data, err := os.ReadFile(routesPath())
	if err != nil {
		return ProxyRoutes{Routes: make(map[string]string)}
	}
	var routes ProxyRoutes
	if json.Unmarshal(data, &routes) != nil {
		return ProxyRoutes{Routes: make(map[string]string)}
	}
	if routes.Routes == nil {
		routes.Routes = make(map[string]string)
	}
	return routes
}

func SaveRoutes(routes *ProxyRoutes) {
	path := routesPath()
	_ = os.MkdirAll(filepath.Dir(path), 0o755)
	data, _ := json.MarshalIndent(routes, "", "  ")
	_ = os.WriteFile(path, data, 0o644)
}

func recalcListenPorts(routes *ProxyRoutes) {
	portSet := make(map[uint16]bool)
	for k := range routes.Routes {
		parts := strings.Split(k, ":")
		if len(parts) > 0 {
			var p uint16
			fmt.Sscanf(parts[len(parts)-1], "%d", &p)
			if p > 0 {
				portSet[p] = true
			}
		}
	}
	routes.ListenPorts = nil
	for p := range portSet {
		routes.ListenPorts = append(routes.ListenPorts, p)
	}
	sort.Slice(routes.ListenPorts, func(i, j int) bool { return routes.ListenPorts[i] < routes.ListenPorts[j] })
}

func ProxyHostname(session, alias, branchSafe string) string {
	return fmt.Sprintf("%s.%s.ws-%s.tncli.test", session, alias, branchSafe)
}

// RegisterRoutesSimple registers proxy routes for a workspace.
func RegisterRoutesSimple(session, branchSafe string, entries []ProxyEntry) {
	WithProjectLock(homePath(".tncli"), func() {
		routes := LoadRoutes()
		for _, e := range entries {
			hostname := ProxyHostname(session, e.Alias, branchSafe)
			key := fmt.Sprintf("%s:%d", hostname, e.Port)
			target := fmt.Sprintf("%s:%d", e.BindIP, e.Port)
			routes.Routes[key] = target
			found := false
			for _, p := range routes.ListenPorts {
				if p == e.Port {
					found = true
					break
				}
			}
			if !found {
				routes.ListenPorts = append(routes.ListenPorts, e.Port)
			}
		}
		SaveRoutes(&routes)
	})
}

type ProxyEntry struct {
	Alias  string
	Port   uint16
	BindIP string
}

// UnregisterRoutes removes routes for a workspace.
func UnregisterRoutes(branchSafe string) {
	WithProjectLock(homePath(".tncli"), func() {
		routes := LoadRoutes()
		prefix := fmt.Sprintf(".ws-%s.tncli.test:", branchSafe)
		for k := range routes.Routes {
			if strings.Contains(k, prefix) {
				delete(routes.Routes, k)
			}
		}
		recalcListenPorts(&routes)
		SaveRoutes(&routes)
	})
}

func SavePID(pid int) {
	path := pidPath()
	_ = os.MkdirAll(filepath.Dir(path), 0o755)
	_ = os.WriteFile(path, []byte(fmt.Sprintf("%d", pid)), 0o644)
}

func ReadPID() (int, bool) {
	data, err := os.ReadFile(pidPath())
	if err != nil {
		return 0, false
	}
	var pid int
	if _, err := fmt.Sscanf(strings.TrimSpace(string(data)), "%d", &pid); err != nil {
		return 0, false
	}
	return pid, true
}

func RemovePID() {
	_ = os.Remove(pidPath())
}

func IsProxyRunning() bool {
	pid, ok := ReadPID()
	if !ok {
		return false
	}
	return exec.Command("kill", "-0", fmt.Sprintf("%d", pid)).Run() == nil
}

func caddyfileFull() string { return homePath(caddyfilePath) }

// GenerateCaddyfile generates Caddyfile from proxy routes.
func GenerateCaddyfile() {
	routes := LoadRoutes()
	home, _ := os.UserHomeDir()
	logPath := filepath.Join(home, ".tncli/proxy.log")

	var b strings.Builder
	fmt.Fprintf(&b, "{\n  auto_https off\n  log {\n    output file %s {\n      roll_size 1mb\n      roll_keep 1\n    }\n    level WARN\n  }\n}\n\n", logPath)

	// Group routes by port
	portRoutes := make(map[uint16][][2]string)
	for key, target := range routes.Routes {
		if hostname, portStr, ok := strings.Cut(key, ":"); ok {
			var port uint16
			fmt.Sscanf(portStr, "%d", &port)
			if port > 0 && target != "" && !strings.HasPrefix(target, ":") {
				portRoutes[port] = append(portRoutes[port], [2]string{hostname, target})
			}
		}
	}

	for port, routeList := range portRoutes {
		fmt.Fprintf(&b, "http://:%d {\n", port)
		b.WriteString("  bind 127.0.0.1\n")
		for i, r := range routeList {
			fmt.Fprintf(&b, "  @r%d host %s\n", i, r[0])
			fmt.Fprintf(&b, "  reverse_proxy @r%d %s\n", i, r[1])
		}
		b.WriteString("}\n\n")
	}

	path := caddyfileFull()
	_ = os.MkdirAll(filepath.Dir(path), 0o755)
	_ = os.WriteFile(path, []byte(b.String()), 0o644)
}

// RunProxyServer runs Caddy in foreground.
func RunProxyServer() error {
	GenerateCaddyfile()
	SavePID(os.Getpid())

	cmd := exec.Command("caddy", "run", "--config", caddyfileFull())
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()

	RemovePID()
	return err
}

// ReloadCaddy reloads Caddy config. Errors are non-fatal (caddy may not be running).
func ReloadCaddy() {
	GenerateCaddyfile()
	// Best-effort: caddy may not be running yet
	exec.Command("caddy", "reload", "--config", caddyfileFull()).Run() //nolint:errcheck
}
