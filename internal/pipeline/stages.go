package pipeline

type CreateStage int

const (
	StageValidate CreateStage = iota
	StageProvision
	StageInfra
	StageSource
	StageConfigure
	StageSetup
	StageNetwork
)

var AllCreateStages = []CreateStage{
	StageValidate, StageProvision, StageInfra, StageSource,
	StageConfigure, StageSetup, StageNetwork,
}

func (s CreateStage) Label() string {
	switch s {
	case StageValidate:
		return "Validating config and hosts"
	case StageProvision:
		return "Allocating IP and slots"
	case StageInfra:
		return "Starting shared services and databases"
	case StageSource:
		return "Creating git worktrees (parallel)"
	case StageConfigure:
		return "Configuring repos (parallel)"
	case StageSetup:
		return "Running setup commands (parallel)"
	case StageNetwork:
		return "Creating docker network"
	default:
		return "Unknown"
	}
}

type DeleteStage int

const (
	StageStop DeleteStage = iota
	StageRelease
	StageCleanup
	StageRemove
	StageFinalize
)

var AllDeleteStages = []DeleteStage{
	StageStop, StageRelease, StageCleanup, StageRemove, StageFinalize,
}

func (s DeleteStage) Label() string {
	switch s {
	case StageStop:
		return "Stopping services"
	case StageRelease:
		return "Releasing IP and slots"
	case StageCleanup:
		return "Running pre-delete commands"
	case StageRemove:
		return "Removing worktrees and databases"
	case StageFinalize:
		return "Cleaning up network and folders"
	default:
		return "Unknown"
	}
}
