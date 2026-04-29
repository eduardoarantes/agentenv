use serde::{Deserialize, Serialize};

/// A plugin with its configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    /// Plugin name
    pub name: String,

    /// Plugin version
    pub version: String,

    /// Plugin location
    pub location: String,

    /// Targets this plugin applies to
    pub targets: Vec<String>,

    /// Plugin metadata
    #[serde(default)]
    pub metadata: serde_json::Value,
}
