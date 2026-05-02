package commands

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/toantran292/tncli/internal/config"
	"github.com/toantran292/tncli/internal/services"
)

func DBReset(cfg *config.Config, workspaceBranch string) error {
	cfgPath, _ := config.FindConfig()
	configDir := filepath.Dir(cfgPath)

	type dbEntry struct {
		repo, dbName string
		port         uint16
		user, pw     string
	}
	var dbs []dbEntry

	for dirName, dir := range cfg.Repos {
		wt := dir.WT()
		if wt == nil {
			continue
		}

		repoBranch := workspaceBranch
		if workspaceBranch == cfg.GlobalDefaultBranch() {
			repoBranch = cfg.DefaultBranchFor(dirName)
		} else {
			wsDir := filepath.Join(configDir, "workspace--"+workspaceBranch, dirName)
			if b := services.CurrentBranch(wsDir); b != "" {
				repoBranch = b
			}
		}

		branchSafe := services.BranchSafe(repoBranch)
		pgSvc := FindPGService(cfg)
		pgPort := uint16(5432)
		pgUser, pgPw := "postgres", "postgres"
		if pgSvc != nil {
			pgPort = services.FirstPortFromList(pgSvc.Ports)
			if pgPort == 0 {
				pgPort = 5432
			}
			if pgSvc.DBUser != "" {
				pgUser = pgSvc.DBUser
			}
			if pgSvc.DBPassword != "" {
				pgPw = pgSvc.DBPassword
			}
		}

		for _, sref := range wt.SharedServices {
			if sref.DBName != "" {
				dbName := strings.ReplaceAll(sref.DBName, "{{branch_safe}}", branchSafe)
				dbName = strings.ReplaceAll(dbName, "{{branch}}", repoBranch)
				dbs = append(dbs, dbEntry{dirName, dbName, pgPort, pgUser, pgPw})
			}
		}
		for _, dbTpl := range wt.Databases {
			dbName := strings.ReplaceAll(dbTpl, "{{branch_safe}}", branchSafe)
			dbName = strings.ReplaceAll(dbName, "{{branch}}", repoBranch)
			dbs = append(dbs, dbEntry{dirName, cfg.Session + "_" + dbName, pgPort, pgUser, pgPw})
		}
	}

	if len(dbs) == 0 {
		fmt.Printf("%sNo databases found for workspace '%s'%s\n", Yellow, workspaceBranch, NC)
		return nil
	}

	fmt.Printf("%sResetting databases for workspace '%s':%s\n", Bold, workspaceBranch, NC)
	for _, db := range dbs {
		fmt.Printf("  %s: %s\n", db.repo, db.dbName)
	}
	fmt.Println()

	var dbNames []string
	for _, db := range dbs {
		dbNames = append(dbNames, db.dbName)
	}

	host := "localhost"
	if pgSvc := FindPGService(cfg); pgSvc != nil && pgSvc.Host != "" {
		host = pgSvc.Host
	}
	port, user, pw := dbs[0].port, dbs[0].user, dbs[0].pw

	fmt.Printf("%s>>>%s dropping %d databases...", Blue, NC, len(dbNames))
	if services.DropSharedDBsBatch(host, port, dbNames, user, pw) {
		fmt.Printf(" %sok%s\n", Green, NC)
	} else {
		fmt.Printf(" %ssome failed%s\n", Yellow, NC)
	}

	fmt.Printf("%s>>>%s creating %d databases...", Blue, NC, len(dbNames))
	services.CreateSharedDBsBatch(host, port, dbNames, user, pw)
	fmt.Printf(" %sok%s\n", Green, NC)

	fmt.Printf("\n%sDatabase reset complete for workspace '%s'.%s\n", Green, workspaceBranch, NC)
	fmt.Println("Run migrations to restore schema (e.g. via TUI shortcuts).")
	return nil
}
