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

        let old_state = State::load(project_root)?;
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
        let (actions, warnings) = enumerate_actions(config, project_root, &resolved)?;
        Ok(SyncPlan { actions, warnings })
    }
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
            claude_hooks: None,
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
}
