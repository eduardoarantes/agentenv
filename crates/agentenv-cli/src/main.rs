#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

//! agentenv CLI - Project-scoped AI agent and plugin environment manager

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "agentenv")]
#[command(about = "Project-scoped AI agent and plugin environment manager", long_about = None)]
#[command(version)]
struct Args {
    /// Path to project root (defaults to current directory)
    #[arg(short, long)]
    project: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Parser, Debug)]
enum Command {
    /// Initialize agentenv in current project
    Init {},

    /// Sync plugins and targets
    Sync {},

    /// List configured plugins and targets
    List {},

    /// Check system and project configuration
    Doctor {},

    /// Explain agentenv configuration
    Explain {},

    /// Clean up managed symlinks
    Clean {},
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("agentenv=info".parse()?),
        )
        .init();

    let args = Args::parse();

    match args.command {
        Command::Init {} => {
            println!("Initializing agentenv...");
            // TODO: Implement init command
        },
        Command::Sync {} => {
            println!("Syncing plugins...");
            // TODO: Implement sync command
        },
        Command::List {} => {
            println!("Listing plugins and targets...");
            // TODO: Implement list command
        },
        Command::Doctor {} => {
            println!("Running doctor...");
            // TODO: Implement doctor command
        },
        Command::Explain {} => {
            println!("Explaining configuration...");
            // TODO: Implement explain command
        },
        Command::Clean {} => {
            println!("Cleaning up...");
            // TODO: Implement clean command
        },
    }

    Ok(())
}
