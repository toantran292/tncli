package services

// DockerRunner defines the interface for docker operations.
// Tests can replace DefaultDocker with a mock.
type DockerRunner interface {
	CreateNetwork(name string) error
	RemoveNetwork(name string)
	ForceCleanup(projectName string)
}

// DefaultDocker is the docker runner used by package-level functions.
var DefaultDocker DockerRunner = &ExecDockerRunner{}

// ExecDockerRunner implements DockerRunner via exec.Command.
type ExecDockerRunner struct{}
