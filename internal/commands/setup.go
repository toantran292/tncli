package commands

import (
	"fmt"
	"os"
	"os/exec"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
)

func Setup(cfg *config.Config) error {
	fmt.Printf("%s[1/3] Port allocation%s\n", Bold, NC)
	fmt.Printf("%s>>>%s port pool %d-%d (%d sessions × %d workspaces × %d ports)\n",
		Green, NC, services.PoolStart, services.PoolEnd, services.MaxSlots, services.MaxBlocks, services.BlockSize)

	fmt.Printf("\n%s[2/3] /etc/hosts for shared services%s\n", Bold, NC)
	if len(cfg.SharedServices) > 0 {
		setupEtcHosts(cfg)
	} else {
		fmt.Printf("  %sno shared services configured%s\n", Dim, NC)
	}

	fmt.Printf("\n%s[3/3] Global gitignore%s\n", Bold, NC)
	services.EnsureGlobalGitignore()
	fmt.Printf("%s>>>%s global gitignore configured\n", Green, NC)

	fmt.Printf("\n%sSetup complete!%s\n", Green, NC)
	return nil
}

func setupEtcHosts(cfg *config.Config) {
	var hostnames []string
	for name := range cfg.SharedServices {
		hostnames = append(hostnames, name)
	}

	missing := services.CheckEtcHosts(hostnames)
	if len(missing) == 0 {
		fmt.Printf("  %s>>>%s /etc/hosts already has: %s\n", Green, NC, strings.Join(hostnames, ", "))
		return
	}

	fmt.Printf("  adding to /etc/hosts: %s\n", strings.Join(missing, ", "))

	line := "127.0.0.1 " + strings.Join(missing, " ") + " # tncli shared services"
	cmd := exec.Command("sudo", "sh", "-c", fmt.Sprintf("echo '%s' >> /etc/hosts", line))
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		fmt.Printf("  %sfailed:%s %v\n", Yellow, NC, err)
		fmt.Printf("  add manually: %s\n", line)
		return
	}
	fmt.Printf("  %s>>>%s done\n", Green, NC)
}
