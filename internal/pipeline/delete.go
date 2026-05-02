package pipeline

import (
	"os/exec"
	"strings"

	"github.com/toantran292/tncli/internal/services"
)

func ExecuteDeleteStage(stage DeleteStage, ctx *DeleteContext) error {
	switch stage {
	case StageStop:
		return nil // Stop handled by caller before building context
	case StageRelease:
		return deleteStageRelease(ctx)
	case StageCleanup:
		return deleteStageCleanup(ctx)
	case StageRemove:
		return deleteStageRemove(ctx)
	case StageFinalize:
		return deleteStageFinalize(ctx)
	}
	return nil
}

func deleteStageRelease(ctx *DeleteContext) error {
	wsKey := "ws-" + ctx.Branch
	for name := range ctx.Config.SharedServices {
		services.ReleaseSlot(name, wsKey)
	}
	// IP released in StageRemove after worktrees are gone (prevents race with TUI scan)
	return nil
}

func deleteStageCleanup(ctx *DeleteContext) error {
	for _, item := range ctx.CleanupItems {
		if len(item.PreDelete) == 0 {
			continue
		}
		if !isDir(item.WtPath) {
			continue
		}
		combined := strings.Join(item.PreDelete, " && ")
		cmd := exec.Command("zsh", "-c", combined)
		cmd.Dir = item.WtPath
		_ = cmd.Run()
	}
	return nil
}

func deleteStageRemove(ctx *DeleteContext) error {
	for _, item := range ctx.CleanupItems {
		_ = services.RemoveWorktree(item.DirPath, item.WtPath, item.WtBranch)
	}

	if len(ctx.DBsToDrop) > 0 {
		var dbNames []string
		for _, db := range ctx.DBsToDrop {
			dbNames = append(dbNames, db.DBName)
		}
		first := ctx.DBsToDrop[0]
		services.DropSharedDBsBatch(first.Host, first.Port, dbNames, first.User, first.Password)
	}

	// Release IP after worktrees are removed (safe from TUI scan race)
	services.ReleaseIP(ctx.ConfigDir, "ws-"+ctx.Branch)
	return nil
}

func deleteStageFinalize(ctx *DeleteContext) error {
	services.RemoveDockerNetwork(ctx.Network)
	services.DeleteWorkspaceFolder(ctx.ConfigDir, ctx.Branch)
	branchSafe := services.BranchSafe(ctx.Branch)
	services.UnregisterRoutes(branchSafe)
	return nil
}
