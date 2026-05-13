//! Sync engine: resolve plugins and install them into target tools.
//!
//! This is the local-only sync path. Marketplace fetch is the caller's
//! responsibility — by the time `Syncer::sync` runs, every marketplace
//! `path` referenced by `Config` must already be populated on disk.

use crate::claude_config::synthesize_local_claude_plugin;
use crate::config::{Config, TargetConfig};
use crate::error::{Error, Result};
use crate::marketplace::{EnsureBehavior, EnsureOutcome, Marketplace};
use crate::resolver::{PluginResolver, ResolvedPlugin};
use crate::state::{State, StateLink};
use crate::symlink::{InstallAction, InstallResult, SymlinkManager};
use std::collections::HashSet;
use std::ffi::OsString;
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
    /// How to treat marketplace remotes during this sync.
    pub fetch: FetchPolicy,
}

/// One entry in a sync plan: where a managed link will be created and what it
/// will point at.
#[derive(Debug, Clone)]
pub struct PlannedAction {
    /// Path the symlink will point at (typically inside a marketplace).
    pub source: PathBuf,
    /// Path where the link will be created (typically inside the project).
    pub target: PathBuf,
    /// Install mode (`symlink` or `copy`).
    pub mode: String,
    /// Target tool name.
    pub tool: String,
    /// Owning plugin name.
    pub plugin: String,
}

/// What a sync would do, without doing it. Drives `agentenv explain`.
#[derive(Debug, Default)]
pub struct SyncPlan {
    /// Actions that would be executed in order.
    pub actions: Vec<PlannedAction>,
    /// Non-fatal warnings discovered while planning.
    pub warnings: Vec<String>,
}

/// Outcome of a sync run.
#[derive(Debug, Default)]
pub struct SyncReport {
    /// Per-link install results.
    pub installs: Vec<InstallResult>,
    /// Stale managed links removed because they're no longer in the plan.
    pub stale_removed: Vec<StateLink>,
    /// Non-fatal warnings (e.g. plugin had no matching target).
    pub warnings: Vec<String>,
}

impl SyncReport {
    /// Number of installs that succeeded.
    pub fn success_count(&self) -> usize {
        self.installs.iter().filter(|r| r.success).count()
    }

    /// Number of installs that failed.
    pub fn failure_count(&self) -> usize {
        self.installs.iter().filter(|r| !r.success).count()
    }

    /// True if every install succeeded.
    pub fn all_succeeded(&self) -> bool {
        self.failure_count() == 0
    }
}

impl Syncer {
    /// Resolve plugins from the local marketplace and install all link plans.
    ///
    /// Sync walks **per-leaf**: for each capability folder a plugin declares,
    /// every immediate child (a skill directory, an agent file, a command
    /// file, …) is linked into `<rendered_target>/<leaf-name>`. This matches
    /// the `agentskills.io` convention and tools like Claude Code that expect
    /// `<scope>/skills/<skill-name>/SKILL.md` at depth 1.
    ///
    /// Mappings can use `{plugin}` in `target` to introduce per-plugin
    /// namespacing on top of the leaf name. The bundled defaults don't —
    /// leaf names are the unit of identity.
    ///
    /// # Errors
    ///
    /// Returns an error from [`Marketplace::ensure`] (e.g. clone failure,
    /// offline-mode marketplace missing), from [`PluginResolver::resolve_all`]
    /// (unknown plugin, missing manifest, unsupported capability) or from
    /// [`SymlinkManager::install`] for unrecoverable issues like an unknown
    /// install mode. Per-link IO failures are recorded in the report rather
    /// than aborting the whole sync.
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

        let mut resolved = PluginResolver::resolve_all(config, project_root)?;
        if config.use_claude_config {
            if let Some(plugin) = synthesize_local_claude_plugin(project_root) {
                resolved.push(plugin);
            }
        }
        let (actions, warnings) = enumerate_actions(config, project_root, &resolved)?;
        report.warnings.extend(warnings);

        // Load state up front: instruction-file propagation needs it to
        // distinguish agentenv-managed symlinks from user files.
        let old_state = State::load(project_root)?;

        let mut new_state = State::default();
        for planned in &actions {
            let action = InstallAction {
                source: planned.source.clone(),
                target: planned.target.clone(),
                mode: planned.mode.clone(),
                tool: planned.tool.clone(),
            };

            let result = SymlinkManager::install(&action)?;
            if result.success {
                new_state.links.push(StateLink {
                    source: planned.source.clone(),
                    target: planned.target.clone(),
                    tool: planned.tool.clone(),
                    mode: planned.mode.clone(),
                    plugin: planned.plugin.clone(),
                });
            }
            report.installs.push(result);
        }

        // Instruction-file propagation. Pure file→file linking with a
        // NEVER-OVERRIDE rule: if a destination already holds a non-managed
        // file, it is left untouched and a warning is emitted.
        let (instr_installs, instr_state_links, instr_warnings) =
            execute_instruction_propagations(config, project_root, &old_state)?;
        report.installs.extend(instr_installs);
        new_state.links.extend(instr_state_links);
        report.warnings.extend(instr_warnings);

        // Hooks pipeline: source → canonical → cursor/codex. Runs only if at
        // least one configured target is a v1 hook write target. Refuse-on-
        // conflict errors here propagate as hard errors from sync.
        let hooks_report = crate::hooks::pipeline::run(config, project_root)?;
        report.warnings.extend(hooks_report.warnings);

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
            if let Err(err) =
                crate::gitignore::refresh_managed_block(project_root, &new_state.links)
            {
                report
                    .warnings
                    .push(format!("failed to refresh .gitignore managed block: {err}"));
            }
        }

        Ok(report)
    }

    /// Build a sync plan without touching the filesystem outside the
    /// marketplace directories. Marketplaces are not fetched — the caller is
    /// expected to have run `sync` (or set them up manually) first.
    pub fn plan<P: AsRef<Path>>(config: &Config, project_root: P) -> Result<SyncPlan> {
        let project_root = project_root.as_ref();
        let mut resolved = PluginResolver::resolve_all(config, project_root)?;
        if config.use_claude_config {
            if let Some(plugin) = synthesize_local_claude_plugin(project_root) {
                resolved.push(plugin);
            }
        }
        let (mut actions, mut warnings) = enumerate_actions(config, project_root, &resolved)?;

        let old_state = State::load(project_root)?;
        let (instr_actions, instr_warnings) =
            plan_instruction_propagations(config, project_root, &old_state)?;
        actions.extend(instr_actions);
        warnings.extend(instr_warnings);

        Ok(SyncPlan { actions, warnings })
    }
}

/// Pseudo-plugin and tool labels used for instruction-file propagation.
/// Surfaces in `agentenv explain`, `list`, and state.json so the user can
/// see which links agentenv owns.
pub(crate) const INSTRUCTIONS_PLUGIN: &str = "_instructions";
pub(crate) const INSTRUCTIONS_TOOL: &str = "instructions";

/// What the engine should do with one instruction-file destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstructionDecision {
    /// Destination doesn't exist — create the symlink.
    Create,
    /// Destination is an agentenv-managed symlink pointing at a different
    /// source — remove and re-create.
    Update,
    /// Destination is an agentenv-managed symlink already pointing at the
    /// right source — confirm in state, no filesystem change.
    Idempotent,
}

/// Inspect a planned instruction-file destination and decide what should
/// happen. Returns `Ok(Some(decision))` for cases we will act on, or
/// `Ok(None)` with a warning string when the destination is occupied by a
/// non-managed file (the NEVER-OVERRIDE rule).
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

/// Build the list of `(source, destination, decision)` tuples that
/// instruction-file propagation would act on, plus any warnings produced.
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

/// Execute instruction-file propagation. Mirrors
/// [`plan_instruction_propagations`] but performs the filesystem work and
/// returns install results + state-link entries for the caller to merge.
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
                    // No filesystem change — but record in new state so the
                    // stale-cleanup pass doesn't remove the link.
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

fn enumerate_actions(
    config: &Config,
    project_root: &Path,
    resolved: &[ResolvedPlugin],
) -> Result<(Vec<PlannedAction>, Vec<String>)> {
    let mut actions = Vec::new();
    let mut warnings = Vec::new();

    for plugin in resolved {
        let plugin_dir = PathBuf::from(&plugin.location);
        let mut linked_any = false;

        for capability in &plugin.capabilities {
            let capability_dir = plugin_dir.join(capability);
            let leaves = list_capability_leaves(&capability_dir)?;
            if leaves.is_empty() {
                continue;
            }

            for (target_name, target) in &config.targets {
                if !plugin_supports_target(plugin, target_name, target) {
                    continue;
                }
                let Some(mappings) = target.source_mappings.get(capability) else {
                    continue;
                };

                for mapping in mappings {
                    let dest_root =
                        render_destination(project_root, &mapping.target, &plugin.name)?;
                    for leaf in &leaves {
                        actions.push(PlannedAction {
                            source: capability_dir.join(leaf),
                            target: dest_root.join(leaf),
                            mode: mapping.mode.clone(),
                            tool: target_name.clone(),
                            plugin: plugin.name.clone(),
                        });
                        linked_any = true;
                    }
                }
            }
        }

        if !linked_any && !plugin.capabilities.is_empty() {
            warnings.push(format!(
                "plugin {} declared capabilities but no target accepted them",
                plugin.name
            ));
        }
    }

    Ok((actions, warnings))
}

/// Remove a link that agentenv previously installed, defensively. Returns
/// `Ok(true)` if the link is gone afterwards; `Ok(false)` if it was modified
/// outside agentenv and was left alone.
pub(crate) fn remove_managed_link(link: &StateLink) -> Result<bool> {
    let path = &link.target;
    let exists = path.exists() || path.is_symlink();
    if !exists {
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

    // Not a symlink — likely a copy install or user-replaced file. Leave it.
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

fn plugin_supports_target(
    plugin: &ResolvedPlugin,
    target_name: &str,
    target: &TargetConfig,
) -> bool {
    if plugin.targets.is_empty() {
        return true;
    }

    plugin
        .targets
        .iter()
        .any(|declared| declared == target_name || target.tools.iter().any(|tool| tool == declared))
}

/// List the immediate children of a capability directory.
///
/// Hidden entries (names starting with `.`) are skipped so plugin authors can
/// drop READMEs or `.gitkeep` files inside a capability folder without
/// polluting the destination. Sorted for deterministic install order.
fn list_capability_leaves(dir: &Path) -> Result<Vec<OsString>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        entries.push(name);
    }
    entries.sort();
    Ok(entries)
}

/// Substitute `{plugin}` in `target`, expand `~/`, and resolve relative paths
/// against `project_root`.
fn render_destination(project_root: &Path, target: &Path, plugin: &str) -> Result<PathBuf> {
    let rendered = target.to_string_lossy().replace("{plugin}", plugin);

    if let Some(rest) = rendered.strip_prefix("~/") {
        let home = dirs::home_dir()
            .ok_or_else(|| Error::Config("cannot determine home directory".to_string()))?;
        return Ok(home.join(rest));
    }

    let path = Path::new(&rendered);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    Ok(project_root.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CleanConfig, MarketplaceConfig, PluginRef, SourceMapping, SyncConfig};
    use crate::targets::TargetDefaults;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    /// Per-capability fixture content. `skills` get directory-form leaves
    /// containing a SKILL.md (matches the agentskills.io spec); other
    /// capabilities get flat-file leaves.
    fn populate_capability(plugin_dir: &Path, capability: &str) {
        let cap_dir = plugin_dir.join(capability);
        fs::create_dir_all(&cap_dir).unwrap();

        if capability == "skills" {
            let skill_dir = cap_dir.join(format!("{capability}-leaf"));
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                "---\nname: skills-leaf\ndescription: test\n---\n",
            )
            .unwrap();
        } else {
            fs::write(
                cap_dir.join(format!("{capability}-leaf.md")),
                "---\nname: leaf\n---\nbody\n",
            )
            .unwrap();
        }
    }

    /// Append a single plugin to a Claude Code-style marketplace at
    /// `<marketplace>/.claude-plugin/marketplace.json` and create the plugin's
    /// capability folders at `<marketplace>/<name>/`.
    fn write_plugin(
        marketplace: &Path,
        name: &str,
        version: &str,
        _targets: &[&str],
        capabilities: &[&str],
    ) {
        let plugin_dir = marketplace.join(name);
        for capability in capabilities {
            populate_capability(&plugin_dir, capability);
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
        targets.insert("claude-code".to_string(), TargetDefaults::claude_code());
        Config {
            version: 1,
            marketplaces,
            plugins: vec![],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            gitignore_managed_links: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
            source: None,
        }
    }

    #[test]
    fn sync_links_each_capability_leaf_at_depth_one() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(
            marketplace.path(),
            "demo",
            "1.0.0",
            &["claude-code"],
            &["skills", "commands"],
        );

        let mut config = base_config(marketplace.path().to_path_buf());
        config.plugins = vec![PluginRef {
            name: "demo".to_string(),
            namespace: None,
            version: None,
        }];

        let report = Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();

        assert_eq!(report.installs.len(), 2);
        assert!(report.all_succeeded());
        assert!(report.warnings.is_empty());

        let skill_link = project.path().join(".claude/skills/skills-leaf");
        let command_link = project.path().join(".claude/commands/commands-leaf.md");
        assert!(skill_link.is_symlink());
        assert!(command_link.is_symlink());
        assert_eq!(
            fs::read_link(&skill_link).unwrap(),
            marketplace.path().join("demo/skills/skills-leaf")
        );
    }

    #[test]
    fn sync_is_idempotent() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(
            marketplace.path(),
            "demo",
            "1.0.0",
            &["claude-code"],
            &["skills"],
        );
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
        assert_eq!(first.installs.len(), second.installs.len());
        assert!(project
            .path()
            .join(".claude/skills/skills-leaf")
            .is_symlink());
    }

    #[test]
    fn sync_skips_target_without_capability_mapping() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(
            marketplace.path(),
            "skills-plugin",
            "1.0.0",
            &[],
            &["skills"],
        );

        // Cursor's defaults map `skills`; if we wired a target without that
        // mapping we'd skip it silently. Here we just ensure the multi-target
        // case still installs into the target that does have the mapping.
        let mut config = base_config(marketplace.path().to_path_buf());
        config
            .targets
            .insert("cursor".to_string(), TargetDefaults::cursor());
        config.plugins = vec![PluginRef {
            name: "skills-plugin".to_string(),
            namespace: None,
            version: None,
        }];

        let report = Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();

        assert!(report.all_succeeded());
        assert!(report
            .installs
            .iter()
            .any(|r| r.action.tool == "claude-code"));
    }

    #[test]
    fn sync_warns_when_no_target_accepts_plugin() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        // claude-code's defaults have no `hooks` mapping; the plugin's only
        // capability is `hooks` → nothing to install, expect a warning.
        write_plugin(marketplace.path(), "hooks-only", "1.0.0", &[], &["hooks"]);

        let mut config = base_config(marketplace.path().to_path_buf());
        config.plugins = vec![PluginRef {
            name: "hooks-only".to_string(),
            namespace: None,
            version: None,
        }];

        let report = Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();

        assert_eq!(report.installs.len(), 0);
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("hooks-only"));
    }

    #[test]
    fn sync_honours_plugin_placeholder_when_user_opts_in() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        let absolute_dest = TempDir::new().unwrap();
        write_plugin(
            marketplace.path(),
            "demo",
            "1.0.0",
            &["claude-code"],
            &["skills"],
        );

        let mut config = base_config(marketplace.path().to_path_buf());
        let target = config.targets.get_mut("claude-code").unwrap();
        target.source_mappings.insert(
            "skills".to_string(),
            vec![SourceMapping {
                target: absolute_dest.path().join("{plugin}"),
                mode: "symlink".to_string(),
            }],
        );
        config.plugins = vec![PluginRef {
            name: "demo".to_string(),
            namespace: None,
            version: None,
        }];

        let report = Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();
        assert!(report.all_succeeded());
        assert!(absolute_dest.path().join("demo/skills-leaf").is_symlink());
    }

    #[test]
    fn sync_skips_hidden_entries_inside_capability_folder() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(
            marketplace.path(),
            "demo",
            "1.0.0",
            &["claude-code"],
            &["skills"],
        );
        // Drop a hidden file alongside the leaf — it should not be linked.
        fs::write(marketplace.path().join("demo/skills/.gitkeep"), "").unwrap();

        let mut config = base_config(marketplace.path().to_path_buf());
        config.plugins = vec![PluginRef {
            name: "demo".to_string(),
            namespace: None,
            version: None,
        }];

        let report = Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();
        assert_eq!(report.installs.len(), 1);
        assert!(report.all_succeeded());
        assert!(!project.path().join(".claude/skills/.gitkeep").exists());
    }

    /// When `use_claude_config: true`, inline-authored agents/skills/commands
    /// under `<project>/.claude/` are propagated to non-claude-code targets
    /// (e.g. cursor), even when no marketplace plugin is configured.
    #[test]
    fn sync_propagates_local_claude_assets_when_flag_enabled() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();

        // No marketplace plugins. The marketplace dir just needs the index so
        // `Marketplace::ensure` is a no-op in offline mode.
        fs::create_dir_all(marketplace.path().join(".claude-plugin")).unwrap();
        fs::write(
            marketplace.path().join(".claude-plugin/marketplace.json"),
            r#"{"name":"empty","owner":{"name":"t"},"plugins":[]}"#,
        )
        .unwrap();

        // Inline agent and skill authored directly in the project's .claude/.
        fs::create_dir_all(project.path().join(".claude/agents")).unwrap();
        fs::write(
            project.path().join(".claude/agents/local-reviewer.md"),
            "---\nname: local-reviewer\n---\nbody\n",
        )
        .unwrap();
        fs::create_dir_all(project.path().join(".claude/skills/local-skill")).unwrap();
        fs::write(
            project.path().join(".claude/skills/local-skill/SKILL.md"),
            "---\nname: local-skill\ndescription: t\n---\n",
        )
        .unwrap();

        let mut config = base_config(marketplace.path().to_path_buf());
        config.use_claude_config = true;
        // Replace the default `claude-code` target (which use_claude_config
        // would drop at load time) with `cursor`, simulating the user's
        // post-load state.
        config.targets.clear();
        config
            .targets
            .insert("cursor".to_string(), TargetDefaults::cursor());

        let report = Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert!(report.all_succeeded(), "sync failed: {report:?}");
        assert_eq!(
            report.installs.len(),
            2,
            "expected agent + skill to link into cursor"
        );

        let agent_link = project.path().join(".cursor/agents/local-reviewer.md");
        let skill_link = project.path().join(".cursor/skills/local-skill");
        assert!(agent_link.is_symlink(), "missing {}", agent_link.display());
        assert!(skill_link.is_symlink(), "missing {}", skill_link.display());
        assert_eq!(
            fs::read_link(&agent_link).unwrap(),
            project.path().join(".claude/agents/local-reviewer.md")
        );
    }

    #[test]
    fn sync_ignores_local_claude_when_flag_disabled() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        fs::create_dir_all(marketplace.path().join(".claude-plugin")).unwrap();
        fs::write(
            marketplace.path().join(".claude-plugin/marketplace.json"),
            r#"{"name":"empty","owner":{"name":"t"},"plugins":[]}"#,
        )
        .unwrap();
        fs::create_dir_all(project.path().join(".claude/agents")).unwrap();
        fs::write(project.path().join(".claude/agents/ignored.md"), "body").unwrap();

        let mut config = base_config(marketplace.path().to_path_buf());
        // use_claude_config is false; targets keep their default `claude-code`.
        config.targets.clear();
        config
            .targets
            .insert("cursor".to_string(), TargetDefaults::cursor());

        let report = Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert_eq!(report.installs.len(), 0);
        assert!(
            !project.path().join(".cursor/agents/ignored.md").exists(),
            "should not propagate inline .claude/ assets when use_claude_config is false"
        );
    }

    /// Empty-marketplace, no-plugins fixture used for instruction-file tests.
    fn instructions_config(marketplace_path: PathBuf) -> Config {
        fs::create_dir_all(marketplace_path.join(".claude-plugin")).unwrap();
        fs::write(
            marketplace_path.join(".claude-plugin/marketplace.json"),
            r#"{"name":"empty","owner":{"name":"t"},"plugins":[]}"#,
        )
        .unwrap();
        let mut config = base_config(marketplace_path);
        config.targets.clear();
        config
    }

    #[test]
    fn instruction_files_link_to_missing_destinations() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        fs::write(project.path().join("CLAUDE.md"), "guidance").unwrap();

        let mut config = instructions_config(marketplace.path().to_path_buf());
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

        assert!(report.all_succeeded(), "report: {report:?}");
        assert_eq!(report.installs.len(), 2);
        let agents = project.path().join("AGENTS.md");
        let junie = project.path().join(".junie/AGENTS.md");
        assert!(agents.is_symlink());
        assert!(junie.is_symlink());
        assert_eq!(
            fs::read_link(&agents).unwrap(),
            project.path().join("CLAUDE.md")
        );
    }

    #[test]
    fn instruction_files_never_override_user_file() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        fs::write(project.path().join("CLAUDE.md"), "guidance").unwrap();
        // User already has AGENTS.md with their own content. Must be preserved.
        fs::write(project.path().join("AGENTS.md"), "user-content").unwrap();

        let mut config = instructions_config(marketplace.path().to_path_buf());
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

        assert_eq!(report.installs.len(), 0, "must not install over user file");
        assert!(
            !project.path().join("AGENTS.md").is_symlink(),
            "user file must remain a regular file"
        );
        assert_eq!(
            fs::read_to_string(project.path().join("AGENTS.md")).unwrap(),
            "user-content"
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("real file is already there")),
            "expected NEVER-OVERRIDE warning, got: {:?}",
            report.warnings
        );
    }

    #[test]
    fn instruction_files_idempotent_on_resync() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        fs::write(project.path().join("CLAUDE.md"), "guidance").unwrap();

        let mut config = instructions_config(marketplace.path().to_path_buf());
        config
            .instruction_files
            .insert("CLAUDE.md".to_string(), vec!["AGENTS.md".to_string()]);

        Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        let report = Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();

        // Second sync: link already correct, so no fresh install was needed
        // (idempotent path) but the link must remain in state and on disk.
        assert_eq!(report.installs.len(), 0);
        assert!(project.path().join("AGENTS.md").is_symlink());
        assert_eq!(report.stale_removed.len(), 0);
    }

    #[test]
    fn instruction_files_warn_when_source_missing() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        // No CLAUDE.md created.

        let mut config = instructions_config(marketplace.path().to_path_buf());
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

        assert_eq!(report.installs.len(), 0);
        assert!(!project.path().join("AGENTS.md").exists());
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("not found at project root")),
            "expected source-missing warning, got: {:?}",
            report.warnings
        );
    }

    #[test]
    fn instruction_files_removed_on_clean_via_state() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        fs::write(project.path().join("CLAUDE.md"), "guidance").unwrap();

        let mut config = instructions_config(marketplace.path().to_path_buf());
        config
            .instruction_files
            .insert("CLAUDE.md".to_string(), vec!["AGENTS.md".to_string()]);

        Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert!(project.path().join("AGENTS.md").is_symlink());

        // Drop the entry from config and re-sync — the link should be
        // detected as stale and removed by the existing stale-cleanup pass.
        config.instruction_files.clear();
        let report = Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert!(
            !project.path().join("AGENTS.md").exists(),
            "instruction-file link should be cleaned up when removed from config"
        );
        assert_eq!(report.stale_removed.len(), 1);
    }

    #[test]
    fn instruction_files_update_when_source_changes() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        fs::write(project.path().join("CLAUDE.md"), "first").unwrap();
        fs::write(project.path().join("CLAUDE2.md"), "second").unwrap();

        let mut config = instructions_config(marketplace.path().to_path_buf());
        config
            .instruction_files
            .insert("CLAUDE.md".to_string(), vec!["AGENTS.md".to_string()]);

        Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert_eq!(
            fs::read_link(project.path().join("AGENTS.md")).unwrap(),
            project.path().join("CLAUDE.md")
        );

        // Repoint AGENTS.md at CLAUDE2.md — agentenv owns the link, so
        // updating is allowed.
        config.instruction_files.clear();
        config
            .instruction_files
            .insert("CLAUDE2.md".to_string(), vec!["AGENTS.md".to_string()]);
        let report = Syncer::sync(
            &config,
            project.path(),
            SyncOptions {
                fetch: FetchPolicy::Skip,
            },
        )
        .unwrap();
        assert!(report.all_succeeded(), "report: {report:?}");
        assert_eq!(
            fs::read_link(project.path().join("AGENTS.md")).unwrap(),
            project.path().join("CLAUDE2.md")
        );
    }

    #[test]
    fn sync_writes_gitignore_block_when_flag_enabled() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(
            marketplace.path(),
            "demo",
            "1.0.0",
            &["claude-code"],
            &["agents"],
        );

        let mut config = base_config(marketplace.path().to_path_buf());
        config.gitignore_managed_links = true;
        config.plugins = vec![PluginRef {
            name: "demo".to_string(),
            namespace: None,
            version: None,
        }];

        Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();

        let gitignore = fs::read_to_string(project.path().join(".gitignore")).unwrap();
        assert!(
            gitignore.contains(crate::gitignore::BEGIN_MARKER),
            "missing begin marker. got: {gitignore}"
        );
        assert!(gitignore.contains("/.claude/agents/agents-leaf.md"));
        assert!(gitignore.contains(crate::gitignore::END_MARKER));
    }

    #[test]
    fn sync_does_not_touch_gitignore_when_flag_disabled() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(
            marketplace.path(),
            "demo",
            "1.0.0",
            &["claude-code"],
            &["agents"],
        );
        fs::write(project.path().join(".gitignore"), "node_modules/\n").unwrap();

        let mut config = base_config(marketplace.path().to_path_buf());
        config.plugins = vec![PluginRef {
            name: "demo".to_string(),
            namespace: None,
            version: None,
        }];

        Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();

        let gitignore = fs::read_to_string(project.path().join(".gitignore")).unwrap();
        assert_eq!(
            gitignore, "node_modules/\n",
            "must not modify user gitignore"
        );
    }

    #[test]
    fn sync_preserves_user_gitignore_content_outside_block() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(
            marketplace.path(),
            "demo",
            "1.0.0",
            &["claude-code"],
            &["agents"],
        );
        // Seed with user content the user wrote by hand.
        let user_gitignore = "node_modules/\n*.log\n.env\n";
        fs::write(project.path().join(".gitignore"), user_gitignore).unwrap();

        let mut config = base_config(marketplace.path().to_path_buf());
        config.gitignore_managed_links = true;
        config.plugins = vec![PluginRef {
            name: "demo".to_string(),
            namespace: None,
            version: None,
        }];

        Syncer::sync(&config, project.path(), SyncOptions::default()).unwrap();

        let after = fs::read_to_string(project.path().join(".gitignore")).unwrap();
        // User lines intact, in the same order, with the managed block appended.
        assert!(after.starts_with("node_modules/\n*.log\n.env\n"));
        assert!(after.contains(crate::gitignore::BEGIN_MARKER));
    }
}
