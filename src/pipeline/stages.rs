/// Stages for workspace creation pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CreateStage {
    Validate,
    Provision,
    Infra,
    Source,
    Configure,
    Setup,
    Network,
}

impl CreateStage {
    pub fn all() -> &'static [CreateStage] {
        &[
            Self::Validate,
            Self::Provision,
            Self::Infra,
            Self::Source,
            Self::Configure,
            Self::Setup,
            Self::Network,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Validate => "Validating config and hosts",
            Self::Provision => "Allocating IP and slots",
            Self::Infra => "Starting shared services and databases",
            Self::Source => "Creating git worktrees",
            Self::Configure => "Generating compose overrides and env files",
            Self::Setup => "Running setup commands",
            Self::Network => "Creating docker network",
        }
    }
}

/// Stages for workspace deletion pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeleteStage {
    Stop,
    Release,
    Cleanup,
    Remove,
    Finalize,
}

impl DeleteStage {
    pub fn all() -> &'static [DeleteStage] {
        &[
            Self::Stop,
            Self::Release,
            Self::Cleanup,
            Self::Remove,
            Self::Finalize,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Stop => "Stopping services",
            Self::Release => "Releasing IP and slots",
            Self::Cleanup => "Running pre-delete commands",
            Self::Remove => "Removing worktrees and databases",
            Self::Finalize => "Cleaning up network and folders",
        }
    }
}
