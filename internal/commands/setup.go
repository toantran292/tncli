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
	home, _ := os.UserHomeDir()
	_ = os.MkdirAll(filepath.Join(home, ".tncli"), 0o755)

	fmt.Printf("%s[1/4] Port allocation%s\n", Bold, NC)
	fmt.Printf("%s>>>%s port pool %d-%d (%d sessions × %d workspaces × %d ports, no sudo needed)\n",
		Green, NC, services.PoolStart, services.PoolEnd, services.MaxSessions, services.BlocksPerSession, services.BlockSize)

	// 2. /etc/hosts for shared services
	fmt.Printf("\n%s[2/4] /etc/hosts%s\n", Bold, NC)
	setupEtcHosts(cfg)

	// 3. Global gitignore
	fmt.Printf("\n%s[3/4] Global gitignore%s\n", Bold, NC)
	services.EnsureGlobalGitignore()
	fmt.Printf("%s>>>%s global gitignore configured\n", Green, NC)

	// 4. DNS
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

	fmt.Printf("\n%sSetup complete!%s No sudo required for workspace operations.\n", Green, NC)
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
		fmt.Printf("%s>>>%s no shared services — skipping\n", Green, NC)
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
		fmt.Printf("%s>>>%s /etc/hosts updated\n", Green, NC)
	}
}
