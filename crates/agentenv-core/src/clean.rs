//! Remove agentenv-managed links from a project.
//!
//! `Cleaner::clean` reads `<project>/.agentenv/state.json`, removes every
//! link recorded there (defensively — only when the link still points at the
//! recorded source), and deletes the state file. Files outside the manifest
//! are never touched.

use crate::error::Result;
use crate::state::{State, StateLink};
use crate::sync::remove_managed_link;
use std::fs;
use std::path::{Path, PathBuf};

/// Clean entry point.
#[derive(Debug)]
pub struct Cleaner;

/// Knobs that control a clean run.
#[derive(Debug, Clone, Copy)]
pub struct CleanOptions {
    /// After removing managed links, walk each link's ancestor directories
    /// inside the project root and remove any that are now empty. Stops at
    /// the project root itself.
    pub prune_empty_dirs: bool,
}

impl Default for CleanOptions {
    fn default() -> Self {
        Self {
            prune_empty_dirs: true,
        }
    }
}

/// Outcome of a clean run.
#[derive(Debug, Default)]
pub struct CleanReport {
    /// Links that were removed (or already absent).
    pub removed: Vec<StateLink>,
    /// Links left alone because they were no longer managed-shaped, with the
    /// reason captured for printing.
    pub skipped: Vec<(StateLink, String)>,
    /// Empty directories pruned after link removal. Empty when
    /// `CleanOptions::prune_empty_dirs` is false.
    pub pruned_dirs: Vec<PathBuf>,
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
    pub fn clean(project_root: &Path, options: CleanOptions) -> Result<CleanReport> {
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

        if options.prune_empty_dirs {
            report.pruned_dirs = prune_empty_ancestors(project_root, &report.removed);
        }

        State::remove(project_root)?;
        Ok(report)
    }
}

/// For each removed link, walk parent directories upward and remove any that
/// are empty, stopping at `project_root`. Returns the directories actually
/// removed, in removal order.
///
/// Best-effort: any IO error simply stops walking that branch — no error is
/// surfaced. Directories outside `project_root` are never touched.
fn prune_empty_ancestors(project_root: &Path, removed: &[StateLink]) -> Vec<PathBuf> {
    let mut pruned = Vec::new();

    for link in removed {
        if !link.target.starts_with(project_root) {
            continue;
        }
        let mut current = link.target.parent();
        while let Some(dir) = current {
            if dir == project_root || !dir.starts_with(project_root) {
                break;
            }
            // A directory that already disappeared in an earlier pass returns
            // an Err here; we just stop walking that branch — no double-count.
            let is_empty = match fs::read_dir(dir) {
                Ok(mut iter) => iter.next().is_none(),
                Err(_) => false,
            };
            if !is_empty {
                break;
            }
            if fs::remove_dir(dir).is_err() {
                break;
            }
            pruned.push(dir.to_path_buf());
            current = dir.parent();
        }
    }

    pruned
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
        crate::symlink::SymlinkManager::create_symlink(source, target).unwrap();
    }

    #[test]
    fn clean_with_no_state_is_a_noop() {
        let project = TempDir::new().unwrap();
        let report = Cleaner::clean(project.path(), CleanOptions::default()).unwrap();
        assert!(report.removed.is_empty());
        assert!(report.skipped.is_empty());
        assert!(report.pruned_dirs.is_empty());
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

        let report = Cleaner::clean(project.path(), CleanOptions::default()).unwrap();
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
        crate::symlink::SymlinkManager::remove(&target).unwrap();
        crate::symlink::SymlinkManager::create_symlink(&other, &target).unwrap();

        let mut state = State::default();
        state.links.push(make_link(target.clone(), source.clone()));
        state.save(project.path()).unwrap();

        let report = Cleaner::clean(project.path(), CleanOptions::default()).unwrap();
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

        let report = Cleaner::clean(project.path(), CleanOptions::default()).unwrap();
        assert_eq!(report.removed.len(), 1);
    }

    #[test]
    fn clean_prunes_empty_ancestor_dirs_by_default() {
        let project = TempDir::new().unwrap();
        let source_root = TempDir::new().unwrap();
        let source = source_root.path().join("skill");
        fs::create_dir_all(&source).unwrap();

        let target = project.path().join(".claude/skills/demo");
        install_link(&source, &target);

        let mut state = State::default();
        state.links.push(make_link(target.clone(), source));
        state.save(project.path()).unwrap();

        let report = Cleaner::clean(project.path(), CleanOptions::default()).unwrap();
        assert_eq!(report.removed.len(), 1);
        // Both the immediate parent and its parent should be gone.
        assert!(!project.path().join(".claude/skills").exists());
        assert!(!project.path().join(".claude").exists());
        // Project root itself is untouched.
        assert!(project.path().exists());
        // Pruned dirs are reported, deepest first.
        assert_eq!(report.pruned_dirs.len(), 2);
        assert!(report
            .pruned_dirs
            .contains(&project.path().join(".claude/skills")));
        assert!(report.pruned_dirs.contains(&project.path().join(".claude")));
    }

    #[test]
    fn clean_does_not_prune_when_disabled() {
        let project = TempDir::new().unwrap();
        let source_root = TempDir::new().unwrap();
        let source = source_root.path().join("skill");
        fs::create_dir_all(&source).unwrap();

        let target = project.path().join(".claude/skills/demo");
        install_link(&source, &target);

        let mut state = State::default();
        state.links.push(make_link(target.clone(), source));
        state.save(project.path()).unwrap();

        let report = Cleaner::clean(
            project.path(),
            CleanOptions {
                prune_empty_dirs: false,
            },
        )
        .unwrap();
        assert_eq!(report.removed.len(), 1);
        // Empty parents are left alone.
        assert!(project.path().join(".claude/skills").exists());
        assert!(project.path().join(".claude").exists());
        assert!(report.pruned_dirs.is_empty());
    }

    #[test]
    fn clean_prune_keeps_dirs_with_other_files() {
        let project = TempDir::new().unwrap();
        let source_root = TempDir::new().unwrap();
        let source = source_root.path().join("skill");
        fs::create_dir_all(&source).unwrap();

        let target = project.path().join(".claude/skills/demo");
        install_link(&source, &target);
        // Drop a user file alongside the managed link.
        fs::write(project.path().join(".claude/skills/user-note.md"), "hi").unwrap();

        let mut state = State::default();
        state.links.push(make_link(target.clone(), source));
        state.save(project.path()).unwrap();

        let report = Cleaner::clean(project.path(), CleanOptions::default()).unwrap();
        assert_eq!(report.removed.len(), 1);
        // .claude/skills still contains the user's file, so it stays.
        assert!(project.path().join(".claude/skills").exists());
        assert!(project.path().join(".claude").exists());
        assert!(project.path().join(".claude/skills/user-note.md").exists());
        assert!(report.pruned_dirs.is_empty());
    }

    #[test]
    fn clean_prune_walks_up_after_sibling_dir_emptied() {
        let project = TempDir::new().unwrap();
        let source_root = TempDir::new().unwrap();
        let source_a = source_root.path().join("a");
        let source_b = source_root.path().join("b");
        fs::create_dir_all(&source_a).unwrap();
        fs::create_dir_all(&source_b).unwrap();

        let target_a = project.path().join(".claude/skills/demo");
        let target_b = project.path().join(".claude/agents/demo");
        install_link(&source_a, &target_a);
        install_link(&source_b, &target_b);

        let mut state = State::default();
        state
            .links
            .push(make_link(target_a.clone(), source_a.clone()));
        state
            .links
            .push(make_link(target_b.clone(), source_b.clone()));
        state.save(project.path()).unwrap();

        let report = Cleaner::clean(project.path(), CleanOptions::default()).unwrap();
        assert_eq!(report.removed.len(), 2);
        // Both per-capability dirs and the shared parent should be pruned.
        assert!(!project.path().join(".claude/skills").exists());
        assert!(!project.path().join(".claude/agents").exists());
        assert!(!project.path().join(".claude").exists());
        assert!(report.pruned_dirs.contains(&project.path().join(".claude")));
    }

    #[test]
    fn clean_prune_never_removes_project_root() {
        let project = TempDir::new().unwrap();
        let source_root = TempDir::new().unwrap();
        let source = source_root.path().join("skill");
        fs::create_dir_all(&source).unwrap();

        // Link directly inside the project root, no intermediate dirs.
        let target = project.path().join("demo");
        install_link(&source, &target);

        let mut state = State::default();
        state.links.push(make_link(target.clone(), source));
        state.save(project.path()).unwrap();

        let report = Cleaner::clean(project.path(), CleanOptions::default()).unwrap();
        assert_eq!(report.removed.len(), 1);
        // Project root must still exist and pruning must not have walked above it.
        assert!(project.path().exists());
        assert!(report.pruned_dirs.is_empty());
    }
}
