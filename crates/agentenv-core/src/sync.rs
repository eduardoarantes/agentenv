//! Sync engine: source → canonical → per-target writers.
//!
//! Drives the three capability pipelines (skills, agents, hooks) plus
//! instruction-file propagation, then folds every owned link into
//! `.agentenv/state.json` and runs the stale-cleanup pass. Marketplace
//! fetch is the caller's responsibility — by the time `Syncer::sync` runs,
//! every marketplace `path` referenced by `Config` must already be
//! populated on disk.

use crate::config::Config;
use crate::error::{Error, Result};
use crate::marketplace::{EnsureBehavior, EnsureOutcome, Marketplace};
use crate::resolver::PluginResolver;
use crate::state::{State, StateLink};
use crate::symlink::{InstallAction, InstallResult, SymlinkManager};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Sync engine.
#[derive(Debug)]
pub struct Syncer;

/// Marketplace fetch policy for a sync run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FetchPolicy {
    /// Honor `sync.refetch` from the loaded config (default).
    #[default]
    FromConfig,
    /// Always fetch existing marketplaces, regardless of config.
    Force,
    /// Never touch the network. Error if a marketplace is missing locally.
    Skip,
}

/// Options for a sync run.
#[derive(Debug, Default, Clone, Copy)]
pub struct SyncOptions {
    pub fetch: FetchPolicy,
}

/// One entry in a sync plan: where a managed link will be created and
/// what it will point at.
#[derive(Debug, Clone)]
pub struct PlannedAction {
    pub source: PathBuf,
    pub target: PathBuf,
    pub mode: String,
    pub tool: String,
    pub plugin: String,
}

/// What a sync would do, without doing it. Drives `agentenv explain`.
///
/// In the source-driven model, predicting the full canonical-driven
/// install set without running the pipelines is non-trivial. v1's
/// `plan()` only surfaces the instruction-file propagation plan plus a
/// note; `agentenv sync` is the source of truth.
#[derive(Debug, Default)]
pub struct SyncPlan {
    pub actions: Vec<PlannedAction>,
    pub warnings: Vec<String>,
}

/// Outcome of a sync run.
#[derive(Debug, Default)]
pub struct SyncReport {
    pub installs: Vec<InstallResult>,
    pub stale_removed: Vec<StateLink>,
    pub warnings: Vec<String>,
}

impl SyncReport {
    pub fn success_count(&self) -> usize {
        self.installs.iter().filter(|r| r.success).count()
    }
    pub fn failure_count(&self) -> usize {
        self.installs.iter().filter(|r| !r.success).count()
    }
    pub fn all_succeeded(&self) -> bool {
        self.failure_count() == 0
    }
}

impl Syncer {
    /// Run every capability pipeline + instruction-file propagation.
    ///
    /// 1. Ensure marketplaces are present (and optionally refetched).
    /// 2. Resolve plugins to get their on-disk locations.
    /// 3. Read source-of-truth content for each capability into the
    ///    canonical artifact under `.agentenv/`.
    /// 4. Run every per-target writer; collect state links + warnings.
    /// 5. Propagate instruction files (CLAUDE.md → AGENTS.md, etc.).
    /// 6. Reconcile `.agentenv/state.json` (writes new links, removes
    ///    stale ones, refreshes the optional gitignore block).
    pub fn sync<P: AsRef<Path>>(
        config: &Config,
        project_root: P,
        options: SyncOptions,
    ) -> Result<SyncReport> {
        let project_root = project_root.as_ref();
        let mut report = SyncReport::default();

        let behavior = ensure_behavior(options.fetch, config.sync.refetch);
        for (namespace, marketplace) in &config.marketplaces {
            let outcome = Marketplace::ensure(marketplace, project_root, behavior)?;
            if let EnsureOutcome::FetchFailedReused(reason) = outcome {
                report.warnings.push(format!(
                    "marketplace {namespace}: refetch failed, reusing local copy ({reason})"
                ));
            }
        }

        let resolved = PluginResolver::resolve_all(config, project_root)?;
        let old_state = State::load(project_root)?;
        let mut new_state = State::default();

        // Skills.
        let skills_report =
            crate::skills::pipeline::run(config, project_root, &resolved, &old_state)?;
        for link in skills_report.state_links {
            report.installs.push(success_install(&link));
            new_state.links.push(link);
        }
        report.warnings.extend(skills_report.warnings);

        // Agents.
        let agents_report =
            crate::agents::pipeline::run(config, project_root, &resolved, &old_state)?;
        for link in agents_report.state_links {
            report.installs.push(success_install(&link));
            new_state.links.push(link);
        }
        report.warnings.extend(agents_report.warnings);

        // Hooks (unchanged — does not produce state links; writes files
        // directly and surfaces warnings).
        let hooks_report = crate::hooks::pipeline::run(config, project_root)?;
        report.warnings.extend(hooks_report.warnings);

        // Instruction-file propagation.
        let (instr_installs, instr_state_links, instr_warnings) =
            execute_instruction_propagations(config, project_root, &old_state)?;
        report.installs.extend(instr_installs);
        new_state.links.extend(instr_state_links);
        report.warnings.extend(instr_warnings);

        // Stale cleanup.
        let kept: HashSet<&Path> = new_state
            .links
            .iter()
            .map(|link| link.target.as_path())
            .collect();
        for stale in &old_state.links {
            if kept.contains(stale.target.as_path()) {
                continue;
            }
            match remove_managed_link(stale) {
                Ok(true) => report.stale_removed.push(stale.clone()),
                Ok(false) => report.warnings.push(format!(
                    "left {} alone — it was modified outside agentenv",
                    stale.target.display()
                )),
                Err(err) => report.warnings.push(format!(
                    "failed to remove stale link {}: {err}",
                    stale.target.display()
                )),
            }
        }

        new_state.save(project_root)?;

        if config.gitignore_managed_links {
            // Hook writers materialize files (e.g. `.cursor/hooks.json`)
            // that don't appear in `state.links`. Collect their project-
            // rooted artifact paths so the gitignore writer can collapse
            // them into the same tool-folder entries as state links.
            let mut extra_artifacts: Vec<PathBuf> = Vec::new();
            if config.source.is_some() {
                for target in config.hook_write_targets() {
                    if let Some(path) =
                        crate::hooks::writers::project_artifact(&target, project_root)
                    {
                        extra_artifacts.push(path);
                    }
                }
            }
            if let Err(err) = crate::gitignore::refresh_managed_block(
                project_root,
                &new_state.links,
                &extra_artifacts,
            ) {
                report
                    .warnings
                    .push(format!("failed to refresh .gitignore managed block: {err}"));
            }
        }

        Ok(report)
    }

    /// Predict what `sync` would do. v1 only enumerates instruction-file
    /// propagation; capability pipelines are evaluated at sync time.
    pub fn plan<P: AsRef<Path>>(config: &Config, project_root: P) -> Result<SyncPlan> {
        let project_root = project_root.as_ref();
        let old_state = State::load(project_root)?;
        let (actions, mut warnings) =
            plan_instruction_propagations(config, project_root, &old_state)?;
        warnings.insert(
            0,
            "explain only covers instruction-file propagation in v1; capability writers \
             evaluate at sync time. Run `agentenv sync` to see the full effect."
                .to_string(),
        );
        Ok(SyncPlan { actions, warnings })
    }
}

fn success_install(link: &StateLink) -> InstallResult {
    InstallResult {
        action: InstallAction {
            source: link.source.clone(),
            target: link.target.clone(),
            mode: link.mode.clone(),
            tool: link.tool.clone(),
        },
        success: true,
        message: format!(
            "installed {} to {} ({})",
            link.source.display(),
            link.target.display(),
            link.mode
        ),
    }
}

/// Pseudo-plugin and tool labels used for instruction-file propagation.
pub(crate) const INSTRUCTIONS_PLUGIN: &str = "_instructions";
pub(crate) const INSTRUCTIONS_TOOL: &str = "instructions";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstructionDecision {
    Create,
    Update,
    Idempotent,
}

fn classify_instruction_destination(
    destination: &Path,
    expected_source: &Path,
    managed_targets: &HashSet<&Path>,
) -> Result<std::result::Result<InstructionDecision, String>> {
    let meta = match fs::symlink_metadata(destination) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Ok(InstructionDecision::Create));
        },
        Err(err) => return Err(Error::Io(err)),
    };

    let is_managed = managed_targets.contains(destination);
    if !meta.file_type().is_symlink() {
        return Ok(Err(format!(
            "not linking instruction file to {} — a real file is already there",
            destination.display()
        )));
    }

    let actual = fs::read_link(destination).map_err(Error::Io)?;
    if !is_managed {
        return Ok(Err(format!(
            "not linking instruction file to {} — an existing symlink is there (not agentenv-managed)",
            destination.display()
        )));
    }

    if actual == expected_source {
        Ok(Ok(InstructionDecision::Idempotent))
    } else {
        Ok(Ok(InstructionDecision::Update))
    }
}

fn plan_instruction_propagations(
    config: &Config,
    project_root: &Path,
    old_state: &State,
) -> Result<(Vec<PlannedAction>, Vec<String>)> {
    let mut actions = Vec::new();
    let mut warnings = Vec::new();
    let managed: HashSet<&Path> = old_state
        .links
        .iter()
        .map(|link| link.target.as_path())
        .collect();

    let mut sources: Vec<&String> = config.instruction_files.keys().collect();
    sources.sort();

    for source_name in sources {
        let source_path = project_root.join(source_name);
        if !source_path.exists() {
            let dest_count = config.instruction_files[source_name].len();
            warnings.push(format!(
                "instruction file `{source_name}` not found at project root; skipping {dest_count} destination(s)"
            ));
            continue;
        }

        for dest in &config.instruction_files[source_name] {
            let dest_path = project_root.join(dest);
            match classify_instruction_destination(&dest_path, &source_path, &managed)? {
                Ok(_decision) => {
                    actions.push(PlannedAction {
                        source: source_path.clone(),
                        target: dest_path,
                        mode: "symlink".to_string(),
                        tool: INSTRUCTIONS_TOOL.to_string(),
                        plugin: INSTRUCTIONS_PLUGIN.to_string(),
                    });
                },
                Err(warning) => warnings.push(warning),
            }
        }
    }

    Ok((actions, warnings))
}

fn execute_instruction_propagations(
    config: &Config,
    project_root: &Path,
    old_state: &State,
) -> Result<(Vec<InstallResult>, Vec<StateLink>, Vec<String>)> {
    let mut installs = Vec::new();
    let mut state_links = Vec::new();
    let mut warnings = Vec::new();
    let managed: HashSet<&Path> = old_state
        .links
        .iter()
        .map(|link| link.target.as_path())
        .collect();

    let mut sources: Vec<&String> = config.instruction_files.keys().collect();
    sources.sort();

    for source_name in sources {
        let source_path = project_root.join(source_name);
        if !source_path.exists() {
            let dest_count = config.instruction_files[source_name].len();
            warnings.push(format!(
                "instruction file `{source_name}` not found at project root; skipping {dest_count} destination(s)"
            ));
            continue;
        }

        for dest in &config.instruction_files[source_name] {
            let dest_path = project_root.join(dest);
            let decision =
                match classify_instruction_destination(&dest_path, &source_path, &managed)? {
                    Ok(decision) => decision,
                    Err(warning) => {
                        warnings.push(warning);
                        continue;
                    },
                };

            let action = InstallAction {
                source: source_path.clone(),
                target: dest_path.clone(),
                mode: "symlink".to_string(),
                tool: INSTRUCTIONS_TOOL.to_string(),
            };

            match decision {
                InstructionDecision::Create | InstructionDecision::Update => {
                    if matches!(decision, InstructionDecision::Update) {
                        SymlinkManager::remove(&dest_path)?;
                    }
                    if let Some(parent) = dest_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let install = SymlinkManager::install(&action)?;
                    if install.success {
                        state_links.push(StateLink {
                            source: source_path.clone(),
                            target: dest_path.clone(),
                            tool: INSTRUCTIONS_TOOL.to_string(),
                            mode: "symlink".to_string(),
                            plugin: INSTRUCTIONS_PLUGIN.to_string(),
                        });
                    }
                    installs.push(install);
                },
                InstructionDecision::Idempotent => {
                    state_links.push(StateLink {
                        source: source_path.clone(),
                        target: dest_path.clone(),
                        tool: INSTRUCTIONS_TOOL.to_string(),
                        mode: "symlink".to_string(),
                        plugin: INSTRUCTIONS_PLUGIN.to_string(),
                    });
                },
            }
        }
    }

    Ok((installs, state_links, warnings))
}

/// Remove a link that agentenv previously installed, defensively.
/// `Ok(true)` if gone afterwards; `Ok(false)` if modified outside agentenv
/// and left alone.
pub(crate) fn remove_managed_link(link: &StateLink) -> Result<bool> {
    let path = &link.target;
    let exists = path.exists() || path.is_symlink();
    if !exists {
        return Ok(true);
    }

    // Codex agent files are materialized TOML, not symlinks; they live
    // under `<project>/.codex/agents/*.toml` and the writer marks them
    // with a sentinel header. We trust the state-link record and remove
    // the file directly.
    if link.mode == "managed-file" {
        fs::remove_file(path).map_err(Error::Io)?;
        return Ok(true);
    }

    if path.is_symlink() {
        match fs::read_link(path) {
            Ok(actual) if actual == link.source => {},
            Ok(_) => return Ok(false),
            Err(err) => return Err(Error::Symlink(err.to_string())),
        }
        SymlinkManager::remove(path)?;
        return Ok(true);
    }

    Ok(false)
}

fn ensure_behavior(policy: FetchPolicy, config_refetch: bool) -> EnsureBehavior {
    match policy {
        FetchPolicy::Force => EnsureBehavior::Refetch,
        FetchPolicy::Skip => EnsureBehavior::Offline,
        FetchPolicy::FromConfig => {
            if config_refetch {
                EnsureBehavior::Refetch
            } else {
                EnsureBehavior::Cache
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CleanConfig, MarketplaceConfig, PluginRef, SyncConfig, TargetConfig};
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    fn empty_marketplace(marketplace_path: &Path) {
        fs::create_dir_all(marketplace_path.join(".claude-plugin")).unwrap();
        fs::write(
            marketplace_path.join(".claude-plugin/marketplace.json"),
            r#"{"name":"empty","owner":{"name":"t"},"plugins":[]}"#,
        )
        .unwrap();
    }

    fn write_plugin(marketplace: &Path, name: &str, version: &str, capabilities: &[&str]) {
        let plugin_dir = marketplace.join(name);
        for capability in capabilities {
            let cap_dir = plugin_dir.join(capability);
            fs::create_dir_all(&cap_dir).unwrap();
            if *capability == "skills" {
                let skill_dir = cap_dir.join(format!("{name}-skill"));
                fs::create_dir_all(&skill_dir).unwrap();
                fs::write(
                    skill_dir.join("SKILL.md"),
                    format!("---\nname: {name}-skill\ndescription: t\n---\n"),
                )
                .unwrap();
            } else if *capability == "agents" {
                fs::write(
                    cap_dir.join(format!("{name}-agent.md")),
                    format!("---\nname: {name}-agent\ndescription: t\n---\nprompt\n"),
                )
                .unwrap();
            }
        }

        let claude_dir = marketplace.join(".claude-plugin");
        fs::create_dir_all(&claude_dir).unwrap();
        let index_path = claude_dir.join("marketplace.json");
        let mut entries: Vec<serde_json::Value> = if index_path.exists() {
            let raw = fs::read_to_string(&index_path).unwrap();
            let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
            value
                .get("plugins")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        entries.push(serde_json::json!({
            "name": name,
            "source": format!("./{name}"),
            "version": version,
            "description": name,
        }));
        let index = serde_json::json!({
            "name": "test-marketplace",
            "owner": {"name": "test"},
            "plugins": entries,
        });
        fs::write(&index_path, serde_json::to_string_pretty(&index).unwrap()).unwrap();
    }

    fn base_config(marketplace_path: PathBuf) -> Config {
        let mut marketplaces = HashMap::new();
        marketplaces.insert(
            "default".to_string(),
            MarketplaceConfig {
                path: marketplace_path,
                remote: "https://example.com/marketplace.git".to_string(),
                r#ref: "main".to_string(),
            },
        );
        let mut targets = HashMap::new();
        targets.insert("cursor".to_string(), TargetConfig::default());
        Config {
            version: 1,
            marketplaces,
            plugins: vec![],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            gitignore_managed_links: false,
            instruction_files: HashMap::new(),
            source: Some("claude-code".to_string()),
        }
    }

    #[test]
    fn sync_propagates_marketplace_skill_to_cursor() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(marketplace.path(), "demo", "1.0.0", &["skills"]);

        let mut config = base_config(marketplace.path().to_path_buf());
        config.plugins = vec![PluginRef {
            name: "demo".to_string(),
            namespace: None,
            version: None,
        }];

        let report = Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();
        assert!(report.all_succeeded(), "report: {report:?}");
        let dest = project.path().join(".cursor/skills/demo-skill");
        assert!(dest.is_symlink(), "missing {}", dest.display());
        assert_eq!(
            fs::read_link(&dest).unwrap(),
            marketplace.path().join("demo/skills/demo-skill")
        );
    }

    #[test]
    fn sync_propagates_marketplace_agent_to_cursor() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(marketplace.path(), "demo", "1.0.0", &["agents"]);

        let mut config = base_config(marketplace.path().to_path_buf());
        config.plugins = vec![PluginRef {
            name: "demo".to_string(),
            namespace: None,
            version: None,
        }];

        let report = Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();
        assert!(report.all_succeeded(), "report: {report:?}");
        let dest = project.path().join(".cursor/agents/demo-agent.md");
        assert!(dest.is_symlink(), "missing {}", dest.display());
    }

    #[test]
    fn sync_is_idempotent() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(marketplace.path(), "demo", "1.0.0", &["skills", "agents"]);
        let mut config = base_config(marketplace.path().to_path_buf());
        config.plugins = vec![PluginRef {
            name: "demo".to_string(),
            namespace: None,
            version: None,
        }];

        let first = Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();
        let second = Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();
        assert!(first.all_succeeded());
        assert!(second.all_succeeded());
        // Per-leaf state links are recreated each run; counts match.
        assert_eq!(first.installs.len(), second.installs.len());
    }

    #[test]
    fn sync_picks_up_local_claude_skills_when_source_is_claude_code() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        empty_marketplace(marketplace.path());

        let skill_dir = project.path().join(".claude/skills/local");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: local\ndescription: t\n---\n",
        )
        .unwrap();

        let config = base_config(marketplace.path().to_path_buf());

        let report = Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert!(report.all_succeeded(), "report: {report:?}");
        assert!(project.path().join(".cursor/skills/local").is_symlink());
    }

    #[test]
    fn sync_removes_stale_links_when_skill_disappears() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        empty_marketplace(marketplace.path());

        let skill_dir = project.path().join(".claude/skills/local");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: local\ndescription: t\n---\n",
        )
        .unwrap();

        let config = base_config(marketplace.path().to_path_buf());

        Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert!(project.path().join(".cursor/skills/local").is_symlink());

        // Delete the source skill and re-sync — cursor link should be removed.
        fs::remove_dir_all(&skill_dir).unwrap();
        let report = Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert!(!project.path().join(".cursor/skills/local").exists());
        assert_eq!(report.stale_removed.len(), 1);
    }

    #[test]
    fn instruction_files_link_to_missing_destinations() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        empty_marketplace(marketplace.path());
        fs::write(project.path().join("CLAUDE.md"), "guidance").unwrap();

        let mut config = base_config(marketplace.path().to_path_buf());
        // No targets configured for instruction-files-only test.
        config.targets.clear();
        config.source = None;
        config.instruction_files.insert(
            "CLAUDE.md".to_string(),
            vec!["AGENTS.md".to_string(), ".junie/AGENTS.md".to_string()],
        );

        let report = Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert!(report.all_succeeded());
        assert!(project.path().join("AGENTS.md").is_symlink());
        assert!(project.path().join(".junie/AGENTS.md").is_symlink());
    }

    #[test]
    fn instruction_files_never_override_user_file() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        empty_marketplace(marketplace.path());
        fs::write(project.path().join("CLAUDE.md"), "guidance").unwrap();
        fs::write(project.path().join("AGENTS.md"), "user-content").unwrap();

        let mut config = base_config(marketplace.path().to_path_buf());
        config.targets.clear();
        config.source = None;
        config
            .instruction_files
            .insert("CLAUDE.md".to_string(), vec!["AGENTS.md".to_string()]);

        let report = Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("real file is already there")),
            "warnings: {:?}",
            report.warnings
        );
        assert_eq!(
            fs::read_to_string(project.path().join("AGENTS.md")).unwrap(),
            "user-content"
        );
    }

    #[test]
    fn sync_writes_gitignore_block_when_flag_enabled() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        empty_marketplace(marketplace.path());

        let skill_dir = project.path().join(".claude/skills/local");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: local\ndescription: t\n---\n",
        )
        .unwrap();

        let mut config = base_config(marketplace.path().to_path_buf());
        config.gitignore_managed_links = true;

        Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        let gitignore = fs::read_to_string(project.path().join(".gitignore")).unwrap();
        assert!(gitignore.contains(crate::gitignore::BEGIN_MARKER));
        // Collapsed to the top-level tool folder rather than each leaf.
        assert!(gitignore.contains("/.cursor/"));
        assert!(
            !gitignore.contains("/.cursor/skills/local"),
            "leaf entries should be collapsed to /.cursor/"
        );
    }
}
