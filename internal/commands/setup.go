package commands

import (
	"fmt"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
)

func Setup(cfg *config.Config) error {
	fmt.Printf("%s[1/2] Port allocation%s\n", Bold, NC)
	fmt.Printf("%s>>>%s port pool %d-%d (%d concurrent sessions × %d workspaces × %d ports, no sudo)\n",
		Green, NC, services.PoolStart, services.PoolEnd, services.MaxSlots, services.MaxBlocks, services.BlockSize)

	fmt.Printf("\n%s[2/2] Global gitignore%s\n", Bold, NC)
	services.EnsureGlobalGitignore()
	fmt.Printf("%s>>>%s global gitignore configured\n", Green, NC)

	fmt.Printf("\n%sSetup complete!%s No sudo required.\n", Green, NC)
	return nil
}
