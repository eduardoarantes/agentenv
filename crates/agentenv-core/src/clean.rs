//! Remove agentenv-managed links from a project.
//!
//! `Cleaner::clean` reads `<project>/.agentenv/state.json`, removes every
//! link recorded there (defensively — only when the link still points at the
//! recorded source), and deletes the state file. Files outside the manifest
//! are never touched.

use crate::error::Result;
use crate::state::{State, StateLink};
use crate::sync::remove_managed_link;
use std::path::Path;

/// Clean entry point.
#[derive(Debug)]
pub struct Cleaner;

/// Outcome of a clean run.
#[derive(Debug, Default)]
pub struct CleanReport {
    /// Links that were removed (or already absent).
    pub removed: Vec<StateLink>,
    /// Links left alone because they were no longer managed-shaped, with the
    /// reason captured for printing.
    pub skipped: Vec<(StateLink, String)>,
}

impl CleanReport {
    /// Total number of state entries processed.
    pub fn total(&self) -> usize {
        self.removed.len() + self.skipped.len()
    }
}

impl Cleaner {
    /// Remove every managed link from the state file at `project_root`, then
    /// delete the state file itself. Idempotent: running twice is a no-op the
    /// second time.
    ///
    /// # Errors
    ///
    /// Returns the IO error from a fatal removal failure or from deleting the
    /// state file. Per-link "user replaced this with their own content"
    /// outcomes are reported as `skipped`, not errors.
    pub fn clean(project_root: &Path) -> Result<CleanReport> {
        let state = State::load(project_root)?;
        let mut report = CleanReport::default();

        for link in &state.links {
            match remove_managed_link(link) {
                Ok(true) => report.removed.push(link.clone()),
                Ok(false) => report
                    .skipped
                    .push((link.clone(), "modified outside agentenv".to_string())),
                Err(err) => report.skipped.push((link.clone(), err.to_string())),
            }
        }

        State::remove(project_root)?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_link(target: PathBuf, source: PathBuf) -> StateLink {
        StateLink {
            source,
            target,
            tool: "claude-code".to_string(),
            mode: "symlink".to_string(),
            plugin: "demo".to_string(),
        }
    }

    fn install_link(source: &Path, target: &Path) {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        std::os::unix::fs::symlink(source, target).unwrap();
    }

    #[test]
    fn clean_with_no_state_is_a_noop() {
        let project = TempDir::new().unwrap();
        let report = Cleaner::clean(project.path()).unwrap();
        assert!(report.removed.is_empty());
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn clean_removes_recorded_symlinks() {
        let project = TempDir::new().unwrap();
        let source_root = TempDir::new().unwrap();
        let source = source_root.path().join("skill");
        fs::create_dir_all(&source).unwrap();
        let target = project.path().join(".claude/skills/demo");
        install_link(&source, &target);

        let mut state = State::default();
        state.links.push(make_link(target.clone(), source.clone()));
        state.save(project.path()).unwrap();

        let report = Cleaner::clean(project.path()).unwrap();
        assert_eq!(report.removed.len(), 1);
        assert!(report.skipped.is_empty());
        assert!(!target.exists() && !target.is_symlink());
        assert!(!State::path(project.path()).exists());
    }

    #[test]
    fn clean_skips_links_replaced_by_user() {
        let project = TempDir::new().unwrap();
        let source_root = TempDir::new().unwrap();
        let source = source_root.path().join("skill");
        fs::create_dir_all(&source).unwrap();
        let target = project.path().join(".claude/skills/demo");
        install_link(&source, &target);

        // User replaces the symlink with one pointing somewhere else.
        let other = source_root.path().join("other");
        fs::create_dir_all(&other).unwrap();
        fs::remove_file(&target).unwrap();
        std::os::unix::fs::symlink(&other, &target).unwrap();

        let mut state = State::default();
        state.links.push(make_link(target.clone(), source.clone()));
        state.save(project.path()).unwrap();

        let report = Cleaner::clean(project.path()).unwrap();
        assert!(report.removed.is_empty());
        assert_eq!(report.skipped.len(), 1);
        // The user's replacement link is left alone.
        assert!(target.is_symlink());
    }

    #[test]
    fn clean_handles_already_removed_links_as_removed() {
        let project = TempDir::new().unwrap();
        let target = project.path().join(".claude/skills/demo");

        let mut state = State::default();
        state
            .links
            .push(make_link(target, PathBuf::from("/does/not/matter")));
        state.save(project.path()).unwrap();

        let report = Cleaner::clean(project.path()).unwrap();
        assert_eq!(report.removed.len(), 1);
    }
}
