//! Configuration model for `.agentrc.yaml`.
//!
//! Source-driven model — see `docs/HOOKS.md` and the broader plan:
//! - `source: <tool>` declares the source of truth for hooks/skills/agents
//! - `targets:` is a set membership map of tools to materialize the
//!   canonical for. Each `TargetConfig` is currently empty (`{}`); the
//!   field exists so future per-target options (e.g. `install_user_scope`)
//!   slot in without a schema break.
//!
//! Path conventions and write/refuse-on-conflict logic live in each
//! capability's writers module, not here.
//!
//! Constants at the bottom of this file declare which targets are known
//! sources / sinks; the capability pipelines and `Config::validate`
//! consult them.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Configuration for agentenv
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Configuration version
    pub version: u32,

    /// Marketplaces by namespace
    #[serde(default)]
    pub marketplaces: HashMap<String, MarketplaceConfig>,

    /// Plugins to import (optionally namespaced)
    #[serde(default)]
    pub plugins: Vec<PluginRef>,

    /// Target configurations (key = target name, value = target config).
    /// Empty per-target objects (`cursor: {}`) are the common case —
    /// opting a target in.
    #[serde(default)]
    pub targets: HashMap<String, TargetConfig>,

    /// Sync configuration
    #[serde(default)]
    pub sync: SyncConfig,

    /// Clean configuration
    #[serde(default)]
    pub clean: CleanConfig,

    /// When `true`, agentenv maintains a managed block in
    /// `<project>/.gitignore` listing every symlink/copy it currently owns
    /// (per `.agentenv/state.json`).
    #[serde(default)]
    pub gitignore_managed_links: bool,

    /// Cross-tool instruction-file propagation. Each key is a source file
    /// at the project root (e.g. `CLAUDE.md`); each value is a list of
    /// project-relative destination paths to symlink it from.
    #[serde(default)]
    pub instruction_files: HashMap<String, Vec<String>>,

    /// Source target whose native layout feeds the canonical pipelines.
    /// Read losslessly into `.agentenv/<capability>.canonical.yaml` and
    /// written out to every other configured supporting target. Mandatory
    /// when at least one target is configured.
    #[serde(default)]
    pub source: Option<String>,
}

/// Marketplace configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceConfig {
    /// Local path to marketplace
    pub path: PathBuf,

    /// Remote URL for marketplace
    pub remote: String,

    /// Git reference (branch/tag)
    #[serde(default = "default_ref")]
    pub r#ref: String,
}

impl MarketplaceConfig {
    /// Resolve [`MarketplaceConfig::path`] against `project_root`.
    pub fn resolve_path(&self, project_root: &Path) -> Result<PathBuf> {
        resolve_marketplace_path(&self.path, project_root)
    }
}

fn resolve_marketplace_path(path: &Path, project_root: &Path) -> Result<PathBuf> {
    let raw = path.to_string_lossy();
    let joined = if let Some(rest) = raw.strip_prefix("~/") {
        let home = dirs::home_dir()
            .ok_or_else(|| Error::Config("cannot determine home directory".to_string()))?;
        home.join(rest)
    } else if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };
    Ok(normalize_path(&joined))
}

/// Drop `.` segments and collapse `..` segments lexically.
pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir => {
                let popped = out.pop();
                if !popped {
                    out.push("..");
                }
            },
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// Plugin reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRef {
    /// Plugin name
    pub name: String,

    /// Marketplace namespace (optional, defaults to "default")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// Optional plugin version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Per-target configuration. Currently empty — the presence of a target
/// in `Config::targets` is enough to opt it in. The struct exists so
/// future per-target options can be added without a schema break.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetConfig {}

/// Sync configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncConfig {
    /// Run sync on editor open
    #[serde(default, rename = "onOpen")]
    pub on_open: bool,

    /// Re-fetch marketplace on sync
    #[serde(default)]
    pub refetch: bool,
}

/// Clean configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanConfig {
    #[serde(default = "default_true", rename = "pruneEmptyDirs")]
    pub prune_empty_dirs: bool,
}

impl Default for CleanConfig {
    fn default() -> Self {
        Self {
            prune_empty_dirs: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_ref() -> String {
    "main".to_string()
}

/// Target names that have a documented hooks convention.
pub const HOOK_CAPABLE_TARGETS: &[&str] = &["claude-code", "cursor", "codex", "copilot"];

/// Target names this v1 implementation can READ from (as `source`).
pub const SOURCE_TARGETS_V1: &[&str] = &["claude-code"];

/// Target names this v1 implementation can WRITE hooks to.
pub const HOOK_WRITE_TARGETS_V1: &[&str] = &["cursor", "codex"];

/// Every target name agentenv recognises (sum of known sources, hook
/// writers, and skill/agent writers). Used to validate user-typed target
/// names with a clear error.
pub const KNOWN_TARGETS: &[&str] = &[
    "claude-code",
    "cursor",
    "codex",
    "copilot",
    "gemini-cli",
    "junie",
    "antigravity",
];

impl Config {
    /// Validate configuration. Source-driven model requires `source:`
    /// whenever `targets:` is non-empty (canonical pipelines need somewhere
    /// to read from).
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(Error::Config(format!(
                "unsupported config version: {}",
                self.version
            )));
        }

        if let Some(source) = self.source.as_deref() {
            if !KNOWN_TARGETS.contains(&source) {
                return Err(Error::Config(format!(
                    "source: `{source}` is not a recognised target name; pick one of: {}",
                    KNOWN_TARGETS.join(", "),
                )));
            }
            if !SOURCE_TARGETS_V1.contains(&source) {
                return Err(Error::Config(format!(
                    "source: `{source}` is recognised but not yet implemented as a source; \
                     v1 supports: {}",
                    SOURCE_TARGETS_V1.join(", "),
                )));
            }
        }

        if self.marketplaces.is_empty() {
            let msg = if self.source.as_deref() == Some("claude-code") {
                "no marketplaces configured: `.agentrc.yaml` is empty and Claude's settings.json provided no `extraKnownMarketplaces`"
            } else {
                "at least one marketplace must be defined"
            };
            return Err(Error::Config(msg.to_string()));
        }

        if self.targets.is_empty() && self.source.is_none() && self.instruction_files.is_empty() {
            return Err(Error::Config(
                "at least one target must be defined".to_string(),
            ));
        }

        if !self.targets.is_empty() && self.source.is_none() {
            return Err(Error::Config(
                "`source:` is required when `targets:` is non-empty — every capability \
                 pipeline needs a source target to read from. v1 supports source: claude-code."
                    .to_string(),
            ));
        }

        for name in self.targets.keys() {
            if !KNOWN_TARGETS.contains(&name.as_str()) {
                return Err(Error::Config(format!(
                    "target `{name}` is not a recognised target name; pick one of: {}",
                    KNOWN_TARGETS.join(", "),
                )));
            }
        }

        for plugin in &self.plugins {
            let namespace = plugin.namespace.as_deref().unwrap_or("default");
            if !self.marketplaces.contains_key(namespace) {
                return Err(Error::Config(format!(
                    "plugin {} references unknown marketplace namespace: {}",
                    plugin.name, namespace
                )));
            }
        }

        Ok(())
    }

    /// Get marketplace by namespace
    pub fn get_marketplace(&self, namespace: &str) -> Option<&MarketplaceConfig> {
        self.marketplaces.get(namespace)
    }

    /// Get all marketplace namespaces
    pub fn marketplace_namespaces(&self) -> Vec<&str> {
        self.marketplaces.keys().map(String::as_str).collect()
    }

    /// Get plugins by namespace
    pub fn get_plugins_in_namespace(&self, namespace: &str) -> Vec<&PluginRef> {
        self.plugins
            .iter()
            .filter(|p| p.namespace.as_deref().unwrap_or("default") == namespace)
            .collect()
    }

    /// Get all target names
    pub fn target_names(&self) -> Vec<&str> {
        self.targets.keys().map(String::as_str).collect()
    }

    /// Get target configuration by name
    pub fn get_target(&self, name: &str) -> Option<&TargetConfig> {
        self.targets.get(name)
    }

    /// Layer a `ClaudeConfigImport` onto this config.
    pub fn merge_claude_import(&mut self, import: crate::claude_config::ClaudeConfigImport) {
        for (name, mp) in import.marketplaces {
            self.marketplaces.entry(name).or_insert(mp);
        }

        let mut existing: std::collections::HashSet<(String, String)> = self
            .plugins
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    p.namespace.clone().unwrap_or_else(|| "default".to_string()),
                )
            })
            .collect();
        for plugin in import.plugins {
            let key = (
                plugin.name.clone(),
                plugin
                    .namespace
                    .clone()
                    .unwrap_or_else(|| "default".to_string()),
            );
            if existing.insert(key) {
                self.plugins.push(plugin);
            }
        }

        if self.targets.remove("claude-code").is_some() {
            tracing::info!(
                "claude-code target dropped from sync because `source: claude-code` treats Claude as the source"
            );
        }
    }

    /// Names of every configured target that is a v1 hook write target,
    /// excluding `source` (always read-only).
    pub fn hook_write_targets(&self) -> Vec<String> {
        let source = self.source.as_deref();
        self.targets
            .keys()
            .filter(|name| HOOK_WRITE_TARGETS_V1.contains(&name.as_str()))
            .filter(|name| Some(name.as_str()) != source)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_marketplaces() -> HashMap<String, MarketplaceConfig> {
        let mut marketplaces = HashMap::new();
        marketplaces.insert(
            "default".to_string(),
            MarketplaceConfig {
                path: PathBuf::from("~/.agentenv/marketplace"),
                remote: "https://example.com/marketplace.git".to_string(),
                r#ref: "main".to_string(),
            },
        );
        marketplaces
    }

    fn base_config() -> Config {
        Config {
            version: 1,
            marketplaces: default_marketplaces(),
            plugins: vec![],
            targets: HashMap::new(),
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            gitignore_managed_links: false,
            instruction_files: HashMap::new(),
            source: None,
        }
    }

    #[test]
    fn validate_rejects_unsupported_version() {
        let mut config = base_config();
        config.version = 99;
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty_targets_with_no_source_and_no_instructions() {
        let config = base_config();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("at least one target"));
    }

    #[test]
    fn validate_rejects_empty_marketplaces() {
        let mut config = base_config();
        config.marketplaces.clear();
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        config.source = Some("claude-code".to_string());
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_requires_source_when_targets_non_empty() {
        let mut config = base_config();
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("source"), "got: {err}");
    }

    #[test]
    fn validate_accepts_source_claude_code() {
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_rejects_unknown_target_name() {
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config
            .targets
            .insert("nonsense".to_string(), TargetConfig::default());
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("nonsense"));
    }

    #[test]
    fn validate_rejects_source_not_in_known_targets() {
        let mut config = base_config();
        config.source = Some("junie".to_string());
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        let err = config.validate().unwrap_err();
        // junie IS in KNOWN_TARGETS but NOT in SOURCE_TARGETS_V1 — should
        // be rejected with the "not yet implemented" message.
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[test]
    fn validate_rejects_unknown_source_name() {
        let mut config = base_config();
        config.source = Some("nonsense".to_string());
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("nonsense"));
    }

    #[test]
    fn validate_allows_empty_targets_when_source_is_set() {
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn plugin_unknown_namespace_is_rejected() {
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        config.plugins.push(PluginRef {
            name: "x".to_string(),
            namespace: Some("nope".to_string()),
            version: None,
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn hook_write_targets_excludes_source_and_unknown_writers() {
        let mut config = base_config();
        config.source = Some("claude-code".to_string());
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        config
            .targets
            .insert("codex".to_string(), TargetConfig::default());
        config
            .targets
            .insert("gemini-cli".to_string(), TargetConfig::default());
        let mut targets = config.hook_write_targets();
        targets.sort();
        assert_eq!(targets, vec!["codex".to_string(), "cursor".to_string()]);
    }

    #[test]
    fn merge_claude_import_drops_claude_code_target() {
        let mut config = base_config();
        config
            .targets
            .insert("claude-code".to_string(), TargetConfig::default());
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        let import = crate::claude_config::ClaudeConfigImport::default();
        config.merge_claude_import(import);
        assert!(!config.targets.contains_key("claude-code"));
        assert!(config.targets.contains_key("cursor"));
    }
}
