#![forbid(unsafe_code)]
#![warn(rust_2018_idioms, unreachable_pub)]

//! agentenv CLI - Project-scoped AI agent and plugin environment manager

use agentenv_core::init::CONFIG_FILENAME;
use agentenv_core::sync::{FetchPolicy, SyncOptions};
use agentenv_core::{
    ClaudeConfigLoader, CleanOptions, CleanReport, Cleaner, Config, ConfigLoader, Initializer,
    PluginResolver, State, SyncPlan, SyncReport, Syncer,
};
use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use std::env;
use std::path::{Path, PathBuf};

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
    Sync {
        /// Force a marketplace fetch even if `sync.refetch` is false in config
        #[arg(long, conflicts_with = "no_fetch")]
        refetch: bool,

        /// Skip the marketplace fetch entirely; require an existing local copy
        #[arg(long, conflicts_with = "refetch")]
        no_fetch: bool,
    },

    /// List configured marketplaces, plugins, and targets
    List {},

    /// Diagnose project and system state; non-zero exit on issues
    Doctor {},

    /// Show what `sync` would do without touching the filesystem
    Explain {},

    /// Remove agentenv-managed links recorded in .agentenv/state.json
    Clean {},

    /// Inspect Claude `settings.json` import (requires `use_claude_config: true`)
    #[command(name = "claude-config")]
    ClaudeConfig {
        #[command(subcommand)]
        command: ClaudeConfigCommand,
    },
}

#[derive(Parser, Debug)]
enum ClaudeConfigCommand {
    /// Show marketplaces, plugins, and hooks imported from Claude settings.json
    Show {
        /// Emit JSON instead of the human-readable table
        #[arg(long)]
        json: bool,
    },
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
        Command::Sync { refetch, no_fetch } => {
            let policy = match (refetch, no_fetch) {
                (true, false) => FetchPolicy::Force,
                (false, true) => FetchPolicy::Skip,
                _ => FetchPolicy::FromConfig,
            };
            run_sync(&project_root, SyncOptions { fetch: policy })
        },
        Command::List {} => run_list(&project_root),
        Command::Doctor {} => run_doctor(&project_root),
        Command::Explain {} => run_explain(&project_root),
        Command::Clean {} => run_clean(&project_root),
        Command::ClaudeConfig { command } => match command {
            ClaudeConfigCommand::Show { json } => run_claude_config_show(&project_root, json),
        },
    }
}

fn resolve_project_root(flag: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = flag {
        return Ok(path.to_path_buf());
    }
    env::current_dir().context("failed to read current working directory")
}

fn load_config(project_root: &Path) -> Result<Config> {
    let config_path = project_root.join(CONFIG_FILENAME);
    ConfigLoader::load_from_file(&config_path)
        .with_context(|| format!("failed to load {}", config_path.display()))
}

fn run_init(project_root: &Path, force: bool) -> Result<()> {
    let path = Initializer::write_default_config(project_root, force)?;
    println!("{} {}", "Created".green().bold(), path.display());
    println!("Edit it to add plugins and targets, then run `agentenv sync`.");
    Ok(())
}

fn run_sync(project_root: &Path, options: SyncOptions) -> Result<()> {
    let config = load_config(project_root)?;
    let report = Syncer::sync(&config, project_root, options)?;
    print_sync_report(&report);

    if report.all_succeeded() {
        Ok(())
    } else {
        anyhow::bail!("{} install(s) failed", report.failure_count());
    }
}

fn run_list(project_root: &Path) -> Result<()> {
    let config = load_config(project_root)?;

    println!("{}", "Marketplaces:".bold());
    for (namespace, marketplace) in &config.marketplaces {
        let resolved = marketplace.resolve_path(project_root)?;
        println!(
            "  {} → {} @ {} ({})",
            namespace,
            marketplace.remote,
            marketplace.r#ref,
            resolved.display()
        );
    }

    println!("\n{}", "Plugins:".bold());
    if config.plugins.is_empty() {
        println!("  (none configured)");
    }
    for plugin in &config.plugins {
        let namespace = plugin.namespace.as_deref().unwrap_or("default");
        let version = plugin.version.as_deref().unwrap_or("latest");
        println!(
            "  {} (namespace: {}, version: {})",
            plugin.name, namespace, version
        );
    }

    println!("\n{}", "Targets:".bold());
    for (name, target) in &config.targets {
        let mut caps: Vec<&str> = target.source_mappings.keys().map(String::as_str).collect();
        caps.sort();
        let label = if caps.is_empty() {
            "(no capabilities mapped)".to_string()
        } else {
            caps.join(", ")
        };
        println!("  {} → {}", name, label);
    }

    Ok(())
}

fn run_explain(project_root: &Path) -> Result<()> {
    let config = load_config(project_root)?;
    let plan = Syncer::plan(&config, project_root)?;
    print_plan(&plan);
    Ok(())
}

fn run_doctor(project_root: &Path) -> Result<()> {
    let mut issues: Vec<String> = Vec::new();

    let config_path = project_root.join(CONFIG_FILENAME);
    if !config_path.exists() {
        report_issue(
            &mut issues,
            format!("no {} found at {}", CONFIG_FILENAME, config_path.display()),
        );
        return finish_doctor(issues);
    }

    let config = match ConfigLoader::load_from_file(&config_path) {
        Ok(config) => {
            report_ok("config valid");
            config
        },
        Err(err) => {
            report_issue(&mut issues, format!("config invalid: {err}"));
            return finish_doctor(issues);
        },
    };

    for (namespace, marketplace) in &config.marketplaces {
        let resolved = match marketplace.resolve_path(project_root) {
            Ok(path) => path,
            Err(err) => {
                report_issue(
                    &mut issues,
                    format!("marketplace {namespace} path invalid: {err}"),
                );
                continue;
            },
        };
        if resolved.exists() {
            report_ok(format!(
                "marketplace {namespace} present at {}",
                resolved.display()
            ));
        } else {
            report_issue(
                &mut issues,
                format!(
                    "marketplace {namespace} missing at {} (run `agentenv sync` to clone)",
                    resolved.display()
                ),
            );
        }
    }

    match PluginResolver::resolve_all(&config, project_root) {
        Ok(plugins) => report_ok(format!("{} plugin(s) resolved", plugins.len())),
        Err(err) => report_issue(&mut issues, format!("plugin resolution failed: {err}")),
    }

    let state = State::load(project_root)?;
    let broken: Vec<_> = state
        .links
        .iter()
        .filter(|link| !link.target.exists() && !link.target.is_symlink())
        .collect();
    if state.links.is_empty() {
        report_ok("no managed links recorded yet");
    } else if broken.is_empty() {
        report_ok(format!(
            "{} managed link(s), none broken",
            state.links.len()
        ));
    } else {
        report_issue(
            &mut issues,
            format!(
                "{} of {} managed link(s) broken — run `agentenv sync` to repair",
                broken.len(),
                state.links.len()
            ),
        );
    }

    finish_doctor(issues)
}

fn run_claude_config_show(project_root: &Path, as_json: bool) -> Result<()> {
    let import = ClaudeConfigLoader::load(project_root)
        .context("failed to load Claude settings.json files")?;

    if as_json {
        let out = serde_json::to_string_pretty(&import)
            .context("failed to serialize Claude import as JSON")?;
        println!("{out}");
        return Ok(());
    }

    println!("{}", "Marketplaces (from Claude):".bold());
    if import.marketplaces.is_empty() {
        println!("  (none)");
    }
    let mut mp_keys: Vec<&String> = import.marketplaces.keys().collect();
    mp_keys.sort();
    for name in mp_keys {
        let mp = &import.marketplaces[name];
        println!("  {} → {} (ref: {})", name, mp.remote, mp.r#ref);
    }

    println!("\n{}", "Plugins (from Claude):".bold());
    if import.plugins.is_empty() {
        println!("  (none)");
    }
    for plugin in &import.plugins {
        let namespace = plugin.namespace.as_deref().unwrap_or("default");
        println!("  {} (namespace: {})", plugin.name, namespace);
    }

    println!("\n{}", "Hooks (from Claude):".bold());
    match &import.hooks {
        serde_json::Value::Null => println!("  (none)"),
        other => {
            let pretty = serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string());
            for line in pretty.lines() {
                println!("  {line}");
            }
        },
    }

    Ok(())
}

fn run_clean(project_root: &Path) -> Result<()> {
    // Best-effort config load: clean still works without an `.agentrc.yaml`,
    // it just falls back to the default options.
    let options = load_config(project_root)
        .map(|config| CleanOptions {
            prune_empty_dirs: config.clean.prune_empty_dirs,
        })
        .unwrap_or_default();
    let report = Cleaner::clean(project_root, options)?;
    print_clean_report(&report);
    Ok(())
}

fn report_ok(message: impl Into<String>) {
    println!("  {} {}", "✓".green(), message.into());
}

fn report_issue(issues: &mut Vec<String>, message: String) {
    println!("  {} {}", "✗".red(), message);
    issues.push(message);
}

fn finish_doctor(issues: Vec<String>) -> Result<()> {
    if issues.is_empty() {
        println!("\n{}", "all checks passed".green().bold());
        Ok(())
    } else {
        anyhow::bail!("{} doctor issue(s) found", issues.len());
    }
}

fn print_sync_report(report: &SyncReport) {
    for result in &report.installs {
        let label = if result.success {
            "linked".green().to_string()
        } else {
            "failed".red().to_string()
        };
        println!("  {} [{}] {}", label, result.action.tool, result.message);
    }
    for stale in &report.stale_removed {
        println!(
            "  {} {} (no longer in plan)",
            "removed".cyan(),
            stale.target.display()
        );
    }
    for warning in &report.warnings {
        println!("  {} {}", "warn".yellow(), warning);
    }

    let summary = format!(
        "{} succeeded, {} failed, {} stale removed, {} warning(s)",
        report.success_count(),
        report.failure_count(),
        report.stale_removed.len(),
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

fn print_plan(plan: &SyncPlan) {
    if plan.actions.is_empty() {
        println!("(no actions)");
    }
    for action in &plan.actions {
        println!(
            "  {} [{}] {} → {} ({}, plugin: {})",
            "would link".cyan(),
            action.tool,
            action.source.display(),
            action.target.display(),
            action.mode,
            action.plugin
        );
    }
    for warning in &plan.warnings {
        println!("  {} {}", "warn".yellow(), warning);
    }
}

fn print_clean_report(report: &CleanReport) {
    for link in &report.removed {
        println!("  {} {}", "removed".green(), link.target.display());
    }
    for (link, reason) in &report.skipped {
        println!(
            "  {} {} ({reason})",
            "skipped".yellow(),
            link.target.display()
        );
    }
    for dir in &report.pruned_dirs {
        println!("  {} {}", "pruned".cyan(), dir.display());
    }
    println!(
        "{} removed, {} skipped, {} dir(s) pruned",
        report.removed.len(),
        report.skipped.len(),
        report.pruned_dirs.len()
    );
}
