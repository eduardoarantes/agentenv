use crate::config::{normalize_path, MarketplaceConfig};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Capability folders that agentenv recognises inside a plugin directory.
///
/// A plugin's capabilities are inferred from which of these folders exist;
/// there is no per-plugin manifest file.
const KNOWN_CAPABILITIES: &[&str] = &["agents", "commands", "skills", "hooks"];

/// Top-level marketplace index file (Claude Code marketplace convention).
#[derive(Debug, Deserialize)]
struct MarketplaceIndex {
    #[serde(default)]
    plugins: Vec<MarketplaceIndexEntry>,
}

#[derive(Debug, Deserialize)]
struct MarketplaceIndexEntry {
    name: String,
    /// Path to the plugin directory, relative to the marketplace root.
    source: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
}

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

    /// Load a marketplace from a local directory.
    ///
    /// Reads `<path>/.claude-plugin/marketplace.json` (Claude Code marketplace
    /// convention) and returns one [`MarketplacePlugin`] per `plugins[]` entry.
    /// Per-plugin capabilities are inferred from which subdirectories exist
    /// inside the plugin's `source` folder ([`KNOWN_CAPABILITIES`]).
    ///
    /// # Errors
    ///
    /// Returns an error if the marketplace index is missing, malformed, or
    /// references a plugin source directory that does not exist.
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let root = path.as_ref();
        let index_path = root.join(".claude-plugin").join("marketplace.json");

        if !index_path.exists() {
            return Err(Error::PluginResolution(format!(
                "marketplace index not found: {}",
                index_path.display()
            )));
        }

        let content = fs::read_to_string(&index_path)?;
        let index: MarketplaceIndex = serde_json::from_str(&content).map_err(|err| {
            Error::PluginResolution(format!(
                "invalid marketplace index {}: {err}",
                index_path.display()
            ))
        })?;

        let mut plugins = Vec::with_capacity(index.plugins.len());
        for entry in index.plugins {
            let source = normalize_path(&root.join(&entry.source));
            if !source.is_dir() {
                return Err(Error::PluginResolution(format!(
                    "plugin {} references missing source directory: {}",
                    entry.name,
                    source.display()
                )));
            }

            let capabilities = KNOWN_CAPABILITIES
                .iter()
                .filter(|cap| source.join(cap).is_dir())
                .map(|cap| (*cap).to_string())
                .collect();

            plugins.push(MarketplacePlugin {
                name: entry.name,
                version: entry.version,
                description: entry.description,
                metadata: serde_json::Value::Null,
                targets: Vec::new(),
                capabilities,
                location: source,
            });
        }

        Ok(Self {
            version: 1,
            plugins,
        })
    }

    /// Make sure a marketplace is available on disk under
    /// `config.resolve_path(project_root)`.
    ///
    /// Behaviour depends on the requested `EnsureBehavior` and on whether the
    /// resolved path already exists. The marketplace cache directory is
    /// treated as agentenv-managed: refetch operations will reset the working
    /// tree to `origin/<ref>`. Don't put hand-edited content there.
    ///
    /// # Errors
    ///
    /// - `EnsureBehavior::Offline` with no local copy → `Error::Network`.
    /// - Initial clone failure → `Error::Network` (no local copy to fall back
    ///   on).
    /// - `git` not on `PATH` → `Error::Network` with a hint.
    pub fn ensure(
        config: &MarketplaceConfig,
        project_root: &Path,
        behavior: EnsureBehavior,
    ) -> Result<EnsureOutcome> {
        let path = config.resolve_path(project_root)?;
        let exists = path.exists();

        match (exists, behavior) {
            (false, EnsureBehavior::Offline) => Err(Error::Network(format!(
                "marketplace at {} is missing and offline mode was requested",
                path.display()
            ))),
            (false, _) => {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)?;
                    }
                }
                git_clone(&config.remote, &path, &config.r#ref)?;
                Ok(EnsureOutcome::Cloned)
            },
            (true, EnsureBehavior::Cache) | (true, EnsureBehavior::Offline) => {
                Ok(EnsureOutcome::Reused)
            },
            (true, EnsureBehavior::Refetch) => match git_fetch_and_reset(&path, &config.r#ref) {
                Ok(()) => Ok(EnsureOutcome::Fetched),
                Err(err) => Ok(EnsureOutcome::FetchFailedReused(err.to_string())),
            },
        }
    }
}

/// How `Marketplace::ensure` should treat an existing marketplace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureBehavior {
    /// Clone if missing; fetch + reset if present.
    Refetch,
    /// Clone if missing; reuse if present.
    Cache,
    /// Reuse if present; error if missing.
    Offline,
}

/// What `Marketplace::ensure` actually did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureOutcome {
    /// No local copy existed; cloned from remote.
    Cloned,
    /// Local copy existed; fetched and reset to `origin/<ref>`.
    Fetched,
    /// Local copy existed and was left untouched.
    Reused,
    /// Refetch was requested but the network call failed; the local copy is
    /// kept as-is. Carries the error message for surfacing as a warning.
    FetchFailedReused(String),
}

fn git_clone(remote: &str, path: &Path, refspec: &str) -> Result<()> {
    // `-c core.autocrlf=false`: marketplace content is agentenv-managed; we
    // don't want git's CRLF heuristic mangling skill files on Windows.
    let output = Command::new("git")
        .args([
            "-c",
            "core.autocrlf=false",
            "clone",
            "--branch",
            refspec,
            "--single-branch",
            "--",
            remote,
        ])
        .arg(path)
        .output()
        .map_err(|err| {
            Error::Network(format!(
                "failed to invoke git: {err}. Is git installed and on PATH?"
            ))
        })?;

    if !output.status.success() {
        return Err(Error::Network(format!(
            "git clone of {remote} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    // Persist the autocrlf=false setting so subsequent fetch + reset don't
    // mangle line endings.
    let _ = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "core.autocrlf", "false"])
        .output();

    Ok(())
}

fn git_fetch_and_reset(path: &Path, refspec: &str) -> Result<()> {
    let fetch = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["fetch", "origin", refspec])
        .output()
        .map_err(|err| Error::Network(format!("failed to invoke git: {err}")))?;

    if !fetch.status.success() {
        return Err(Error::Network(format!(
            "git fetch failed: {}",
            String::from_utf8_lossy(&fetch.stderr).trim()
        )));
    }

    let reset = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["reset", "--hard", "FETCH_HEAD"])
        .output()
        .map_err(|err| Error::Network(format!("failed to invoke git: {err}")))?;

    if !reset.status.success() {
        return Err(Error::Network(format!(
            "git reset --hard FETCH_HEAD failed: {}",
            String::from_utf8_lossy(&reset.stderr).trim()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod ensure_tests {
    use super::*;
    use std::ffi::OsStr;
    use tempfile::TempDir;

    /// Run `git` with `args` in `cwd`, panic with stderr on failure.
    fn run_git<I, S>(cwd: &Path, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        if !output.status.success() {
            panic!(
                "git failed in {}: {}",
                cwd.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    /// Initialise a bare repo at `<base>/remote.git` and seed it with a single
    /// commit on `branch` containing the supplied files. Returns the bare repo
    /// path.
    fn seed_remote(base: &Path, branch: &str, files: &[(&str, &str)]) -> PathBuf {
        let bare = base.join("remote.git");
        let workdir = base.join("seed");
        fs::create_dir_all(&bare).unwrap();
        fs::create_dir_all(&workdir).unwrap();

        run_git(
            base,
            ["init", "--bare", "-b", branch, bare.to_str().unwrap()],
        );
        run_git(base, ["init", "-b", branch, workdir.to_str().unwrap()]);
        run_git(&workdir, ["config", "user.email", "test@example.com"]);
        run_git(&workdir, ["config", "user.name", "Test"]);
        run_git(&workdir, ["config", "commit.gpgsign", "false"]);
        run_git(&workdir, ["config", "core.autocrlf", "false"]);

        for (rel, contents) in files {
            let dest = workdir.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&dest, contents).unwrap();
        }

        run_git(&workdir, ["add", "-A"]);
        run_git(&workdir, ["commit", "-m", "seed"]);
        run_git(
            &workdir,
            ["remote", "add", "origin", bare.to_str().unwrap()],
        );
        run_git(&workdir, ["push", "origin", branch]);

        bare
    }

    /// Push a follow-up commit with the given files into the bare remote.
    fn push_followup(base: &Path, branch: &str, files: &[(&str, &str)]) {
        let workdir = base.join("seed");
        for (rel, contents) in files {
            let dest = workdir.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&dest, contents).unwrap();
        }
        run_git(&workdir, ["add", "-A"]);
        run_git(&workdir, ["commit", "-m", "followup"]);
        run_git(&workdir, ["push", "origin", branch]);
    }

    fn config_for(remote: &Path, local: &Path, branch: &str) -> MarketplaceConfig {
        MarketplaceConfig {
            path: local.to_path_buf(),
            remote: remote.to_string_lossy().into_owned(),
            r#ref: branch.to_string(),
        }
    }

    #[test]
    fn ensure_clones_when_missing() {
        let scratch = TempDir::new().unwrap();
        let bare = seed_remote(scratch.path(), "main", &[("README.md", "hello\n")]);
        let local = scratch.path().join("local");
        let config = config_for(&bare, &local, "main");

        let outcome = Marketplace::ensure(&config, scratch.path(), EnsureBehavior::Cache).unwrap();
        assert_eq!(outcome, EnsureOutcome::Cloned);
        assert_eq!(
            fs::read_to_string(local.join("README.md")).unwrap(),
            "hello\n"
        );
    }

    #[test]
    fn ensure_reuses_existing_local_in_cache_mode() {
        let scratch = TempDir::new().unwrap();
        let bare = seed_remote(scratch.path(), "main", &[("README.md", "hello\n")]);
        let local = scratch.path().join("local");
        let config = config_for(&bare, &local, "main");

        Marketplace::ensure(&config, scratch.path(), EnsureBehavior::Cache).unwrap();
        // Tamper with the working tree to confirm Cache leaves it alone.
        fs::write(local.join("README.md"), "tampered\n").unwrap();

        let outcome = Marketplace::ensure(&config, scratch.path(), EnsureBehavior::Cache).unwrap();
        assert_eq!(outcome, EnsureOutcome::Reused);
        assert_eq!(
            fs::read_to_string(local.join("README.md")).unwrap(),
            "tampered\n"
        );
    }

    #[test]
    fn ensure_refetches_picks_up_remote_changes() {
        let scratch = TempDir::new().unwrap();
        let bare = seed_remote(scratch.path(), "main", &[("README.md", "v1\n")]);
        let local = scratch.path().join("local");
        let config = config_for(&bare, &local, "main");

        Marketplace::ensure(&config, scratch.path(), EnsureBehavior::Cache).unwrap();
        push_followup(scratch.path(), "main", &[("README.md", "v2\n")]);

        let outcome =
            Marketplace::ensure(&config, scratch.path(), EnsureBehavior::Refetch).unwrap();
        assert_eq!(outcome, EnsureOutcome::Fetched);
        assert_eq!(fs::read_to_string(local.join("README.md")).unwrap(), "v2\n");
    }

    #[test]
    fn ensure_offline_errors_when_missing() {
        let scratch = TempDir::new().unwrap();
        let local = scratch.path().join("never-cloned");
        let config = MarketplaceConfig {
            path: local,
            remote: "https://example.invalid/missing.git".to_string(),
            r#ref: "main".to_string(),
        };

        let err =
            Marketplace::ensure(&config, scratch.path(), EnsureBehavior::Offline).unwrap_err();
        assert!(matches!(err, Error::Network(_)));
        assert!(err.to_string().contains("offline"));
    }

    #[test]
    fn ensure_offline_reuses_existing_local() {
        let scratch = TempDir::new().unwrap();
        let bare = seed_remote(scratch.path(), "main", &[("README.md", "hello\n")]);
        let local = scratch.path().join("local");
        let config = config_for(&bare, &local, "main");

        Marketplace::ensure(&config, scratch.path(), EnsureBehavior::Cache).unwrap();
        let outcome =
            Marketplace::ensure(&config, scratch.path(), EnsureBehavior::Offline).unwrap();
        assert_eq!(outcome, EnsureOutcome::Reused);
    }

    #[test]
    fn ensure_warns_when_refetch_fails_but_local_exists() {
        let scratch = TempDir::new().unwrap();
        let bare = seed_remote(scratch.path(), "main", &[("README.md", "v1\n")]);
        let local = scratch.path().join("local");
        let config = config_for(&bare, &local, "main");

        Marketplace::ensure(&config, scratch.path(), EnsureBehavior::Cache).unwrap();
        // Break the remote so the next fetch fails.
        fs::remove_dir_all(&bare).unwrap();

        let outcome =
            Marketplace::ensure(&config, scratch.path(), EnsureBehavior::Refetch).unwrap();
        match outcome {
            EnsureOutcome::FetchFailedReused(reason) => {
                assert!(!reason.is_empty(), "warning reason should be set");
            },
            other => panic!("expected FetchFailedReused, got {other:?}"),
        }
        // Local copy is still readable from the previous successful clone.
        assert_eq!(fs::read_to_string(local.join("README.md")).unwrap(), "v1\n");
    }
}
