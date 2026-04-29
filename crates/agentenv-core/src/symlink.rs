//! Symlink management for plugin installation

use crate::error::Result;
use std::fs;
use std::path::{Path, PathBuf};

/// Symlink manager for installing plugins to target tools
pub struct SymlinkManager;

/// Installation action
#[derive(Debug, Clone)]
pub struct InstallAction {
    /// Source path in marketplace
    pub source: PathBuf,

    /// Target path in tool
    pub target: PathBuf,

    /// Installation mode (symlink or copy)
    pub mode: String,

    /// Target tool name
    pub tool: String,
}

/// Installation result
#[derive(Debug, Clone)]
pub struct InstallResult {
    /// Action that was performed
    pub action: InstallAction,

    /// Whether installation succeeded
    pub success: bool,

    /// Result message
    pub message: String,
}

impl SymlinkManager {
    /// Create a symlink from source to target
    ///
    /// # Arguments
    ///
    /// * `source` - Source path (must exist)
    /// * `target` - Target symlink path
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Source does not exist
    /// - Target parent directory doesn't exist
    /// - Permission denied
    pub fn create_symlink<P: AsRef<Path>>(source: P, target: P) -> Result<()> {
        let source = source.as_ref();
        let target = target.as_ref();

        if !source.exists() {
            return Err(crate::error::Error::Symlink(format!(
                "source path does not exist: {}",
                source.display()
            )));
        }

        // Create parent directories if needed
        if let Some(parent) = target.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Remove existing target if it exists.
        if fs::symlink_metadata(target).is_ok() {
            remove_link_or_file(target)?;
        }

        // Create symlink
        #[cfg(unix)]
        std::os::unix::fs::symlink(source, target)?;

        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(source, target)?;

        Ok(())
    }

    /// Copy a directory/file from source to target
    ///
    /// # Arguments
    ///
    /// * `source` - Source path
    /// * `target` - Target path
    ///
    /// # Errors
    ///
    /// Returns error if copy fails
    pub fn copy_path<P: AsRef<Path>>(source: P, target: P) -> Result<()> {
        let source = source.as_ref();
        let target = target.as_ref();

        if !source.exists() {
            return Err(crate::error::Error::Symlink(format!(
                "source path does not exist: {}",
                source.display()
            )));
        }

        // Create parent directories if needed
        if let Some(parent) = target.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        if source.is_dir() {
            Self::copy_dir_recursive(source, target)?;
        } else {
            fs::copy(source, target)?;
        }

        Ok(())
    }

    /// Recursively copy a directory
    fn copy_dir_recursive<P: AsRef<Path>>(src: P, dst: P) -> Result<()> {
        let src = src.as_ref();
        let dst = dst.as_ref();

        fs::create_dir_all(dst)?;

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = entry.file_name();
            let target_path = dst.join(file_name);

            if path.is_dir() {
                Self::copy_dir_recursive(&path, &target_path)?;
            } else {
                fs::copy(&path, &target_path)?;
            }
        }

        Ok(())
    }

    /// Remove a symlink or file
    ///
    /// # Arguments
    ///
    /// * `path` - Path to remove
    ///
    /// # Errors
    ///
    /// Returns error if removal fails
    pub fn remove<P: AsRef<Path>>(path: P) -> Result<()> {
        let path = path.as_ref();

        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(_) => return Ok(()), // Already removed.
        };

        if meta.file_type().is_symlink() {
            remove_link_or_file(path)?;
        } else if meta.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }

        Ok(())
    }

    /// Install a plugin using the specified mode.
    pub fn install(action: &InstallAction) -> Result<InstallResult> {
        let result = match action.mode.as_str() {
            "symlink" => Self::create_symlink(&action.source, &action.target),
            "copy" => Self::copy_path(&action.source, &action.target),
            mode => {
                return Err(crate::error::Error::Config(format!(
                    "unknown installation mode: {}",
                    mode
                )))
            },
        };

        let (success, message) = match result {
            Ok(()) => (
                true,
                format!(
                    "installed {} to {} ({})",
                    action.source.display(),
                    action.target.display(),
                    action.mode
                ),
            ),
            Err(e) => (
                false,
                format!(
                    "failed to install {} to {}: {}",
                    action.source.display(),
                    action.target.display(),
                    e
                ),
            ),
        };

        Ok(InstallResult {
            action: action.clone(),
            success,
            message,
        })
    }
}

/// Remove a symlink or regular file from `path`.
///
/// On Unix `fs::remove_file` handles both file and directory symlinks. On
/// Windows `remove_file` rejects directory symlinks (Access Denied) and
/// `remove_dir` rejects file symlinks, so we try both.
fn remove_link_or_file(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(_) => fs::remove_dir(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.txt");
        let target = temp_dir.path().join("target.txt");

        fs::write(&source, "content").unwrap();

        let result = SymlinkManager::create_symlink(&source, &target);
        assert!(result.is_ok());
        assert!(target.is_symlink());
    }

    #[test]
    fn test_create_symlink_missing_source() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("nonexistent.txt");
        let target = temp_dir.path().join("target.txt");

        let result = SymlinkManager::create_symlink(&source, &target);
        assert!(result.is_err());
    }

    #[test]
    fn test_copy_file() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.txt");
        let target = temp_dir.path().join("target.txt");

        fs::write(&source, "content").unwrap();

        let result = SymlinkManager::copy_path(&source, &target);
        assert!(result.is_ok());
        assert!(target.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "content");
    }

    #[test]
    fn test_copy_directory() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("source");
        let target_dir = temp_dir.path().join("target");

        fs::create_dir(&source_dir).unwrap();
        fs::write(source_dir.join("file.txt"), "content").unwrap();

        let result = SymlinkManager::copy_path(&source_dir, &target_dir);
        assert!(result.is_ok());
        assert!(target_dir.exists());
        assert!(target_dir.join("file.txt").exists());
    }

    #[test]
    fn test_remove_file() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("test.txt");

        fs::write(&file, "content").unwrap();
        assert!(file.exists());

        let result = SymlinkManager::remove(&file);
        assert!(result.is_ok());
        assert!(!file.exists());
    }

    #[test]
    fn test_remove_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("nonexistent.txt");

        let result = SymlinkManager::remove(&file);
        assert!(result.is_ok()); // Should not error
    }

    #[test]
    fn test_install_with_symlink_mode() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");

        fs::create_dir(&source).unwrap();

        let action = InstallAction {
            source: source.clone(),
            target: target.clone(),
            mode: "symlink".to_string(),
            tool: "test-tool".to_string(),
        };

        let result = SymlinkManager::install(&action).unwrap();
        assert!(result.success);
        assert!(target.is_symlink());
    }

    #[test]
    fn test_install_with_copy_mode() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");

        fs::create_dir(&source).unwrap();
        fs::write(source.join("file.txt"), "content").unwrap();

        let action = InstallAction {
            source: source.clone(),
            target: target.clone(),
            mode: "copy".to_string(),
            tool: "test-tool".to_string(),
        };

        let result = SymlinkManager::install(&action).unwrap();
        assert!(result.success);
        assert!(target.exists());
        assert!(!target.is_symlink());
    }

    #[test]
    fn test_install_with_invalid_mode() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");

        fs::create_dir(&source).unwrap();

        let action = InstallAction {
            source,
            target,
            mode: "invalid-mode".to_string(),
            tool: "test-tool".to_string(),
        };

        let result = SymlinkManager::install(&action);
        assert!(result.is_err());
    }
}
