#![forbid(unsafe_code)]
#![warn(rust_2018_idioms, unreachable_pub)]

//! agentenv CLI - Project-scoped AI agent and plugin environment manager

use agentenv_core::init::CONFIG_FILENAME;
use agentenv_core::{ConfigLoader, Initializer, SyncReport, Syncer};
use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use std::env;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "agentenv")]
#[command(about = "Project-scoped AI agent and plugin environment manager", long_about = None)]
#[command(version)]
struct Args {
    /// Path to project root (defaults to current directory)
    #[arg(short, long, global = true)]
    project: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Parser, Debug)]
enum Command {
    /// Initialize agentenv in current project
    Init {
        /// Overwrite an existing `.agentrc.yaml` if present
        #[arg(long)]
        force: bool,
    },

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
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("agentenv=info".parse()?),
        )
        .init();

    let args = Args::parse();
    let project_root = resolve_project_root(args.project.as_deref())?;

    match args.command {
        Command::Init { force } => run_init(&project_root, force),
        Command::Sync {} => run_sync(&project_root),
        Command::List {} => not_implemented("list"),
        Command::Doctor {} => not_implemented("doctor"),
        Command::Explain {} => not_implemented("explain"),
        Command::Clean {} => not_implemented("clean"),
    }
}

fn resolve_project_root(flag: Option<&std::path::Path>) -> Result<PathBuf> {
    if let Some(path) = flag {
        return Ok(path.to_path_buf());
    }
    env::current_dir().context("failed to read current working directory")
}

fn run_init(project_root: &std::path::Path, force: bool) -> Result<()> {
    let path = Initializer::write_default_config(project_root, force)?;
    println!("{} {}", "Created".green().bold(), path.display());
    println!("Edit it to add plugins and targets, then run `agentenv sync`.");
    Ok(())
}

fn run_sync(project_root: &std::path::Path) -> Result<()> {
    let config_path = project_root.join(CONFIG_FILENAME);
    let config = ConfigLoader::load_from_file(&config_path)
        .with_context(|| format!("failed to load {}", config_path.display()))?;

    let report = Syncer::sync(&config, project_root)?;
    print_report(&report);

    if report.all_succeeded() {
        Ok(())
    } else {
        anyhow::bail!(
            "{} install(s) failed",
            report.failure_count()
        );
    }
}

fn print_report(report: &SyncReport) {
    for result in &report.installs {
        let label = if result.success {
            "linked".green().to_string()
        } else {
            "failed".red().to_string()
        };
        println!(
            "  {} [{}] {}",
            label,
            result.action.tool,
            result.message,
        );
    }
    for warning in &report.warnings {
        println!("  {} {}", "warn".yellow(), warning);
    }

    let summary = format!(
        "{} succeeded, {} failed, {} warning(s)",
        report.success_count(),
        report.failure_count(),
        report.warnings.len()
    );
    if report.all_succeeded() && report.warnings.is_empty() {
        println!("{}", summary.green().bold());
    } else if report.all_succeeded() {
        println!("{}", summary.yellow());
    } else {
        println!("{}", summary.red().bold());
    }
}

fn not_implemented(name: &str) -> Result<()> {
    anyhow::bail!("`{}` command is not implemented yet", name)
}
