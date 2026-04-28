use crate::error::{Error, Result};
use crate::plugin::PluginManifest;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Marketplace for plugins
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marketplace {
    /// Marketplace version
    pub version: u32,

    /// Available plugins
    pub plugins: Vec<MarketplacePlugin>,
}

/// Plugin entry in marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePlugin {
    /// Plugin name
    pub name: String,

    /// Plugin version
    pub version: String,

    /// Plugin description
    pub description: String,

    /// Plugin metadata
    #[serde(default)]
    pub metadata: serde_json::Value,

    /// Supported targets
    #[serde(default)]
    pub targets: Vec<String>,

    /// Plugin capabilities
    #[serde(default)]
    pub capabilities: Vec<String>,

    /// Plugin location in the marketplace
    #[serde(skip)]
    pub location: PathBuf,
}

impl Marketplace {
    /// Find a plugin by name
    pub fn find_plugin(&self, name: &str) -> Option<&MarketplacePlugin> {
        self.plugins.iter().find(|p| p.name == name)
    }

    /// Load marketplace plugin manifests from a local marketplace directory.
    ///
    /// The expected layout is `plugins/<plugin>/.claude-plugin/plugin.json`.
    ///
    /// # Errors
    ///
    /// Returns an error if the marketplace path does not exist, plugin entries
    /// cannot be read, or a manifest is malformed.
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let plugins_dir = path.join("plugins");

        if !plugins_dir.exists() {
            return Err(Error::PluginResolution(format!(
                "marketplace plugins directory not found: {}",
                plugins_dir.display()
            )));
        }

        let mut plugins = Vec::new();

        for entry in fs::read_dir(&plugins_dir)? {
            let entry = entry?;
            let plugin_dir = entry.path();

            if !plugin_dir.is_dir() {
                continue;
            }

            let manifest_path = plugin_dir.join(".claude-plugin").join("plugin.json");
            if !manifest_path.exists() {
                let name = plugin_dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("<unknown>");
                return Err(Error::PluginResolution(format!(
                    "plugin {} is missing manifest: {}",
                    name,
                    manifest_path.display()
                )));
            }

            let manifest_content = fs::read_to_string(&manifest_path)?;
            let manifest: PluginManifest =
                serde_json::from_str(&manifest_content).map_err(|err| {
                    Error::PluginResolution(format!(
                        "invalid plugin manifest {}: {}",
                        manifest_path.display(),
                        err
                    ))
                })?;

            plugins.push(MarketplacePlugin {
                name: manifest.name,
                version: manifest.version,
                description: manifest.description,
                metadata: manifest.metadata,
                targets: manifest.targets,
                capabilities: manifest.capabilities,
                location: plugin_dir,
            });
        }

        Ok(Self {
            version: 1,
            plugins,
        })
    }

    /// Fetch marketplace from remote
    pub async fn fetch(_remote: &str) -> Result<Self> {
        // TODO: Implement marketplace fetching
        todo!()
    }
}
