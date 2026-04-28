//! Configuration loading and parsing

use crate::config::Config;
use crate::error::Result;
use std::fs;
use std::path::Path;

/// Configuration loader
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration from a YAML file
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the `.agentrc.yaml` file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - File cannot be read
    /// - YAML is malformed
    /// - Configuration fails validation
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Config> {
        let content = fs::read_to_string(path)?;
        let config: Config = serde_yaml::from_str(&content)?;
        config.validate()?;
        Ok(config.apply_defaults())
    }

    /// Load configuration from a YAML string
    ///
    /// # Arguments
    ///
    /// * `yaml_str` - YAML configuration as string
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - YAML is malformed
    /// - Configuration fails validation
    pub fn load_from_string(yaml_str: &str) -> Result<Config> {
        let config: Config = serde_yaml::from_str(yaml_str)?;
        config.validate()?;
        Ok(config.apply_defaults())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_from_string_valid() {
        let yaml = r#"
version: 1
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://example.com/marketplace.git
targets:
  claude-code: {}
"#;

        let config = ConfigLoader::load_from_string(yaml);
        assert!(config.is_ok());
    }

    #[test]
    fn test_load_from_string_invalid_yaml() {
        let yaml = "invalid: [yaml: structure";
        let result = ConfigLoader::load_from_string(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_string_invalid_version() {
        let yaml = r#"
version: 99
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://example.com/marketplace.git
targets:
  claude-code: {}
"#;

        let result = ConfigLoader::load_from_string(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_string_no_targets() {
        let yaml = r#"
version: 1
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://example.com/marketplace.git
targets: {}
"#;

        let result = ConfigLoader::load_from_string(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_file() -> std::io::Result<()> {
        let yaml = r#"
version: 1
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://example.com/marketplace.git
targets:
  claude-code: {}
"#;

        let mut file = NamedTempFile::new()?;
        file.write_all(yaml.as_bytes())?;
        file.flush()?;

        let result = ConfigLoader::load_from_file(file.path());
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_load_from_file_not_found() {
        let result = ConfigLoader::load_from_file("/nonexistent/path/.agentrc.yaml");
        assert!(result.is_err());
    }
}
