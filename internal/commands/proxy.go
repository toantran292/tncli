package commands

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
)

func ProxyStart() error {
	if services.IsProxyRunning() {
		pid, _ := services.ReadPID()
		fmt.Printf("%sproxy already running%s (pid %d)\n", Green, NC, pid)
		return nil
	}

	cfgPath, err := config.FindConfig()
	if err != nil {
		return err
	}
	cfg, err := config.Load(cfgPath)
	if err != nil {
		return err
	}
	registerProxyRoutesFromConfig(cfg)

	exe, _ := os.Executable()
	home, _ := os.UserHomeDir()
	_ = os.MkdirAll(filepath.Join(home, ".tncli"), 0o755)

	proc, err := os.StartProcess(exe, []string{exe, "proxy", "serve"}, &os.ProcAttr{
		Files: []*os.File{nil, nil, nil},
	})
	if err != nil {
		return fmt.Errorf("failed to start proxy: %w", err)
	}
	fmt.Printf("%sproxy started%s (pid %d)\n", Green, NC, proc.Pid)
	_ = proc.Release()
	return nil
}

func ProxyStop() {
	pid, ok := services.ReadPID()
	if !ok {
		fmt.Println("proxy not running")
		return
	}
	p, err := os.FindProcess(pid)
	if err == nil {
		_ = p.Kill()
	}
	services.RemovePID()
	fmt.Printf("%sproxy stopped%s (was pid %d)\n", Green, NC, pid)
}

func ProxyRestart() error {
	ProxyStop()
	home, _ := os.UserHomeDir()
	_ = os.Remove(filepath.Join(home, ".tncli/proxy-routes.json"))
	return ProxyStart()
}

func ProxyStatus() {
	if services.IsProxyRunning() {
		pid, _ := services.ReadPID()
		fmt.Printf("%sproxy running%s (pid %d)\n", Green, NC, pid)
	} else {
		fmt.Printf("%sproxy not running%s\n", Yellow, NC)
	}

	routes := services.LoadRoutes()
	if len(routes.Routes) == 0 {
		fmt.Println("no routes configured")
	} else {
		fmt.Printf("\n%sListen ports:%s %v\n", Bold, NC, routes.ListenPorts)
		fmt.Printf("\n%sRoutes:%s\n", Bold, NC)
		for hostname, target := range routes.Routes {
			fmt.Printf("  %s%s%s → %s\n", Blue, hostname, NC, target)
		}
	}
}

func ProxyInstall() error {
	exe, err := os.Executable()
	if err != nil {
		return err
	}

	home, _ := os.UserHomeDir()
	plistDir := filepath.Join(home, "Library/LaunchAgents")
	plistPath := filepath.Join(plistDir, "com.tncli.proxy.plist")
	logPath := filepath.Join(home, ".tncli/proxy.log")

	plist := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tncli.proxy</string>
    <key>ProgramArguments</key>
    <array>
        <string>%s</string>
        <string>proxy</string>
        <string>serve</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>%s</string>
    <key>StandardErrorPath</key>
    <string>%s</string>
</dict>
</plist>`, exe, logPath, logPath)

	_ = os.MkdirAll(plistDir, 0o755)
	if err := os.WriteFile(plistPath, []byte(plist), 0o644); err != nil {
		return fmt.Errorf("failed to write plist: %w", err)
	}
	_ = exec.Command("launchctl", "unload", plistPath).Run()
	if err := exec.Command("launchctl", "load", plistPath).Run(); err != nil {
		return fmt.Errorf("failed to load launchd plist: %w", err)
	}
	fmt.Printf("%sproxy daemon installed and started%s\n", Green, NC)
	fmt.Printf("  plist: %s\n  log:   %s\n", plistPath, logPath)
	return nil
}

func ProxyUninstall() {
	home, _ := os.UserHomeDir()
	plistPath := filepath.Join(home, "Library/LaunchAgents/com.tncli.proxy.plist")
	if _, err := os.Stat(plistPath); err == nil {
		_ = exec.Command("launchctl", "unload", plistPath).Run()
		_ = os.Remove(plistPath)
		fmt.Printf("%sproxy daemon uninstalled%s\n", Green, NC)
	} else {
		fmt.Println("proxy daemon not installed")
	}
}

func registerProxyRoutesFromConfig(cfg *config.Config) {
	var entries []services.ProxyEntry
	for _, dir := range cfg.Repos {
		if dir.Alias != "" && dir.ProxyPort != nil {
			entries = append(entries, services.ProxyEntry{Alias: dir.Alias, Port: *dir.ProxyPort})
		}
		for svcName, svc := range dir.Services {
			if svc.ProxyPort != nil {
				entries = append(entries, services.ProxyEntry{Alias: svcName, Port: *svc.ProxyPort})
			}
		}
	}
	if len(entries) == 0 {
		return
	}

	defaultBranch := cfg.GlobalDefaultBranch()
	mainIP := services.MainIP(cfg.Session, defaultBranch)
	branchSafe := services.BranchSafe(defaultBranch)
	for i := range entries {
		entries[i].BindIP = mainIP
	}
	services.RegisterRoutesSimple(cfg.Session, branchSafe, entries)

	cwd, _ := os.Getwd()
	dirEntries, _ := os.ReadDir(cwd)
	for _, e := range dirEntries {
		if branch, ok := strings.CutPrefix(e.Name(), "workspace--"); ok && e.IsDir() {
			wsKey := "ws-" + branch
			ip := services.AllocateIP(cfg.Session, wsKey)
			bs := services.BranchSafe(branch)
			wsEntries := make([]services.ProxyEntry, len(entries))
			copy(wsEntries, entries)
			for i := range wsEntries {
				wsEntries[i].BindIP = ip
			}
			services.RegisterRoutesSimple(cfg.Session, bs, wsEntries)
		}
	}
}
