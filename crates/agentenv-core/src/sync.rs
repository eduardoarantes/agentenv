//! Sync engine: resolve plugins and install them into target tools.
//!
//! This is the local-only sync path. Marketplace fetch is the caller's
//! responsibility — by the time `Syncer::sync` runs, every marketplace
//! `path` referenced by `Config` must already be populated on disk.

use crate::config::{Config, TargetConfig};
use crate::error::{Error, Result};
use crate::resolver::{PluginResolver, ResolvedPlugin};
use crate::symlink::{InstallAction, InstallResult, SymlinkManager};
use std::path::{Path, PathBuf};

/// Sync engine.
#[derive(Debug)]
pub struct Syncer;

/// Outcome of a sync run.
#[derive(Debug, Default)]
pub struct SyncReport {
    /// Per-link install results.
    pub installs: Vec<InstallResult>,
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
    /// For each resolved plugin, every declared capability is linked into every
    /// configured target that (a) supports the plugin and (b) defines a
    /// `source_mappings` entry for that capability. Each plugin's contribution
    /// is namespaced under its own subdirectory in the destination, so multiple
    /// plugins can coexist in a single target folder.
    ///
    /// # Errors
    ///
    /// Returns an error from [`PluginResolver::resolve_all`] (e.g. unknown
    /// plugin, missing manifest, unsupported capability) or from
    /// [`SymlinkManager::install`] for unrecoverable issues like an unknown
    /// install mode. Per-link IO failures are recorded in the report rather
    /// than aborting the whole sync.
    pub fn sync<P: AsRef<Path>>(config: &Config, project_root: P) -> Result<SyncReport> {
        let project_root = project_root.as_ref();
        let resolved = PluginResolver::resolve_all(config)?;
        let mut report = SyncReport::default();

        for plugin in &resolved {
            Self::sync_plugin(config, project_root, plugin, &mut report)?;
        }

        Ok(report)
    }

    fn sync_plugin(
        config: &Config,
        project_root: &Path,
        plugin: &ResolvedPlugin,
        report: &mut SyncReport,
    ) -> Result<()> {
        let plugin_dir = PathBuf::from(&plugin.location);
        let mut linked_any = false;

        for capability in &plugin.capabilities {
            let source = plugin_dir.join(capability);

            for (target_name, target) in &config.targets {
                if !plugin_supports_target(plugin, target_name, target) {
                    continue;
                }

                let Some(mappings) = target.source_mappings.get(capability) else {
                    continue;
                };

                for mapping in mappings {
                    let dest = expand_destination(project_root, &mapping.target)?
                        .join(&plugin.name);

                    let action = InstallAction {
                        source: source.clone(),
                        target: dest,
                        mode: mapping.mode.clone(),
                        tool: target_name.clone(),
                    };

                    let result = SymlinkManager::install(&action)?;
                    if result.success {
                        linked_any = true;
                    }
                    report.installs.push(result);
                }
            }
        }

        if !linked_any && !plugin.capabilities.is_empty() {
            report.warnings.push(format!(
                "plugin {} declared capabilities but no target accepted them",
                plugin.name
            ));
        }

        Ok(())
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

    plugin.targets.iter().any(|declared| {
        declared == target_name || target.tools.iter().any(|tool| tool == declared)
    })
}

fn expand_destination(project_root: &Path, target: &Path) -> Result<PathBuf> {
    let target_str = target.to_string_lossy();

    if let Some(rest) = target_str.strip_prefix("~/") {
        let home = dirs::home_dir()
            .ok_or_else(|| Error::Config("cannot determine home directory".to_string()))?;
        return Ok(home.join(rest));
    }

    if target.is_absolute() {
        return Ok(target.to_path_buf());
    }

    Ok(project_root.join(target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MarketplaceConfig, PluginRef, SourceMapping, SyncConfig};
    use crate::targets::TargetDefaults;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    fn write_plugin(
        marketplace: &Path,
        name: &str,
        version: &str,
        targets: &[&str],
        capabilities: &[&str],
    ) {
        let plugin_dir = marketplace.join("plugins").join(name);
        let manifest_dir = plugin_dir.join(".claude-plugin");
        fs::create_dir_all(&manifest_dir).unwrap();
        for capability in capabilities {
            fs::create_dir_all(plugin_dir.join(capability)).unwrap();
        }
        let targets_json = targets
            .iter()
            .map(|t| format!(r#""{t}""#))
            .collect::<Vec<_>>()
            .join(", ");
        let capabilities_json = capabilities
            .iter()
            .map(|c| format!(r#""{c}""#))
            .collect::<Vec<_>>()
            .join(", ");
        fs::write(
            manifest_dir.join("plugin.json"),
            format!(
                r#"{{"name":"{name}","version":"{version}","description":"{name}","targets":[{targets_json}],"capabilities":[{capabilities_json}],"metadata":{{}}}}"#
            ),
        )
        .unwrap();
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
        }
    }

    #[test]
    fn sync_creates_symlinks_for_each_capability() {
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

        let report = Syncer::sync(&config, project.path()).unwrap();

        assert_eq!(report.installs.len(), 2);
        assert!(report.all_succeeded());
        assert!(report.warnings.is_empty());

        let skills_link = project.path().join(".claude-code/skills/demo");
        let commands_link = project.path().join(".claude-code/commands/demo");
        assert!(skills_link.is_symlink());
        assert!(commands_link.is_symlink());
        assert_eq!(
            fs::read_link(&skills_link).unwrap(),
            marketplace.path().join("plugins/demo/skills")
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

        let first = Syncer::sync(&config, project.path()).unwrap();
        let second = Syncer::sync(&config, project.path()).unwrap();

        assert!(first.all_succeeded());
        assert!(second.all_succeeded());
        assert_eq!(first.installs.len(), second.installs.len());
        assert!(project.path().join(".claude-code/skills/demo").is_symlink());
    }

    #[test]
    fn sync_skips_target_unsupported_by_plugin() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(
            marketplace.path(),
            "claude-only",
            "1.0.0",
            &["claude-code"],
            &["skills"],
        );

        let mut config = base_config(marketplace.path().to_path_buf());
        config
            .targets
            .insert("cursor".to_string(), TargetDefaults::cursor());
        config.plugins = vec![PluginRef {
            name: "claude-only".to_string(),
            namespace: None,
            version: None,
        }];

        let report = Syncer::sync(&config, project.path()).unwrap();

        assert_eq!(report.installs.len(), 1);
        assert!(report.installs[0].success);
        assert_eq!(report.installs[0].action.tool, "claude-code");
    }

    #[test]
    fn sync_warns_when_no_target_accepts_plugin() {
        let marketplace = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_plugin(
            marketplace.path(),
            "hooks-only",
            "1.0.0",
            &["claude-code"],
            &["hooks"],
        );

        // Inject a target with a `hooks` source_mapping so plugin resolution
        // accepts the capability, but install will then have nowhere to land
        // because no target supports the plugin's declared targets... actually
        // claude-code is its target — so the warning case is when capability
        // exists but target doesn't define it. Use a capability the target
        // doesn't map.
        let mut config = base_config(marketplace.path().to_path_buf());
        // Add a hooks mapping to a different target so resolution succeeds...
        let mut other = TargetDefaults::cursor();
        other.source_mappings.insert(
            "hooks".to_string(),
            vec![SourceMapping {
                source: PathBuf::from("ignored"),
                target: PathBuf::from(".cursor/hooks"),
                mode: "symlink".to_string(),
            }],
        );
        config.targets.insert("cursor".to_string(), other);
        config.plugins = vec![PluginRef {
            name: "hooks-only".to_string(),
            namespace: None,
            version: None,
        }];

        // Plugin only supports claude-code, but claude-code lacks `hooks`
        // mapping; cursor has hooks but plugin doesn't target it.
        let report = Syncer::sync(&config, project.path()).unwrap();

        assert_eq!(report.installs.len(), 0);
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("hooks-only"));
    }

    #[test]
    fn sync_resolves_absolute_target_paths() {
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
        // Override skills mapping with an absolute path.
        let target = config.targets.get_mut("claude-code").unwrap();
        target.source_mappings.insert(
            "skills".to_string(),
            vec![SourceMapping {
                source: PathBuf::from("ignored"),
                target: absolute_dest.path().to_path_buf(),
                mode: "symlink".to_string(),
            }],
        );
        config.plugins = vec![PluginRef {
            name: "demo".to_string(),
            namespace: None,
            version: None,
        }];

        let report = Syncer::sync(&config, project.path()).unwrap();
        assert!(report.all_succeeded());
        assert!(absolute_dest.path().join("demo").is_symlink());
    }
}
