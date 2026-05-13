//! Project initialization: emit a starter `.agentrc.yaml`.

use crate::error::{Error, Result};
use crate::state::STATE_DIR;
use std::fs;
use std::path::{Path, PathBuf};

/// Filename of the project configuration.
pub const CONFIG_FILENAME: &str = ".agentrc.yaml";

/// Filename of the gitignore file at project root.
const GITIGNORE_FILENAME: &str = ".gitignore";

/// Pattern appended to `.gitignore` to exclude the per-project state dir.
/// The trailing slash matches git's directory-only convention.
const STATE_DIR_PATTERN: &str = ".agentenv/";

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

/// Outcome of [`Initializer::run`]: the config path that was written plus a
/// flag indicating whether `.gitignore` was modified to exclude the local
/// state directory.
#[derive(Debug)]
pub struct InitOutcome {
    /// Absolute path to the `.agentrc.yaml` that was written.
    pub config_path: PathBuf,
    /// `true` if `.gitignore` was created or appended to during init.
    pub gitignore_updated: bool,
}

/// Project initializer.
#[derive(Debug)]
pub struct Initializer;

impl Initializer {
    /// Write the default starter `.agentrc.yaml` at `project_root` and ensure
    /// `.gitignore` excludes the local `.agentenv/` state directory.
    ///
    /// Returns the path that was written and whether `.gitignore` was
    /// modified. If `.agentrc.yaml` already exists and `force` is `false`,
    /// returns an error and does not modify either file.
    ///
    /// # Errors
    ///
    /// - `Error::Config` if the file exists and `force` is `false`.
    /// - `Error::Io` if either file cannot be written.
    pub fn run<P: AsRef<Path>>(project_root: P, force: bool) -> Result<InitOutcome> {
        let project_root = project_root.as_ref();
        let config_path = Self::write_default_config(project_root, force)?;
        let gitignore_updated = ensure_state_dir_ignored(project_root)?;
        Ok(InitOutcome {
            config_path,
            gitignore_updated,
        })
    }

    /// Write the default starter `.agentrc.yaml` at `project_root`.
    ///
    /// Returns the absolute path to the file that was written. If the file
    /// already exists and `force` is `false`, returns an error and does not
    /// modify the file.
    ///
    /// Prefer [`Initializer::run`] for the full init flow; this helper is
    /// kept for callers that only want the config write.
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

/// Ensure `<project_root>/.gitignore` ignores the per-project state directory.
///
/// - Creates `.gitignore` with the pattern if no file exists.
/// - Appends the pattern (separated by a newline) when the file exists but
///   doesn't already ignore the state dir.
/// - No-op when any uncommented line already matches `.agentenv` or
///   `.agentenv/` (with or without a leading `/`).
///
/// Returns `true` if `.gitignore` was created or modified.
fn ensure_state_dir_ignored(project_root: &Path) -> Result<bool> {
    let path = project_root.join(GITIGNORE_FILENAME);
    let existing = match fs::read_to_string(&path) {
        Ok(content) => Some(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(Error::Io(err)),
    };

    if let Some(content) = &existing {
        if content.lines().any(line_ignores_state_dir) {
            return Ok(false);
        }
    }

    let new_content = match existing {
        Some(mut content) => {
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(STATE_DIR_PATTERN);
            content.push('\n');
            content
        },
        None => format!("{STATE_DIR_PATTERN}\n"),
    };

    fs::write(&path, new_content)?;
    Ok(true)
}

/// Returns `true` when `line` is an uncommented gitignore pattern that already
/// excludes the state directory. Accepts the four common spellings (with and
/// without leading `/`, with and without trailing `/`).
fn line_ignores_state_dir(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return false;
    }
    let normalized = trimmed.trim_start_matches('/').trim_end_matches('/');
    normalized == STATE_DIR
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

    #[test]
    fn run_creates_gitignore_when_absent() {
        let temp = TempDir::new().unwrap();
        let outcome = Initializer::run(temp.path(), false).unwrap();

        assert!(outcome.gitignore_updated);
        let gitignore = fs::read_to_string(temp.path().join(GITIGNORE_FILENAME)).unwrap();
        assert_eq!(gitignore, ".agentenv/\n");
    }

    #[test]
    fn run_appends_to_existing_gitignore_without_pattern() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join(GITIGNORE_FILENAME),
            "target/\nnode_modules/\n",
        )
        .unwrap();

        let outcome = Initializer::run(temp.path(), false).unwrap();

        assert!(outcome.gitignore_updated);
        let gitignore = fs::read_to_string(temp.path().join(GITIGNORE_FILENAME)).unwrap();
        assert_eq!(gitignore, "target/\nnode_modules/\n.agentenv/\n");
    }

    #[test]
    fn run_appends_newline_before_pattern_when_file_missing_trailing_newline() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(GITIGNORE_FILENAME), "target/").unwrap();

        Initializer::run(temp.path(), false).unwrap();

        let gitignore = fs::read_to_string(temp.path().join(GITIGNORE_FILENAME)).unwrap();
        assert_eq!(gitignore, "target/\n.agentenv/\n");
    }

    #[test]
    fn run_is_noop_when_pattern_already_present() {
        for pattern in [".agentenv", ".agentenv/", "/.agentenv", "/.agentenv/"] {
            let temp = TempDir::new().unwrap();
            let original = format!("target/\n{pattern}\n");
            fs::write(temp.path().join(GITIGNORE_FILENAME), &original).unwrap();

            let outcome = Initializer::run(temp.path(), false).unwrap();

            assert!(
                !outcome.gitignore_updated,
                "{pattern} should be recognized as already-ignored"
            );
            let gitignore = fs::read_to_string(temp.path().join(GITIGNORE_FILENAME)).unwrap();
            assert_eq!(gitignore, original, "{pattern} caused unexpected rewrite");
        }
    }

    #[test]
    fn run_treats_commented_pattern_as_inactive() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(GITIGNORE_FILENAME), "# .agentenv/\n").unwrap();

        let outcome = Initializer::run(temp.path(), false).unwrap();

        assert!(outcome.gitignore_updated);
        let gitignore = fs::read_to_string(temp.path().join(GITIGNORE_FILENAME)).unwrap();
        assert_eq!(gitignore, "# .agentenv/\n.agentenv/\n");
    }

    #[test]
    fn run_is_idempotent_on_gitignore() {
        let temp = TempDir::new().unwrap();
        let first = Initializer::run(temp.path(), false).unwrap();
        assert!(first.gitignore_updated);

        // Second run must overwrite config (force) but leave .gitignore alone.
        let second = Initializer::run(temp.path(), true).unwrap();
        assert!(!second.gitignore_updated);
        let gitignore = fs::read_to_string(temp.path().join(GITIGNORE_FILENAME)).unwrap();
        assert_eq!(gitignore, ".agentenv/\n");
    }

    #[test]
    fn run_without_force_does_not_touch_gitignore_when_config_exists() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(CONFIG_FILENAME), "version: 99\n").unwrap();

        let err = Initializer::run(temp.path(), false).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        assert!(
            !temp.path().join(GITIGNORE_FILENAME).exists(),
            "gitignore must not be touched when init aborts"
        );
    }
}
