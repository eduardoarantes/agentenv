//! Managed-link state file.
//!
//! After every sync, agentenv writes the list of links it owns to
//! `<project>/.agentenv/state.json`. Subsequent runs use this manifest to
//! detect stale links (entries removed from `.agentrc.yaml` since the last
//! sync) and to power `agentenv clean`. Files outside the manifest are not
//! touched.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Project subdirectory that holds agentenv-managed bookkeeping.
pub const STATE_DIR: &str = ".agentenv";

/// Filename of the state manifest inside `STATE_DIR`.
pub const STATE_FILENAME: &str = "state.json";

/// On-disk record of every managed link agentenv has installed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    /// State schema version.
    pub version: u32,
    /// One entry per managed link.
    #[serde(default)]
    pub links: Vec<StateLink>,
}

/// A single managed link.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct StateLink {
    /// Absolute path the symlink points to (typically inside the marketplace).
    pub source: PathBuf,
    /// Absolute path where the symlink was installed (inside the project tree).
    pub target: PathBuf,
    /// Target tool name (e.g. `claude-code`).
    pub tool: String,
    /// Install mode (`symlink` or `copy`).
    pub mode: String,
    /// Owning plugin name.
    pub plugin: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: 1,
            links: Vec::new(),
        }
    }
}

impl State {
    /// Resolve the state file path for `project_root`.
    pub fn path(project_root: &Path) -> PathBuf {
        project_root.join(STATE_DIR).join(STATE_FILENAME)
    }

    /// Load the state file, returning an empty State if no file exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be parsed as JSON.
    pub fn load(project_root: &Path) -> Result<Self> {
        let path = Self::path(project_root);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)?;
        serde_json::from_str(&content)
            .map_err(|err| Error::Config(format!("invalid state file {}: {err}", path.display())))
    }

    /// Persist the state file under `<project_root>/.agentenv/state.json`.
    ///
    /// Creates the parent directory if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an IO error on write failure or a Config error if serialization
    /// fails.
    pub fn save(&self, project_root: &Path) -> Result<()> {
        let path = Self::path(project_root);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|err| Error::Config(format!("failed to serialize state: {err}")))?;
        fs::write(&path, json)?;
        Ok(())
    }

    /// Delete the state file and the `.agentenv/` directory if it's empty.
    pub fn remove(project_root: &Path) -> Result<()> {
        let path = Self::path(project_root);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        let dir = project_root.join(STATE_DIR);
        if dir.exists() && fs::read_dir(&dir)?.next().is_none() {
            fs::remove_dir(&dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn link(target: &Path, source: &Path) -> StateLink {
        StateLink {
            source: source.to_path_buf(),
            target: target.to_path_buf(),
            tool: "claude-code".to_string(),
            mode: "symlink".to_string(),
            plugin: "demo".to_string(),
        }
    }

    #[test]
    fn load_returns_empty_state_when_file_missing() {
        let project = TempDir::new().unwrap();
        let state = State::load(project.path()).unwrap();
        assert_eq!(state.version, 1);
        assert!(state.links.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let project = TempDir::new().unwrap();
        let mut state = State::default();
        state.links.push(link(
            &project.path().join(".claude/skills/demo"),
            &project.path().join("marketplace/plugins/demo/skills/demo"),
        ));
        state.save(project.path()).unwrap();

        let loaded = State::load(project.path()).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.links, state.links);
    }

    #[test]
    fn remove_deletes_file_and_empty_directory() {
        let project = TempDir::new().unwrap();
        State::default().save(project.path()).unwrap();
        assert!(State::path(project.path()).exists());

        State::remove(project.path()).unwrap();
        assert!(!State::path(project.path()).exists());
        assert!(!project.path().join(STATE_DIR).exists());
    }

    #[test]
    fn remove_is_idempotent_when_state_missing() {
        let project = TempDir::new().unwrap();
        // No state file written. remove() should not fail.
        State::remove(project.path()).unwrap();
    }

    #[test]
    fn invalid_json_in_state_file_is_a_config_error() {
        let project = TempDir::new().unwrap();
        let path = State::path(project.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not json").unwrap();

        let err = State::load(project.path()).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }
}
