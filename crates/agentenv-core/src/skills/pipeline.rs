//! End-to-end skills pipeline: source → canonical → targets.
//!
//! Runs from `Syncer::sync` whenever `source` is set and at least one
//! configured target has a skills writer. Mirrors [`crate::hooks::pipeline`]:
//! the source is read losslessly into the canonical artifact, the artifact
//! is written under `.agentenv/`, and every configured non-source target
//! is rendered.
//!
//! Conflict handling is **soft** (warn-and-skip per skill, surfaced via
//! `PipelineReport::warnings`) rather than the hooks pipeline's hard fail.
//! Skills are per-leaf — one conflicting skill should not block the others.

use crate::config::Config;
use crate::error::Result;
use crate::pipeline_common::configured_write_targets;
use crate::resolver::ResolvedPlugin;
use crate::skills::{canonical_io, readers, writers};
use crate::state::{State, StateLink};
use std::path::{Path, PathBuf};

/// Outcome of one pipeline run.
#[derive(Debug, Default, Clone)]
pub struct PipelineReport {
    /// Path the canonical artifact was written to (`None` when the pipeline
    /// short-circuited because no skills were found).
    pub canonical_path: Option<PathBuf>,
    /// State entries the caller should fold into `.agentenv/state.json`.
    pub state_links: Vec<StateLink>,
    /// Non-fatal warnings — one per (target, dropped skill) pair.
    pub warnings: Vec<String>,
}

/// Run the pipeline. Returns a no-op report when `source` is unset.
///
/// Source roots are assembled from:
/// - `<project>/.claude/skills` (always — Claude is the documented source)
/// - each resolved plugin's `<plugin>/skills` directory
///
/// Both project and plugin content is parsed by the same Claude-shaped
/// reader, matching the v1 decision that marketplace plugins ship in
/// Claude's native shape (see plan).
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
    roots.push(project_root.join(".claude/skills"));
    for plugin in resolved {
        roots.push(PathBuf::from(&plugin.location).join("skills"));
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

    fn write_skill(project: &Path, name: &str) {
        let dir = project.join(".claude/skills").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: t\n---\nbody\n"),
        )
        .unwrap();
    }

    #[test]
    fn pipeline_is_noop_when_source_unset() {
        let project = TempDir::new().unwrap();
        write_skill(project.path(), "hello");
        let mut config = base_config();
        config.targets.insert("cursor".to_string(), empty_target());
        // source is None → opt-in, expect a silent no-op.
        let report = run(&config, project.path(), &[], &State::default()).unwrap();
        assert!(report.canonical_path.is_none());
        assert!(report.state_links.is_empty());
        assert!(!project.path().join(".cursor/skills/hello").exists());
    }

    #[test]
    fn pipeline_is_noop_when_no_skills_found() {
        let project = TempDir::new().unwrap();
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config.targets.insert("cursor".to_string(), empty_target());
        let report = run(&config, project.path(), &[], &State::default()).unwrap();
        assert!(report.canonical_path.is_none());
        assert!(report.state_links.is_empty());
    }

    #[test]
    fn writes_canonical_and_fans_out_to_configured_targets() {
        let project = TempDir::new().unwrap();
        write_skill(project.path(), "hello");
        write_skill(project.path(), "world");
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config.targets.insert("cursor".to_string(), empty_target());
        config.targets.insert("codex".to_string(), empty_target());

        let report = run(&config, project.path(), &[], &State::default()).unwrap();
        assert!(report
            .canonical_path
            .unwrap()
            .ends_with("skills.canonical.yaml"));
        // 2 skills × 2 targets = 4 state links
        assert_eq!(report.state_links.len(), 4);
        assert!(project.path().join(".cursor/skills/hello").is_symlink());
        assert!(project.path().join(".cursor/skills/world").is_symlink());
        assert!(project.path().join(".agents/skills/hello").is_symlink());
        assert!(project.path().join(".agents/skills/world").is_symlink());
    }

    #[test]
    fn ignores_targets_without_skills_writer() {
        let project = TempDir::new().unwrap();
        write_skill(project.path(), "hello");
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config.targets.insert("cursor".to_string(), empty_target());
        // claude-code as a target name is not in skills::writers::write_targets()
        // (it's the source). Adding it should be a no-op for skills.
        config
            .targets
            .insert("claude-code".to_string(), empty_target());

        let report = run(&config, project.path(), &[], &State::default()).unwrap();
        assert_eq!(report.state_links.len(), 1);
        assert_eq!(report.state_links[0].tool, "cursor");
    }

    #[test]
    fn skips_target_named_as_source() {
        let project = TempDir::new().unwrap();
        write_skill(project.path(), "hello");
        let mut config = base_config();
        // Hypothetical: cursor is both the source AND a configured target.
        // We skip cursor as a write target because it's read-only.
        config.source = Some("cursor".to_string());
        config.targets.insert("cursor".to_string(), empty_target());
        // Use claude-code reader is not available for source=cursor in v1,
        // but the source-routing only matters once we hit the reader. To
        // keep this test focused on target filtering, point at claude-code:
        config.source = Some("claude-code".to_string());
        config.targets.insert("cursor".to_string(), empty_target());
        let targets = configured_write_targets(&config, "cursor", writers::write_targets());
        assert!(targets.iter().all(|t| t != "cursor"));
    }

    #[test]
    fn warns_and_skips_when_destination_is_user_authored() {
        let project = TempDir::new().unwrap();
        write_skill(project.path(), "hello");
        // User has a real file at the cursor destination.
        let dest = project.path().join(".cursor/skills/hello");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(&dest, "user content").unwrap();

        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config.targets.insert("cursor".to_string(), empty_target());

        let report = run(&config, project.path(), &[], &State::default()).unwrap();
        assert!(
            report.warnings.iter().any(|w| w.contains("real file/dir")),
            "warnings: {:?}",
            report.warnings
        );
        // User file preserved; no state link emitted for this skill.
        assert!(!dest.is_symlink());
        assert!(report.state_links.is_empty());
    }
}
