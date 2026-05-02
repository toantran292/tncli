package pipeline

import (
	"github.com/toantran292/tncli/internal/services"
)

// CreateState holds mutable state accumulated across pipeline stages.
type CreateState struct {
	WsFolder    string
	NetworkName string
	BranchSafe  string
	BindIP      string
	WtDirs      []services.DirMapping
}

func NewCreateState(ctx *CreateContext) *CreateState {
	return &CreateState{
		NetworkName: "tncli-ws-" + ctx.Branch,
		BranchSafe:  services.BranchSafe(ctx.Branch),
		BindIP:      ctx.BindIP,
	}
}

// ExecuteCreateStage dispatches a single pipeline stage.
func ExecuteCreateStage(stage CreateStage, ctx *CreateContext, state *CreateState) error {
	switch stage {
	case StageValidate:
		return stageValidate(ctx)
	case StageProvision:
		return stageProvision(ctx, state)
	case StageInfra:
		return stageInfra(ctx, state)
	case StageSource:
		return stageSourceParallel(ctx, state)
	case StageConfigure:
		return stageConfigureParallel(ctx, state)
	case StageSetup:
		return stageSetupParallel(ctx, state)
	case StageNetwork:
		return stageNetworkCreate(ctx, state)
	}
	return nil
}
