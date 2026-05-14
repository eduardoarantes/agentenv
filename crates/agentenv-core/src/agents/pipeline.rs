//! End-to-end agents pipeline: source → canonical → targets.
//!
//! Mirrors [`crate::skills::pipeline`] — same orchestration shape, soft
//! conflict handling (warn-and-skip per agent), uses the resolved-plugin
//! list to extend the source roots beyond the project's own `.claude/`.
//!
//! The Codex writer can emit a hard error (refuse-on-conflict on the
//! managed TOML file); the pipeline lets it propagate. All symlink writers
//! are soft.

use crate::agents::{canonical_io, readers, writers};
use crate::config::Config;
use crate::error::Result;
use crate::pipeline_common::configured_write_targets;
use crate::resolver::ResolvedPlugin;
use crate::state::{State, StateLink};
use std::path::{Path, PathBuf};

/// Outcome of one pipeline run.
#[derive(Debug, Default, Clone)]
pub struct PipelineReport {
    /// Path the canonical artifact was written to (`None` when the
    /// pipeline short-circuited because no agents were found).
    pub canonical_path: Option<PathBuf>,
    /// State entries the caller should fold into `.agentenv/state.json`.
    pub state_links: Vec<StateLink>,
    /// Non-fatal warnings — one per (target, dropped item) pair.
    pub warnings: Vec<String>,
}

/// Run the pipeline. Returns a no-op report when `source` is unset.
///
/// Source roots are assembled from:
/// - `<project>/.claude/agents` (always — Claude is the documented source)
/// - each resolved plugin's `<plugin>/agents` directory
pub fn run(
    config: &Config,
    project_root: &Path,
    resolved: &[ResolvedPlugin],
    old_state: &State,
) -> Result<PipelineReport> {
    let mut report = PipelineReport::default();

    let Some(source) = config.source.as_deref() else {
        return Ok(report);
    };

    let mut roots: Vec<PathBuf> = Vec::new();
    roots.push(project_root.join(".claude/agents"));
    for plugin in resolved {
        roots.push(PathBuf::from(&plugin.location).join("agents"));
    }
    let root_refs: Vec<&Path> = roots.iter().map(|p| p.as_path()).collect();

    let canonical = match readers::read(source, &root_refs)? {
        Some(c) => c,
        None => return Ok(report),
    };
    let canonical_path = canonical_io::write(project_root, &canonical)?;
    report.canonical_path = Some(canonical_path);

    let write_targets = configured_write_targets(config, source, writers::write_targets());
    for target in write_targets {
        let outcome = writers::write(&target, &canonical, project_root, old_state)?;
        report.state_links.extend(outcome.state_links);
        report.warnings.extend(outcome.report.drops);
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, MarketplaceConfig, TargetConfig};
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    fn base_config() -> Config {
        let mut marketplaces = HashMap::new();
        marketplaces.insert(
            "default".to_string(),
            MarketplaceConfig {
                path: PathBuf::from("~/.agentenv/marketplace"),
                remote: "https://example.com/m.git".to_string(),
                r#ref: "main".to_string(),
            },
        );
        Config {
            version: 1,
            marketplaces,
            plugins: vec![],
            targets: HashMap::new(),
            sync: Default::default(),
            clean: Default::default(),
            gitignore_managed_links: false,
            instruction_files: HashMap::new(),
            source: None,
        }
    }

    fn empty_target() -> TargetConfig {
        TargetConfig::default()
    }

    fn write_agent(project: &Path, name: &str, frontmatter: &str, body: &str) {
        let dir = project.join(".claude/agents");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{name}.md")),
            format!("---\n{frontmatter}\n---\n{body}"),
        )
        .unwrap();
    }

    #[test]
    fn pipeline_is_noop_when_source_unset() {
        let project = TempDir::new().unwrap();
        write_agent(project.path(), "rev", "name: rev\ndescription: r", "p");
        let mut config = base_config();
        config.targets.insert("cursor".to_string(), empty_target());
        let report = run(&config, project.path(), &[], &State::default()).unwrap();
        assert!(report.canonical_path.is_none());
        assert!(report.state_links.is_empty());
    }

    #[test]
    fn pipeline_is_noop_when_no_agents_found() {
        let project = TempDir::new().unwrap();
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config.targets.insert("cursor".to_string(), empty_target());
        let report = run(&config, project.path(), &[], &State::default()).unwrap();
        assert!(report.canonical_path.is_none());
    }

    #[test]
    fn writes_canonical_and_fans_out_to_configured_targets() {
        let project = TempDir::new().unwrap();
        write_agent(project.path(), "rev", "name: rev\ndescription: r", "p");
        write_agent(
            project.path(),
            "refactor",
            "name: refactor\ndescription: r",
            "p",
        );
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config.targets.insert("cursor".to_string(), empty_target());
        config.targets.insert("copilot".to_string(), empty_target());

        let report = run(&config, project.path(), &[], &State::default()).unwrap();
        assert!(report
            .canonical_path
            .unwrap()
            .ends_with("agents.canonical.yaml"));
        assert_eq!(report.state_links.len(), 4); // 2 agents × 2 targets
        assert!(project.path().join(".cursor/agents/rev.md").is_symlink());
        assert!(project
            .path()
            .join(".cursor/agents/refactor.md")
            .is_symlink());
        assert!(project
            .path()
            .join(".github/agents/rev.agent.md")
            .is_symlink());
        assert!(project
            .path()
            .join(".github/agents/refactor.agent.md")
            .is_symlink());
    }

    #[test]
    fn codex_writer_materializes_toml() {
        let project = TempDir::new().unwrap();
        write_agent(
            project.path(),
            "rev",
            "name: rev\ndescription: Reviews PRs\nmodel: gpt-5",
            "You are a reviewer.\n",
        );
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config.targets.insert("codex".to_string(), empty_target());

        let report = run(&config, project.path(), &[], &State::default()).unwrap();
        assert_eq!(report.state_links.len(), 1);
        assert_eq!(report.state_links[0].mode, "managed-file");

        let dest = project.path().join(".codex/agents/rev.toml");
        let raw = fs::read_to_string(&dest).unwrap();
        let parsed: toml::Value = toml::from_str(&raw).unwrap();
        assert_eq!(parsed["name"].as_str(), Some("rev"));
        assert_eq!(parsed["model"].as_str(), Some("gpt-5"));
    }

    #[test]
    fn antigravity_target_produces_only_warnings() {
        let project = TempDir::new().unwrap();
        write_agent(project.path(), "rev", "name: rev\ndescription: r", "p");
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config
            .targets
            .insert("antigravity".to_string(), empty_target());
        let report = run(&config, project.path(), &[], &State::default()).unwrap();
        assert!(report.state_links.is_empty());
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("antigravity") && w.contains("rev")));
    }
}
