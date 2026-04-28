//! Plugin resolution and discovery

use crate::config::{Config, PluginRef};
use crate::error::{Error, Result};
use crate::marketplace::Marketplace;
use std::collections::HashMap;

/// Plugin resolver
#[derive(Debug)]
pub struct PluginResolver;

/// Resolved plugin with full metadata
#[derive(Debug, Clone)]
pub struct ResolvedPlugin {
    /// Plugin name
    pub name: String,

    /// Plugin version
    pub version: String,

    /// Marketplace namespace it came from
    pub namespace: String,

    /// Plugin location in marketplace
    pub location: String,

    /// Plugin metadata
    pub metadata: serde_json::Value,

    /// Supported targets declared by the plugin manifest
    pub targets: Vec<String>,

    /// Capabilities declared by the plugin manifest
    pub capabilities: Vec<String>,
}

impl PluginResolver {
    /// Resolve all plugins in configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration with plugins and marketplaces
    ///
    /// # Returns
    ///
    /// A vector of resolved plugins from all marketplaces
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A marketplace cannot be fetched
    /// - A plugin is not found in its marketplace
    /// - Plugin metadata is invalid
    pub fn resolve_all(config: &Config) -> Result<Vec<ResolvedPlugin>> {
        let mut resolved = Vec::new();

        for plugin in &config.plugins {
            let namespace = plugin.namespace.as_deref().unwrap_or("default");
            let resolved_plugin = Self::resolve_plugin(config, plugin, namespace)?;
            resolved.push(resolved_plugin);
        }

        Ok(resolved)
    }

    /// Resolve a single plugin
    fn resolve_plugin(
        config: &Config,
        plugin_ref: &PluginRef,
        namespace: &str,
    ) -> Result<ResolvedPlugin> {
        let marketplace = config.get_marketplace(namespace).ok_or_else(|| {
            crate::error::Error::Config(format!("marketplace namespace not found: {}", namespace))
        })?;

        let marketplace_index = Marketplace::load_from_path(&marketplace.path)?;
        let plugin = marketplace_index
            .find_plugin(&plugin_ref.name)
            .ok_or_else(|| {
                Error::PluginResolution(format!(
                    "plugin {} not found in marketplace namespace {} ({})",
                    plugin_ref.name,
                    namespace,
                    marketplace.path.display()
                ))
            })?;

        if let Some(requested_version) = &plugin_ref.version {
            if requested_version != "latest" && requested_version != &plugin.version {
                return Err(Error::PluginResolution(format!(
                    "plugin {} requested version {} but marketplace has {}",
                    plugin_ref.name, requested_version, plugin.version
                )));
            }
        }

        Self::validate_targets(config, plugin_ref, &plugin.targets)?;
        Self::validate_capabilities(config, plugin_ref, &plugin.capabilities, &plugin.location)?;

        Ok(ResolvedPlugin {
            name: plugin.name.clone(),
            version: plugin.version.clone(),
            namespace: namespace.to_string(),
            location: plugin.location.display().to_string(),
            metadata: plugin.metadata.clone(),
            targets: plugin.targets.clone(),
            capabilities: plugin.capabilities.clone(),
        })
    }

    fn validate_targets(
        config: &Config,
        plugin_ref: &PluginRef,
        supported_targets: &[String],
    ) -> Result<()> {
        if supported_targets.is_empty() {
            return Ok(());
        }

        let configured_targets = config.targets.iter().flat_map(|(target_name, target)| {
            std::iter::once(target_name.as_str()).chain(target.tools.iter().map(String::as_str))
        });

        if configured_targets.into_iter().any(|target| {
            supported_targets
                .iter()
                .any(|supported| supported == target)
        }) {
            return Ok(());
        }

        Err(Error::PluginResolution(format!(
            "plugin {} does not support configured targets; supported targets: {}",
            plugin_ref.name,
            supported_targets.join(", ")
        )))
    }

    fn validate_capabilities(
        config: &Config,
        plugin_ref: &PluginRef,
        capabilities: &[String],
        plugin_location: &std::path::Path,
    ) -> Result<()> {
        for capability in capabilities {
            let has_target_mapping = config
                .targets
                .values()
                .any(|target| target.source_mappings.contains_key(capability));

            if !has_target_mapping {
                return Err(Error::PluginResolution(format!(
                    "plugin {} declares unsupported capability {}",
                    plugin_ref.name, capability
                )));
            }

            let capability_path = plugin_location.join(capability);
            if !capability_path.exists() {
                return Err(Error::PluginResolution(format!(
                    "plugin {} missing capability folder {}",
                    plugin_ref.name,
                    capability_path.display()
                )));
            }
        }

        Ok(())
    }

    /// Get plugins grouped by namespace
    pub fn group_by_namespace(
        resolved: &[ResolvedPlugin],
    ) -> HashMap<String, Vec<&ResolvedPlugin>> {
        let mut grouped = HashMap::new();

        for plugin in resolved {
            grouped
                .entry(plugin.namespace.clone())
                .or_insert_with(Vec::new)
                .push(plugin);
        }

        grouped
    }

    /// Get plugins grouped by target tool
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration with targets
    /// * `resolved` - Resolved plugins
    ///
    /// # Returns
    ///
    /// Plugins mapped to their target tools
    pub fn group_by_target(
        config: &Config,
        _resolved: &[ResolvedPlugin],
    ) -> HashMap<String, Vec<String>> {
        let mut grouped = HashMap::new();

        for target_name in config.target_names() {
            if let Some(target) = config.get_target(target_name) {
                for tool in &target.tools {
                    grouped
                        .entry(tool.clone())
                        .or_insert_with(Vec::new)
                        .push(target_name.to_string());
                }
            }
        }

        grouped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MarketplaceConfig;
    use crate::targets::TargetDefaults;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_config(marketplace_path: PathBuf) -> Config {
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
            sync: Default::default(),
        }
    }

    fn write_plugin(marketplace_path: &std::path::Path, name: &str, version: &str) {
        let plugin_dir = marketplace_path.join("plugins").join(name);
        let manifest_dir = plugin_dir.join(".claude-plugin");
        fs::create_dir_all(plugin_dir.join("skills")).unwrap();
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(
            manifest_dir.join("plugin.json"),
            format!(
                r#"{{
  "name": "{name}",
  "version": "{version}",
  "description": "{name}",
  "targets": ["claude-code"],
  "capabilities": ["skills"],
  "metadata": {{}}
}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn test_resolve_single_plugin() {
        let temp_dir = TempDir::new().unwrap();
        write_plugin(temp_dir.path(), "test-plugin", "1.0.0");
        let mut config = create_test_config(temp_dir.path().to_path_buf());
        config.plugins = vec![PluginRef {
            name: "test-plugin".to_string(),
            namespace: Some("default".to_string()),
            version: Some("1.0.0".to_string()),
        }];

        let resolved = PluginResolver::resolve_all(&config);
        assert!(resolved.is_ok());

        let plugins = resolved.unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "test-plugin");
        assert_eq!(plugins[0].version, "1.0.0");
        assert_eq!(plugins[0].namespace, "default");
    }

    #[test]
    fn test_resolve_multiple_plugins() {
        let temp_dir = TempDir::new().unwrap();
        write_plugin(temp_dir.path(), "plugin1", "1.0.0");
        write_plugin(temp_dir.path(), "plugin2", "1.0.0");
        let mut config = create_test_config(temp_dir.path().to_path_buf());
        config.plugins = vec![
            PluginRef {
                name: "plugin1".to_string(),
                namespace: Some("default".to_string()),
                version: None,
            },
            PluginRef {
                name: "plugin2".to_string(),
                namespace: Some("default".to_string()),
                version: None,
            },
        ];

        let resolved = PluginResolver::resolve_all(&config).unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].name, "plugin1");
        assert_eq!(resolved[1].name, "plugin2");
    }

    #[test]
    fn test_resolve_plugin_default_version() {
        let temp_dir = TempDir::new().unwrap();
        write_plugin(temp_dir.path(), "test-plugin", "1.0.0");
        let mut config = create_test_config(temp_dir.path().to_path_buf());
        config.plugins = vec![PluginRef {
            name: "test-plugin".to_string(),
            namespace: Some("default".to_string()),
            version: None,
        }];

        let resolved = PluginResolver::resolve_all(&config).unwrap();
        assert_eq!(resolved[0].version, "1.0.0");
    }

    #[test]
    fn test_group_by_namespace() {
        let plugins = vec![
            ResolvedPlugin {
                name: "plugin1".to_string(),
                version: "1.0.0".to_string(),
                namespace: "default".to_string(),
                location: "/path/to/plugin1".to_string(),
                metadata: serde_json::json!({}),
                targets: vec!["claude-code".to_string()],
                capabilities: vec!["skills".to_string()],
            },
            ResolvedPlugin {
                name: "plugin2".to_string(),
                version: "1.0.0".to_string(),
                namespace: "default".to_string(),
                location: "/path/to/plugin2".to_string(),
                metadata: serde_json::json!({}),
                targets: vec!["claude-code".to_string()],
                capabilities: vec!["skills".to_string()],
            },
            ResolvedPlugin {
                name: "custom-plugin".to_string(),
                version: "1.0.0".to_string(),
                namespace: "custom".to_string(),
                location: "/path/to/custom-plugin".to_string(),
                metadata: serde_json::json!({}),
                targets: vec!["cursor".to_string()],
                capabilities: vec!["skills".to_string()],
            },
        ];

        let grouped = PluginResolver::group_by_namespace(&plugins);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped.get("default").unwrap().len(), 2);
        assert_eq!(grouped.get("custom").unwrap().len(), 1);
    }

    #[test]
    fn test_group_by_target() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(temp_dir.path().to_path_buf());
        let resolved = vec![];

        let grouped = PluginResolver::group_by_target(&config, &resolved);
        assert!(grouped.contains_key("claude-code"));
        assert_eq!(grouped.get("claude-code").unwrap()[0], "claude-code");
    }
}
