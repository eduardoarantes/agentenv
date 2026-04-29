//! Project initialization: emit a starter `.agentrc.yaml`.

use crate::error::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Filename of the project configuration.
pub const CONFIG_FILENAME: &str = ".agentrc.yaml";

/// Default starter configuration emitted by `agentenv init`.
///
/// Plugins are intentionally empty so a fresh project can run `agentenv sync`
/// without the marketplace being populated.
pub const DEFAULT_CONFIG: &str = r#"version: 1

marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/eduardoarantes/claude-code-plugin-marketplace.git
    ref: main

# Add plugin entries here, e.g.:
#   - name: git-simple
plugins: []

targets:
  claude-code: {}

sync:
  onOpen: true
  refetch: true
  mode: symlink
"#;

/// Project initializer.
#[derive(Debug)]
pub struct Initializer;

impl Initializer {
    /// Write the default starter `.agentrc.yaml` at `project_root`.
    ///
    /// Returns the absolute path to the file that was written. If the file
    /// already exists and `force` is `false`, returns an error and does not
    /// modify the file.
    ///
    /// # Errors
    ///
    /// - `Error::Config` if the file exists and `force` is `false`.
    /// - `Error::Io` if the file cannot be written.
    pub fn write_default_config<P: AsRef<Path>>(project_root: P, force: bool) -> Result<PathBuf> {
        let path = project_root.as_ref().join(CONFIG_FILENAME);

        if path.exists() && !force {
            return Err(Error::Config(format!(
                "{} already exists; pass --force to overwrite",
                path.display()
            )));
        }

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        fs::write(&path, DEFAULT_CONFIG)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::ConfigLoader;
    use tempfile::TempDir;

    #[test]
    fn writes_starter_config_to_project_root() {
        let temp = TempDir::new().unwrap();
        let path = Initializer::write_default_config(temp.path(), false).unwrap();

        assert_eq!(path, temp.path().join(CONFIG_FILENAME));
        assert!(path.exists());
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("version: 1"));
        assert!(contents.contains("claude-code"));
    }

    #[test]
    fn refuses_to_overwrite_existing_config() {
        let temp = TempDir::new().unwrap();
        Initializer::write_default_config(temp.path(), false).unwrap();
        fs::write(temp.path().join(CONFIG_FILENAME), "version: 99\n").unwrap();

        let err = Initializer::write_default_config(temp.path(), false).unwrap_err();
        assert!(matches!(err, Error::Config(_)));

        // Existing file is untouched.
        let contents = fs::read_to_string(temp.path().join(CONFIG_FILENAME)).unwrap();
        assert_eq!(contents, "version: 99\n");
    }

    #[test]
    fn force_overwrites_existing_config() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(CONFIG_FILENAME), "version: 99\n").unwrap();

        Initializer::write_default_config(temp.path(), true).unwrap();
        let contents = fs::read_to_string(temp.path().join(CONFIG_FILENAME)).unwrap();
        assert!(contents.contains("version: 1"));
    }

    #[test]
    fn default_config_parses_and_validates() {
        let config = ConfigLoader::load_from_string(DEFAULT_CONFIG).unwrap();
        assert_eq!(config.version, 1);
        assert!(config.get_target("claude-code").is_some());
    }
}
