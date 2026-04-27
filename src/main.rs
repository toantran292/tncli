mod commands;
mod config;
mod lock;
mod tmux;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};

const VERSION: &str = "0.1.0";

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let command = cli.command.unwrap_or(Command::Ui);

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
    }

    Ok(())
}
