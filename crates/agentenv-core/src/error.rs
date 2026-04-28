use thiserror::Error;

/// Result type for agentenv-core operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for agentenv-core
#[derive(Error, Debug)]
pub enum Error {
    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// YAML parsing error
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// Plugin resolution error
    #[error("Plugin resolution error: {0}")]
    PluginResolution(String),

    /// Network error
    #[error("Network error: {0}")]
    Network(String),

    /// Symlink error
    #[error("Symlink error: {0}")]
    Symlink(String),
}
