mod commands;
mod config;
mod lock;
mod pipeline;
mod services;
mod tmux;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};

const VERSION: &str = "0.4.10";

#[derive(Parser)]
#[command(name = "tncli", about = "tmux-based project launcher", version = VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open interactive TUI
    Ui,
    /// Start a service or combination
    Start { target: String },
    /// Stop service/combination (no arg = stop all)
    Stop { target: Option<String> },
    /// Restart a service or combination
    Restart { target: String },
    /// Show running services
    Status,
    /// Attach to tmux session (optionally focus a service)
    Attach { target: Option<String> },
    /// Show recent output of a service
    Logs { target: String },
    /// List all services and combinations
    List,
    /// Update tncli to the latest release
    Update,
    /// Manage workspaces
    #[command(subcommand)]
    Workspace(WorkspaceCmd),
    /// Setup loopback IPs and /etc/hosts for worktrees (requires sudo)
    Setup,
    /// Database management
    #[command(subcommand)]
    Db(DbCmd),
    /// Reverse proxy for inter-service communication
    #[command(subcommand)]
    Proxy(ProxyCmd),
}

#[derive(Subcommand)]
enum ProxyCmd {
    /// Start proxy server (foreground — used by launchd/systemd)
    Serve,
    /// Start proxy daemon
    Start,
    /// Stop proxy daemon
    Stop,
    /// Restart proxy daemon (stop + clear routes + start)
    Restart,
    /// Show proxy status and routes
    Status,
    /// Install system daemon (launchd on macOS)
    Install,
    /// Uninstall system daemon
    Uninstall,
}

#[derive(Subcommand)]
enum DbCmd {
    /// Drop and recreate databases for a workspace branch (or "main")
    Reset { branch: String },
}

#[derive(Subcommand)]
enum WorkspaceCmd {
    /// Create workspace (worktrees for all dirs in a workspace)
    Create {
        workspace: String,
        branch: String,
        /// Resume from stage N (1-based, skips completed stages)
        #[arg(long)]
        from_stage: Option<usize>,
        /// Selected repos with branches: "repo1:branch1,repo2:branch2"
        #[arg(long)]
        repos: Option<String>,
    },
    /// Delete workspace
    Delete { branch: String },
    /// List active workspaces
    List,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let command = cli.command.unwrap_or(Command::Ui);

    // Update doesn't need config
    if matches!(command, Command::Update) {
        return commands::cmd_update();
    }

    let config_path = config::find_config()?;
    let cfg = config::Config::load(&config_path)?;

    match command {
        Command::Ui => tui::run_tui()?,
        Command::Start { target } => commands::cmd_start(&cfg, &config_path, &target)?,
        Command::Stop { target } => commands::cmd_stop(&cfg, target.as_deref())?,
        Command::Restart { target } => commands::cmd_restart(&cfg, &config_path, &target)?,
        Command::Status => commands::cmd_status(&cfg)?,
        Command::Attach { target } => commands::cmd_attach(&cfg, target.as_deref())?,
        Command::Logs { target } => commands::cmd_logs(&cfg, &target)?,
        Command::List => commands::cmd_list(&cfg)?,
        Command::Update => unreachable!(),
        Command::Setup => commands::cmd_setup(&cfg)?,
        Command::Workspace(ws) => match ws {
            WorkspaceCmd::Create { workspace, branch, from_stage, repos } => commands::cmd_workspace_create(&cfg, &config_path, &workspace, &branch, from_stage, repos.as_deref())?,
            WorkspaceCmd::Delete { branch } => commands::cmd_workspace_delete(&cfg, &config_path, &branch)?,
            WorkspaceCmd::List => commands::cmd_workspace_list(&cfg, &config_path)?,
        },
        Command::Db(db) => match db {
            DbCmd::Reset { branch } => commands::cmd_db_reset(&cfg, &branch)?,
        },
        Command::Proxy(proxy) => match proxy {
            ProxyCmd::Serve => services::proxy::run_proxy_server()?,
            ProxyCmd::Start => commands::cmd_proxy_start()?,
            ProxyCmd::Stop => commands::cmd_proxy_stop()?,
            ProxyCmd::Restart => commands::cmd_proxy_restart()?,
            ProxyCmd::Status => commands::cmd_proxy_status()?,
            ProxyCmd::Install => commands::cmd_proxy_install()?,
            ProxyCmd::Uninstall => commands::cmd_proxy_uninstall()?,
        },
    }

    Ok(())
}
