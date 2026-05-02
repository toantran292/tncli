package commands

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
)

func Setup(cfg *config.Config) error {
	// 1. Setup loopback IPs
	subnetCount := services.SetupSubnetCount
	hostMax := services.SetupHostMax
	var ips []string
	for subnet := 1; subnet <= int(subnetCount); subnet++ {
		for host := 2; host <= int(hostMax); host++ {
			ips = append(ips, fmt.Sprintf("127.0.%d.%d", subnet, host))
		}
	}
	hostsPerSubnet := int(hostMax) - 1
	total := len(ips)

	alreadySetup := exec.Command("ping", "-c", "1", "-W", "1", "127.0.1.2").Run() == nil
	if alreadySetup {
		fmt.Printf("%s>>>%s loopback IPs already configured (%d IPs, %d subnets × %d hosts)\n", Green, NC, total, subnetCount, hostsPerSubnet)
	} else {
		fmt.Printf("%sSetting up loopback IPs (127.0.{1..%d}.{2..%d})...%s\n", Bold, subnetCount, hostMax, NC)
		var cmds []string
		for _, ip := range ips {
			cmds = append(cmds, fmt.Sprintf("ifconfig lo0 alias %s 2>/dev/null", ip))
		}
		if exec.Command("sudo", "sh", "-c", strings.Join(cmds, "; ")).Run() == nil {
			fmt.Printf("%s>>>%s %s%d loopback IPs configured%s\n", Green, NC, Green, total, NC)
		} else {
			fmt.Fprintf(os.Stderr, "%swarning:%s failed to setup loopback IPs (sudo required)\n", Yellow, NC)
		}
		_ = exec.Command("sudo", "dscacheutil", "-flushcache").Run()
		_ = exec.Command("sudo", "killall", "-HUP", "mDNSResponder").Run()
	}

	// 1b. LaunchDaemon
	home, _ := os.UserHomeDir()
	scriptPath := filepath.Join(home, ".tncli/setup-loopback.sh")
	plistPath := "/Library/LaunchDaemons/com.tncli.loopback.plist"
	_ = os.MkdirAll(filepath.Join(home, ".tncli"), 0o755)
	var scriptLines []string
	scriptLines = append(scriptLines, "#!/bin/sh")
	for _, ip := range ips {
		scriptLines = append(scriptLines, fmt.Sprintf("ifconfig lo0 alias %s 2>/dev/null", ip))
	}
	_ = os.WriteFile(scriptPath, []byte(strings.Join(scriptLines, "\n")+"\n"), 0o755)

	if _, err := os.Stat(plistPath); err == nil {
		fmt.Printf("%s>>>%s LaunchDaemon already installed\n", Green, NC)
	} else {
		plist := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tncli.loopback</string>
    <key>ProgramArguments</key>
    <array>
        <string>%s</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
`, scriptPath)
		tmpPlist := filepath.Join(home, ".tncli/com.tncli.loopback.plist")
		_ = os.WriteFile(tmpPlist, []byte(plist), 0o644)
		if exec.Command("sudo", "cp", tmpPlist, plistPath).Run() == nil {
			_ = exec.Command("sudo", "chown", "root:wheel", plistPath).Run()
			fmt.Printf("%s>>>%s LaunchDaemon installed\n", Green, NC)
		}
		_ = os.Remove(tmpPlist)
	}

	// 2. /etc/hosts
	setupEtcHosts(cfg)

	// 3. Global gitignore
	services.EnsureGlobalGitignore()
	fmt.Printf("%s>>>%s global gitignore configured\n", Green, NC)

	// 4. Caddy
	if exec.Command("caddy", "version").Run() == nil {
		fmt.Printf("%s>>>%s caddy already installed\n", Green, NC)
	} else {
		fmt.Printf("%sInstalling caddy...%s\n", Bold, NC)
		if exec.Command("brew", "install", "caddy").Run() == nil {
			fmt.Printf("%s>>>%s %scaddy installed%s\n", Green, NC, Green, NC)
		} else {
			fmt.Fprintf(os.Stderr, "%swarning:%s failed to install caddy\n", Yellow, NC)
		}
	}

	// 5. DNS
	fmt.Printf("\n%s[4/4] DNS (*.tncli.test → 127.0.0.1)%s\n", Bold, NC)
	dnsStatus := services.GetDNSStatus()
	if dnsStatus.IsReady() {
		fmt.Printf("%s>>>%s dnsmasq already configured and running\n", Green, NC)
		resolved := false
		for i := 0; i < 3; i++ {
			if services.VerifyResolution() {
				resolved = true
				break
			}
			time.Sleep(time.Second)
		}
		if resolved {
			fmt.Printf("%s>>>%s *.tncli.test resolves correctly\n", Green, NC)
		} else {
			fmt.Fprintf(os.Stderr, "%swarning:%s DNS resolution not working — try: sudo brew services restart dnsmasq\n", Yellow, NC)
		}
	} else {
		actions, err := services.SetupDnsmasq()
		if err == nil {
			for _, a := range actions {
				fmt.Printf("%s>>>%s %s\n", Green, NC, a)
			}
			time.Sleep(2 * time.Second)
			if services.VerifyResolution() {
				fmt.Printf("%s>>>%s *.tncli.test resolves correctly\n", Green, NC)
			}
		} else {
			fmt.Fprintf(os.Stderr, "%swarning:%s dnsmasq setup failed: %v\n", Yellow, NC, err)
		}
	}

	fmt.Printf("\n%sSetup complete!%s\n", Green, NC)
	return nil
}

func setupEtcHosts(cfg *config.Config) {
	var hostnames []string
	for name, svc := range cfg.SharedServices {
		host := svc.Host
		if host == "" {
			host = fmt.Sprintf("%s.%s.tncli.test", cfg.Session, name)
		}
		if !services.ContainsStr(hostnames, host) {
			hostnames = append(hostnames, host)
		}
	}
	if len(hostnames) == 0 {
		return
	}
	hostsContent, _ := os.ReadFile("/etc/hosts")
	var missing []string
	for _, h := range hostnames {
		if !strings.Contains(string(hostsContent), h) {
			missing = append(missing, h)
		}
	}
	if len(missing) == 0 {
		fmt.Printf("%s>>>%s /etc/hosts already configured\n", Green, NC)
		return
	}
	fmt.Printf("%sAdding to /etc/hosts:%s\n", Bold, NC)
	var entries []string
	for _, h := range missing {
		fmt.Printf("  127.0.0.1 %s\n", h)
		entries = append(entries, "127.0.0.1 "+h)
	}
	cmd := fmt.Sprintf("echo '\n# tncli shared services\n%s' >> /etc/hosts", strings.Join(entries, "\n"))
	if exec.Command("sudo", "sh", "-c", cmd).Run() == nil {
		fmt.Printf("%s>>>%s %s/etc/hosts updated%s\n", Green, NC, Green, NC)
	}
}
