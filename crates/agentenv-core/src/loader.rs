//! Configuration loading and parsing

use crate::claude_config::ClaudeConfigLoader;
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
        Self::load_from_file_impl(path.as_ref(), None)
    }

    /// Test-only variant of [`Self::load_from_file`] that uses `home` instead
    /// of `dirs::home_dir()` when reading the global Claude `settings.json`.
    /// Keeps integration tests hermetic.
    #[cfg(test)]
    pub fn load_from_file_with_home<P: AsRef<Path>>(path: P, home: &Path) -> Result<Config> {
        Self::load_from_file_impl(path.as_ref(), Some(home))
    }

    fn load_from_file_impl(path: &Path, home_override: Option<&Path>) -> Result<Config> {
        let content = fs::read_to_string(path)?;
        let mut config: Config = serde_yaml::from_str(&content)?;

        // Claude import is implicit when `source: claude-code` — agentenv reads
        // marketplaces / plugins / hook config out of `.claude/settings.json`
        // and auto-wires `instruction_files` defaults. Missing settings.json
        // files are not an error; the import is best-effort.
        if config.source.as_deref() == Some("claude-code") {
            let project_root = path.parent().unwrap_or_else(|| Path::new("."));
            let import = match home_override {
                Some(home) => ClaudeConfigLoader::load_with_home(project_root, home)?,
                None => ClaudeConfigLoader::load(project_root)?,
            };
            config.merge_claude_import(import);
            crate::claude_config::apply_default_instruction_files(&mut config, project_root);
        }

        config.validate()?;
        Ok(config)
    }

    /// Load configuration from a YAML string
    ///
    /// Note: This entry point does NOT trigger Claude `settings.json` import
    /// because there is no project root to resolve `.claude/` against. Use
    /// [`Self::load_from_file`] for the full flow.
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
        Ok(config)
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
source: claude-code
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://example.com/marketplace.git
targets:
  cursor: {}
"#;

        let config = ConfigLoader::load_from_string(yaml);
        assert!(config.is_ok());
    }

    #[test]
    fn test_load_from_string_requires_source_when_targets_present() {
        let yaml = r#"
version: 1
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://example.com/marketplace.git
targets:
  cursor: {}
"#;
        let err = ConfigLoader::load_from_string(yaml).unwrap_err();
        assert!(err.to_string().contains("source"));
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
source: claude-code
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://example.com/marketplace.git
targets:
  cursor: {}
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
source: claude-code
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://example.com/marketplace.git
targets:
  cursor: {}
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

    #[test]
    fn test_load_from_file_with_source_claude_code_merges_settings() {
        use tempfile::TempDir;
        let project = TempDir::new().unwrap();
        let project_root = project.path();

        // Project .claude/settings.json with marketplaces + plugins.
        let claude_dir = project_root.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.json"),
            r#"{
                "extraKnownMarketplaces": {
                    "code-mp": {
                        "source": { "source": "git", "url": "https://example.com/m.git" }
                    }
                },
                "enabledPlugins": {
                    "ts-agents@code-mp": true
                }
            }"#,
        )
        .unwrap();

        let yaml = r#"
version: 1
source: claude-code
targets:
  cursor: {}
"#;
        std::fs::write(project_root.join(".agentrc.yaml"), yaml).unwrap();

        let home = TempDir::new().unwrap();
        let config =
            ConfigLoader::load_from_file_with_home(project_root.join(".agentrc.yaml"), home.path())
                .unwrap();
        assert!(config.marketplaces.contains_key("code-mp"));
        assert!(config.plugins.iter().any(|p| p.name == "ts-agents"));
        assert!(config.targets.contains_key("cursor"));
    }

    #[test]
    fn test_load_from_file_rejects_claude_code_target_when_source_is_claude_code() {
        use tempfile::TempDir;
        let project = TempDir::new().unwrap();
        let project_root = project.path();
        let claude_dir = project_root.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.json"),
            r#"{
                "extraKnownMarketplaces": {
                    "code-mp": {
                        "source": { "source": "git", "url": "https://example.com/m.git" }
                    }
                }
            }"#,
        )
        .unwrap();

        let yaml = r#"
version: 1
source: claude-code
targets:
  claude-code: {}
  cursor: {}
"#;
        std::fs::write(project_root.join(".agentrc.yaml"), yaml).unwrap();

        let home = TempDir::new().unwrap();
        // Previously the loader silently stripped `claude-code` from the
        // targets map. We now reject the overlap explicitly so the user
        // doesn't end up with a vanished config entry.
        let err =
            ConfigLoader::load_from_file_with_home(project_root.join(".agentrc.yaml"), home.path())
                .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("claude-code"), "got: {msg}");
        assert!(msg.contains("source"), "got: {msg}");
    }
}
