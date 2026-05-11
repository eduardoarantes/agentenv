use crate::error::{Error, Result};
use crate::targets::TargetDefaults;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Configuration for agentenv
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Configuration version
    pub version: u32,

    /// Import marketplaces, plugins, and hooks from Claude Code's
    /// `settings.json` files (project `<root>/.claude/settings.json` and
    /// global `~/.claude/settings.json`). When `true`, the `claude-code`
    /// target is dropped from sync because Claude Code is treated as the
    /// source of truth, not a destination.
    #[serde(default)]
    pub use_claude_config: bool,

    /// Marketplaces by namespace
    #[serde(default)]
    pub marketplaces: HashMap<String, MarketplaceConfig>,

    /// Plugins to import (optionally namespaced)
    #[serde(default)]
    pub plugins: Vec<PluginRef>,

    /// Target configurations (key = target name, value = target config)
    #[serde(default)]
    pub targets: HashMap<String, TargetConfig>,

    /// Sync configuration
    #[serde(default)]
    pub sync: SyncConfig,

    /// Clean configuration
    #[serde(default)]
    pub clean: CleanConfig,

    /// Cross-tool instruction-file propagation. Each key is a source file
    /// at the project root (e.g. `CLAUDE.md`, `AGENTS.md`, `CURSOR.md`);
    /// each value is a list of project-relative destination paths to
    /// symlink it from (e.g. `.junie/AGENTS.md`).
    ///
    /// Never overrides existing files: if a destination already contains
    /// user content (regular file or foreign symlink), it is left untouched
    /// and a warning is emitted. Agentenv-managed symlinks pointing at a
    /// different source are updated; pointing at the same source are
    /// no-ops.
    #[serde(default)]
    pub instruction_files: HashMap<String, Vec<String>>,

    /// Runtime-only: hooks imported from Claude `settings.json`. Not
    /// serialized to or from disk; populated by `ClaudeConfigLoader` when
    /// `use_claude_config: true`. Exposed for the `claude-config show`
    /// command and reserved for future hook materialization.
    #[serde(skip)]
    pub claude_hooks: Option<serde_json::Value>,
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
    ///
    /// - `~/foo` → `$HOME/foo`
    /// - relative paths → joined with `project_root`
    /// - absolute paths → returned as-is
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
///
/// Does not touch the filesystem (unlike `canonicalize`), so it is safe to
/// call on paths that don't exist yet (e.g. a marketplace cache about to be
/// cloned).
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

/// Target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    /// Free-form target type identifier. Built-in defaults set this to the
    /// target name (e.g. `claude-code`, `cursor`, `codex`).
    #[serde(default)]
    pub r#type: String,

    /// Tools this target applies to
    #[serde(default)]
    pub tools: Vec<String>,

    /// Key paths for this target
    #[serde(default)]
    pub paths: HashMap<String, String>,

    /// Source-to-target mappings (category -> list of mappings)
    #[serde(default)]
    pub source_mappings: HashMap<String, Vec<SourceMapping>>,
}

/// Plugin-capability mapping for a target tool.
///
/// The source side is implicit: every resolved plugin contributes
/// `<plugin_location>/<capability>`. This struct only describes where a
/// plugin's capability folder is installed inside the target tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMapping {
    /// Target path in tool
    pub target: PathBuf,

    /// Installation mode (symlink or copy)
    #[serde(default = "default_mode")]
    pub mode: String,
}

/// Sync configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncConfig {
    /// Run sync on editor open
    #[serde(default, rename = "onOpen")]
    pub on_open: bool,

    /// Re-fetch marketplace on sync
    #[serde(default)]
    pub refetch: bool,

    /// Sync mode (symlink, copy, etc.)
    #[serde(default = "default_mode")]
    pub mode: String,
}

/// Clean configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanConfig {
    /// After removing managed links, walk back up each link's ancestor
    /// directories inside the project root and remove any that are now empty.
    /// Stops at the project root.
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

fn default_mode() -> String {
    "symlink".to_string()
}

impl Config {
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(crate::error::Error::Config(format!(
                "unsupported config version: {}",
                self.version
            )));
        }

        if self.marketplaces.is_empty() {
            let msg = if self.use_claude_config {
                "no marketplaces configured: `.agentrc.yaml` is empty and Claude's settings.json provided no `extraKnownMarketplaces`"
            } else {
                "at least one marketplace must be defined"
            };
            return Err(crate::error::Error::Config(msg.to_string()));
        }

        // Zero targets is valid when there's other work for sync to do:
        // - `use_claude_config: true` (claude-code may be dropped during
        //   merge, and `claude-config show` still works without targets)
        // - `instruction_files` propagation is configured (pure file→file
        //   linking, target-independent)
        if self.targets.is_empty()
            && !self.use_claude_config
            && self.instruction_files.is_empty()
        {
            return Err(crate::error::Error::Config(
                "at least one target must be defined".to_string(),
            ));
        }

        // Validate that all plugin namespaces exist
        for plugin in &self.plugins {
            let namespace = plugin.namespace.as_deref().unwrap_or("default");
            if !self.marketplaces.contains_key(namespace) {
                return Err(crate::error::Error::Config(format!(
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

    /// Apply defaults to targets
    ///
    /// Merges user-provided target configurations with project defaults.
    /// User configuration takes precedence over defaults.
    ///
    /// # Example
    ///
    /// If your `.agentrc.yaml` specifies:
    /// ```yaml
    /// targets:
    ///   claude-code:
    ///     paths:
    ///       config: /custom/path
    /// ```
    ///
    /// After apply_defaults, it will have:
    /// - All default source_mappings for claude-code
    /// - The custom config path
    /// - All other defaults preserved
    pub fn apply_defaults(mut self) -> Self {
        let mut merged_targets = HashMap::new();

        for (name, user_config) in self.targets {
            if let Some(default_config) = TargetDefaults::get(&name) {
                // Merge with defaults: user config takes precedence
                merged_targets.insert(name, user_config.merge_with_defaults(default_config));
            } else {
                // No defaults available, keep user config as-is
                merged_targets.insert(name, user_config);
            }
        }

        self.targets = merged_targets;
        self
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
    ///
    /// - Marketplaces from Claude fill in any namespace the user did not
    ///   define in `.agentrc.yaml`; user entries win on conflict.
    /// - Plugins from Claude are appended unless `(name, namespace)` already
    ///   appears in `self.plugins`.
    /// - The `claude-code` target is removed because Claude Code is treated
    ///   as the source of truth, not a sync destination, when
    ///   `use_claude_config: true`.
    /// - `hooks` are stashed on `claude_hooks` for surfacing via the CLI.
    ///   No file is mutated by this call.
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
                "claude-code target dropped because use_claude_config: true treats Claude as the source"
            );
        }

        self.claude_hooks = match import.hooks {
            serde_json::Value::Null => None,
            other => Some(other),
        };
    }
}

impl TargetConfig {
    /// Get all source mappings for a category
    pub fn get_mappings(&self, category: &str) -> Option<&Vec<SourceMapping>> {
        self.source_mappings.get(category)
    }

    /// Expand path variables (e.g., ~/ to home directory)
    pub fn expand_path(&self, path: &str) -> Result<PathBuf> {
        let expanded = if let Some(rest) = path.strip_prefix("~/") {
            let home = dirs::home_dir().ok_or_else(|| {
                crate::error::Error::Config("cannot determine home directory".to_string())
            })?;
            home.join(rest)
        } else {
            PathBuf::from(path)
        };
        Ok(expanded)
    }

    /// Merge user configuration with defaults
    ///
    /// User configuration takes precedence over defaults.
    /// If a field is empty in user config, it's filled from defaults.
    fn merge_with_defaults(self, defaults: TargetConfig) -> TargetConfig {
        let merged_type = if self.r#type.is_empty() {
            defaults.r#type
        } else {
            self.r#type
        };

        let merged_tools = if self.tools.is_empty() {
            defaults.tools
        } else {
            self.tools
        };

        // Merge paths: user paths take precedence
        let mut merged_paths = defaults.paths;
        for (key, value) in self.paths {
            merged_paths.insert(key, value);
        }

        // Merge source_mappings: user mappings take precedence
        let mut merged_mappings = defaults.source_mappings;
        for (category, mappings) in self.source_mappings {
            merged_mappings.insert(category, mappings);
        }

        TargetConfig {
            r#type: merged_type,
            tools: merged_tools,
            paths: merged_paths,
            source_mappings: merged_mappings,
        }
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

    #[test]
    fn test_config_validate_empty_targets() {
        let marketplaces = {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                MarketplaceConfig {
                    path: PathBuf::from("~/.agentenv/marketplace"),
                    remote: "https://example.com/marketplace.git".to_string(),
                    r#ref: "main".to_string(),
                },
            );
            m
        };

        let config = Config {
            version: 1,
            marketplaces,
            plugins: vec![],
            targets: HashMap::new(),
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_empty_marketplaces() {
        let mut targets = HashMap::new();
        targets.insert(
            "claude-code".to_string(),
            TargetConfig {
                r#type: "vscode-extension".to_string(),
                tools: vec!["claude-code".to_string()],
                paths: HashMap::new(),
                source_mappings: HashMap::new(),
            },
        );

        let config = Config {
            version: 1,
            marketplaces: HashMap::new(),
            plugins: vec![],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_success() {
        let marketplaces = {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                MarketplaceConfig {
                    path: PathBuf::from("~/.agentenv/marketplace"),
                    remote: "https://example.com/marketplace.git".to_string(),
                    r#ref: "main".to_string(),
                },
            );
            m
        };

        let mut targets = HashMap::new();
        targets.insert(
            "claude-code".to_string(),
            TargetConfig {
                r#type: "vscode-extension".to_string(),
                tools: vec!["claude-code".to_string()],
                paths: HashMap::new(),
                source_mappings: HashMap::new(),
            },
        );

        let config = Config {
            version: 1,
            marketplaces,
            plugins: vec![],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_plugin_unknown_namespace() {
        let marketplaces = {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                MarketplaceConfig {
                    path: PathBuf::from("~/.agentenv/marketplace"),
                    remote: "https://example.com/marketplace.git".to_string(),
                    r#ref: "main".to_string(),
                },
            );
            m
        };

        let mut targets = HashMap::new();
        targets.insert(
            "claude-code".to_string(),
            TargetConfig {
                r#type: "vscode-extension".to_string(),
                tools: vec![],
                paths: HashMap::new(),
                source_mappings: HashMap::new(),
            },
        );

        let config = Config {
            version: 1,
            marketplaces,
            plugins: vec![PluginRef {
                name: "test-plugin".to_string(),
                namespace: Some("unknown-namespace".to_string()),
                version: None,
            }],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_plugin_defaults_to_default_namespace() {
        let marketplaces = {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                MarketplaceConfig {
                    path: PathBuf::from("~/.agentenv/marketplace"),
                    remote: "https://example.com/marketplace.git".to_string(),
                    r#ref: "main".to_string(),
                },
            );
            m
        };

        let mut targets = HashMap::new();
        targets.insert(
            "claude-code".to_string(),
            TargetConfig {
                r#type: "vscode-extension".to_string(),
                tools: vec![],
                paths: HashMap::new(),
                source_mappings: HashMap::new(),
            },
        );

        let config = Config {
            version: 1,
            marketplaces,
            plugins: vec![PluginRef {
                name: "test-plugin".to_string(),
                namespace: None, // Should default to "default"
                version: None,
            }],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_get_marketplace() {
        let marketplaces = {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                MarketplaceConfig {
                    path: PathBuf::from("~/.agentenv/marketplace"),
                    remote: "https://example.com/marketplace.git".to_string(),
                    r#ref: "main".to_string(),
                },
            );
            m.insert(
                "custom".to_string(),
                MarketplaceConfig {
                    path: PathBuf::from("~/.agentenv/custom"),
                    remote: "https://custom.com/marketplace.git".to_string(),
                    r#ref: "main".to_string(),
                },
            );
            m
        };

        let mut targets = HashMap::new();
        targets.insert(
            "claude-code".to_string(),
            TargetConfig {
                r#type: "vscode-extension".to_string(),
                tools: vec![],
                paths: HashMap::new(),
                source_mappings: HashMap::new(),
            },
        );

        let config = Config {
            version: 1,
            marketplaces,
            plugins: vec![],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        assert!(config.get_marketplace("default").is_some());
        assert!(config.get_marketplace("custom").is_some());
        assert!(config.get_marketplace("unknown").is_none());
    }

    #[test]
    fn test_target_names() {
        let mut targets = HashMap::new();
        targets.insert(
            "claude-code".to_string(),
            TargetConfig {
                r#type: "vscode-extension".to_string(),
                tools: vec![],
                paths: HashMap::new(),
                source_mappings: HashMap::new(),
            },
        );
        targets.insert(
            "cursor".to_string(),
            TargetConfig {
                r#type: "cursor-extension".to_string(),
                tools: vec![],
                paths: HashMap::new(),
                source_mappings: HashMap::new(),
            },
        );

        let config = Config {
            version: 1,
            marketplaces: default_marketplaces(),
            plugins: vec![],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        let names = config.target_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"claude-code"));
        assert!(names.contains(&"cursor"));
    }

    #[test]
    fn test_source_mapping_defaults_to_symlink_mode() {
        let mut targets = HashMap::new();
        let claude_code_target = TargetConfig {
            r#type: "vscode-extension".to_string(),
            tools: vec!["claude-code".to_string()],
            paths: HashMap::new(),
            source_mappings: HashMap::new(),
        };
        targets.insert("claude-code".to_string(), claude_code_target);

        let config = Config {
            version: 1,
            marketplaces: default_marketplaces(),
            plugins: vec![],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_apply_defaults_to_claude_code() {
        let mut targets = HashMap::new();
        let claude_code_target = TargetConfig {
            r#type: "".to_string(), // Empty, should use default
            tools: vec![],          // Empty, should use default
            paths: HashMap::new(),
            source_mappings: HashMap::new(),
        };
        targets.insert("claude-code".to_string(), claude_code_target);

        let config = Config {
            version: 1,
            marketplaces: default_marketplaces(),
            plugins: vec![],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        let merged = config.apply_defaults();
        let claude_target = merged.get_target("claude-code").unwrap();

        assert_eq!(claude_target.r#type, "claude-code");
        assert!(claude_target.tools.contains(&"claude-code".to_string()));
        assert_eq!(
            claude_target.source_mappings.get("skills").unwrap()[0]
                .target
                .to_string_lossy(),
            ".claude/skills"
        );
        assert!(claude_target.source_mappings.contains_key("commands"));
        assert!(claude_target.source_mappings.contains_key("agents"));
    }

    #[test]
    fn test_apply_defaults_user_config_takes_precedence() {
        let mut targets = HashMap::new();
        let mut user_paths = HashMap::new();
        user_paths.insert("config".to_string(), "/custom/path".to_string());

        let claude_code_target = TargetConfig {
            r#type: "custom-type".to_string(), // Override default
            tools: vec!["custom-tool".to_string()],
            paths: user_paths,
            source_mappings: HashMap::new(),
        };
        targets.insert("claude-code".to_string(), claude_code_target);

        let config = Config {
            version: 1,
            marketplaces: default_marketplaces(),
            plugins: vec![],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        let merged = config.apply_defaults();
        let claude_target = merged.get_target("claude-code").unwrap();

        // User config should take precedence
        assert_eq!(claude_target.r#type, "custom-type");
        assert_eq!(claude_target.tools[0], "custom-tool");
        assert_eq!(claude_target.paths.get("config").unwrap(), "/custom/path");

        // But defaults should be merged for source_mappings
        assert!(claude_target.source_mappings.contains_key("skills"));
    }

    #[test]
    fn test_apply_defaults_unknown_target() {
        let mut targets = HashMap::new();
        let unknown_target = TargetConfig {
            r#type: "unknown-type".to_string(),
            tools: vec![],
            paths: HashMap::new(),
            source_mappings: HashMap::new(),
        };
        targets.insert("unknown-target".to_string(), unknown_target);

        let config = Config {
            version: 1,
            marketplaces: default_marketplaces(),
            plugins: vec![],
            targets,
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };

        let merged = config.apply_defaults();
        let unknown = merged.get_target("unknown-target").unwrap();

        // Unknown targets should keep their user config as-is
        assert_eq!(unknown.r#type, "unknown-type");
        assert!(unknown.source_mappings.is_empty());
    }

    fn empty_config() -> Config {
        Config {
            version: 1,
            marketplaces: HashMap::new(),
            plugins: vec![],
            targets: HashMap::new(),
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            instruction_files: HashMap::new(),
            use_claude_config: true,
            claude_hooks: None,
        }
    }

    #[test]
    fn merge_claude_import_fills_empty_marketplaces() {
        let mut config = empty_config();
        let mut marketplaces = HashMap::new();
        marketplaces.insert(
            "claude-mp".to_string(),
            MarketplaceConfig {
                path: PathBuf::from("~/.agentenv/marketplaces/claude-mp"),
                remote: "https://example.com/claude.git".to_string(),
                r#ref: "main".to_string(),
            },
        );
        let import = crate::claude_config::ClaudeConfigImport {
            marketplaces,
            plugins: vec![],
            hooks: serde_json::Value::Null,
        };
        config.merge_claude_import(import);
        assert!(config.marketplaces.contains_key("claude-mp"));
    }

    #[test]
    fn merge_claude_import_user_marketplace_wins() {
        let mut config = empty_config();
        config.marketplaces.insert(
            "shared".to_string(),
            MarketplaceConfig {
                path: PathBuf::from("~/custom"),
                remote: "https://user.example.com/m.git".to_string(),
                r#ref: "main".to_string(),
            },
        );
        let mut marketplaces = HashMap::new();
        marketplaces.insert(
            "shared".to_string(),
            MarketplaceConfig {
                path: PathBuf::from("~/.agentenv/marketplaces/shared"),
                remote: "https://claude.example.com/m.git".to_string(),
                r#ref: "main".to_string(),
            },
        );
        let import = crate::claude_config::ClaudeConfigImport {
            marketplaces,
            plugins: vec![],
            hooks: serde_json::Value::Null,
        };
        config.merge_claude_import(import);
        let shared = config.marketplaces.get("shared").unwrap();
        assert_eq!(shared.remote, "https://user.example.com/m.git");
    }

    #[test]
    fn merge_claude_import_dedupes_plugins() {
        let mut config = empty_config();
        config.plugins.push(PluginRef {
            name: "shared".to_string(),
            namespace: Some("m".to_string()),
            version: None,
        });
        let import = crate::claude_config::ClaudeConfigImport {
            marketplaces: HashMap::new(),
            plugins: vec![
                PluginRef {
                    name: "shared".to_string(),
                    namespace: Some("m".to_string()),
                    version: None,
                },
                PluginRef {
                    name: "new".to_string(),
                    namespace: Some("m".to_string()),
                    version: None,
                },
            ],
            hooks: serde_json::Value::Null,
        };
        config.merge_claude_import(import);
        assert_eq!(config.plugins.len(), 2);
        assert!(config.plugins.iter().any(|p| p.name == "new"));
    }

    #[test]
    fn merge_claude_import_drops_claude_code_target() {
        let mut config = empty_config();
        config.targets.insert(
            "claude-code".to_string(),
            TargetConfig {
                r#type: String::new(),
                tools: vec![],
                paths: HashMap::new(),
                source_mappings: HashMap::new(),
            },
        );
        config.targets.insert(
            "cursor".to_string(),
            TargetConfig {
                r#type: String::new(),
                tools: vec![],
                paths: HashMap::new(),
                source_mappings: HashMap::new(),
            },
        );
        let import = crate::claude_config::ClaudeConfigImport {
            marketplaces: HashMap::new(),
            plugins: vec![],
            hooks: serde_json::Value::Null,
        };
        config.merge_claude_import(import);
        assert!(!config.targets.contains_key("claude-code"));
        assert!(config.targets.contains_key("cursor"));
    }

    #[test]
    fn merge_claude_import_stashes_hooks() {
        let mut config = empty_config();
        let hooks = serde_json::json!({ "Stop": [{ "matcher": ".*" }] });
        let import = crate::claude_config::ClaudeConfigImport {
            marketplaces: HashMap::new(),
            plugins: vec![],
            hooks: hooks.clone(),
        };
        config.merge_claude_import(import);
        assert_eq!(config.claude_hooks, Some(hooks));
    }

    #[test]
    fn validate_allows_empty_targets_when_use_claude_config_true() {
        let mut config = empty_config();
        config.marketplaces.insert(
            "m".to_string(),
            MarketplaceConfig {
                path: PathBuf::from("~/m"),
                remote: "https://example.com/m.git".to_string(),
                r#ref: "main".to_string(),
            },
        );
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_still_requires_some_marketplace_after_claude_merge() {
        let mut config = empty_config();
        let import = crate::claude_config::ClaudeConfigImport::default();
        config.merge_claude_import(import);
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("no marketplaces configured"));
    }
}
