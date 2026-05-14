//! End-to-end hooks pipeline: source → canonical → targets.
//!
//! Runs as a post-step inside `Syncer::sync` whenever at least one
//! configured target is a v1 hook write target (`cursor` or `codex`). When
//! the pipeline is active, `source` is mandatory and must be one of the
//! v1 hook source targets (currently only `claude-code`).
//!
//! The pipeline is conservative on failures:
//! * **Refuse-on-conflict** errors in the writers propagate as hard errors
//!   from `agentenv sync` — that is the user's signal to clean up the
//!   destination file or pick a different source.
//! * Per-event drops (a canonical event with no native counterpart on the
//!   target) are surfaced as warnings via [`PipelineReport`].

use crate::config::Config;
use crate::error::Result;
use crate::hooks::{canonical_io, readers, writers};
use std::path::{Path, PathBuf};

/// Outcome of one pipeline run.
#[derive(Debug, Default, Clone)]
pub struct PipelineReport {
    /// Path the canonical artifact was written to (`None` when the pipeline
    /// short-circuited because source had no hooks).
    pub canonical_path: Option<PathBuf>,
    /// Non-fatal warnings, one per (target, dropped event) pair.
    pub warnings: Vec<String>,
}

/// Run the pipeline for `config`.
///
/// `source` is the opt-in: when unset, the pipeline is a no-op and other
/// agentenv flows (skills/agents sync, instruction-file propagation) keep
/// working unchanged. When set, the pipeline reads the source target's
/// native hooks, writes the canonical artifact, and renders every
/// configured non-source v1 hook write target.
///
/// Caller decides where in `Syncer::sync` this runs; today it executes
/// after instruction-file propagation and before the stale-cleanup pass.
pub fn run(config: &Config, project_root: &Path) -> Result<PipelineReport> {
    let mut report = PipelineReport::default();

    let Some(source) = config.source.as_deref() else {
        return Ok(report);
    };

    let write_targets = config.hook_write_targets();
    if write_targets.is_empty() {
        report.warnings.push(format!(
            "`source: {source}` is set but no v1 hook write target (cursor, codex) is configured \
             — hooks pipeline is a no-op."
        ));
        return Ok(report);
    }

    let canonical = match readers::read(source, project_root)? {
        Some(c) => c,
        None => return Ok(report),
    };

    let canonical_path = canonical_io::write(project_root, &canonical)?;
    report.canonical_path = Some(canonical_path);

    for target in write_targets {
        let write_report = writers::write(&target, &canonical, project_root)?;
        for drop_reason in write_report.drops {
            report.warnings.push(drop_reason);
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, MarketplaceConfig, TargetConfig};
    use crate::hooks::writers::cursor::destination as cursor_dest;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    /// Write `<project>/.claude/settings.json` with `{"hooks": value}`.
    fn write_project_hooks(project: &Path, hooks: serde_json::Value) {
        let claude_dir = project.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.json"),
            serde_json::json!({ "hooks": hooks }).to_string(),
        )
        .unwrap();
    }

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

    #[test]
    fn pipeline_is_noop_when_no_hook_write_target() {
        let project = TempDir::new().unwrap();
        let config = base_config();
        write_project_hooks(
            project.path(),
            serde_json::json!({
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "x"}]}
                ]
            }),
        );
        // No cursor / codex target → pipeline must not run.
        let report = run(&config, project.path()).unwrap();
        assert!(report.canonical_path.is_none());
        assert!(!project.path().join(".cursor/hooks.json").exists());
    }

    #[test]
    fn pipeline_is_noop_when_source_unset() {
        let project = TempDir::new().unwrap();
        let mut config = base_config();
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        write_project_hooks(
            project.path(),
            serde_json::json!({
                "Stop": [{"matcher": ".*", "hooks": [{"type": "command", "command": "x"}]}]
            }),
        );
        // source is None → pipeline is opt-in, expect a silent no-op.
        let report = run(&config, project.path()).unwrap();
        assert!(report.canonical_path.is_none());
        assert!(!project.path().join(".cursor/hooks.json").exists());
    }

    #[test]
    fn pipeline_warns_when_source_set_but_no_write_target() {
        let project = TempDir::new().unwrap();
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        // No cursor / codex target.
        let report = run(&config, project.path()).unwrap();
        assert!(report.canonical_path.is_none());
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("no v1 hook write target")),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn pipeline_skips_source_target_writer() {
        let _project = TempDir::new().unwrap();
        let mut config = base_config();
        // Pretend cursor is both a configured target AND the source.
        config.source = Some("cursor".to_string());
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        // (We won't actually run this — the reader will reject cursor as
        // source in v1. The point is just that `hook_write_targets`
        // already removes the source from iteration before we even hit
        // the reader.)
        let targets = config.hook_write_targets();
        assert!(targets.iter().all(|t| t != "cursor"));
    }

    #[test]
    fn pipeline_writes_canonical_and_cursor_file_on_happy_path() {
        let project = TempDir::new().unwrap();
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        write_project_hooks(
            project.path(),
            serde_json::json!({
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "echo bash"}]}
                ]
            }),
        );
        let report = run(&config, project.path()).unwrap();
        assert!(report.canonical_path.is_some());
        assert!(cursor_dest(project.path()).exists());
    }

    #[test]
    fn pipeline_short_circuits_when_source_has_no_hooks() {
        let project = TempDir::new().unwrap();
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        // Deliberately do NOT create .claude/settings.json — reader returns None.
        let report = run(&config, project.path()).unwrap();
        assert!(report.canonical_path.is_none());
        assert!(!cursor_dest(project.path()).exists());
    }

    #[test]
    fn pipeline_surfaces_drop_warnings() {
        let project = TempDir::new().unwrap();
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        // PreCompact has no cursor counterpart — must be reported.
        write_project_hooks(
            project.path(),
            serde_json::json!({
                "PreCompact": [
                    {"matcher": ".*", "hooks": [{"type": "command", "command": "echo c"}]}
                ]
            }),
        );
        let report = run(&config, project.path()).unwrap();
        assert!(!report.warnings.is_empty());
        assert!(report.warnings.iter().any(|w| w.contains("PreCompact")));
    }

    #[test]
    fn pipeline_unused_target_config_compiles() {
        // Sanity: building a TargetConfig literal at the test site keeps
        // module imports honest after refactors.
        let _ = TargetConfig::default();
    }
}
