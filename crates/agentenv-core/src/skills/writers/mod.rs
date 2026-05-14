//! Per-target writers: render the canonical skill model into each target's
//! native on-disk layout.
//!
//! Every target accepts the [agentskills.io](https://agentskills.io)
//! `<name>/SKILL.md` directory shape verbatim, so writers differ only in
//! **destination path** — there is no per-target transformation work. We
//! capture that uniformity in a single table + one install routine rather
//! than six near-identical files. The hooks pipeline has six diverging
//! writers because Cursor JSON, Codex TOML, etc. each need real
//! translation; for skills, structural symmetry would be wasted ceremony.
//!
//! Each write produces zero or more `.cursor/skills/<name>` (etc.) symlinks
//! pointing at the source skill directory captured in
//! [`CanonicalSkill::source_dir`]. State-link records are returned so the
//! syncer can fold them into `.agentenv/state.json` and the existing
//! stale-cleanup pass at [`crate::sync`].

use crate::error::{Error, Result};
use crate::skills::types::{Canonical, WriteReport};
use crate::state::{State, StateLink};
use crate::symlink::SymlinkManager;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// State-link `plugin` field used for skills installs. Matches the
/// `_instructions` tag the instruction-file propagation flow uses for
/// non-marketplace links — see [`crate::sync::INSTRUCTIONS_PLUGIN`].
pub const SKILLS_PLUGIN: &str = "_skills";

/// Outcome of a single writer run.
#[derive(Debug, Default, Clone)]
pub struct WriterOutcome {
    /// State entries the syncer should fold into `.agentenv/state.json`.
    pub state_links: Vec<StateLink>,
    /// Per-skill drops/warnings surfaced by this writer.
    pub report: WriteReport,
}

/// Project-relative destination root for each target's skills tree.
///
/// Paths from [docs/platform-standards.md §8](../../../../docs/platform-standards.md);
/// Codex uses the cross-tool `.agents/` alias rather than `.codex/`,
/// Antigravity uses singular `.agent/`.
fn destination_root(target: &str) -> Option<&'static str> {
    match target {
        "cursor" => Some(".cursor/skills"),
        "codex" => Some(".agents/skills"),
        "copilot" => Some(".github/skills"),
        "gemini-cli" => Some(".gemini/skills"),
        "junie" => Some(".junie/skills"),
        "antigravity" => Some(".agent/skills"),
        _ => None,
    }
}

/// Target names this module knows how to write to. Used by `Config::validate`
/// to surface "configured target has no writer" warnings.
pub fn write_targets() -> &'static [&'static str] {
    &[
        "cursor",
        "codex",
        "copilot",
        "gemini-cli",
        "junie",
        "antigravity",
    ]
}

/// Dispatch to the right writer based on the target name.
///
/// `old_state` is consulted to recognise agentenv-managed symlinks at the
/// destination — collisions with user files / foreign symlinks are
/// surfaced as warnings via [`WriteReport`] and the skill is skipped, in
/// line with the NEVER-OVERRIDE rule used by instruction-file propagation.
pub fn write(
    target: &str,
    canonical: &Canonical,
    project_root: &Path,
    old_state: &State,
) -> Result<WriterOutcome> {
    let Some(rel) = destination_root(target) else {
        return Err(Error::Config(format!(
            "skills writer for target `{target}` is not implemented in this version"
        )));
    };
    install_to_dir(canonical, project_root, rel, target, old_state)
}

fn install_to_dir(
    canonical: &Canonical,
    project_root: &Path,
    target_rel: &str,
    target_name: &str,
    old_state: &State,
) -> Result<WriterOutcome> {
    let dest_root = project_root.join(target_rel);
    let mut outcome = WriterOutcome::default();

    let managed: HashSet<&Path> = old_state.links.iter().map(|l| l.target.as_path()).collect();

    for skill in &canonical.skills {
        // `source_dir` is `#[serde(skip)]`, so it deserializes to an empty
        // `PathBuf` from disk. Production sync always re-runs the reader
        // before invoking writers (see `crate::skills::pipeline`), so this
        // branch is only reachable when a caller hands us a canonical built
        // without a reader (today: tests). Treat it as a defensive guard.
        if skill.source_dir.as_os_str().is_empty() {
            outcome.report.drops.push(format!(
                "{target_name}: skill `{}` has no source_dir captured — skipping (likely an orphaned canonical entry)",
                skill.name
            ));
            continue;
        }
        let dest = dest_root.join(&skill.name);
        match check_conflict(&dest, &managed) {
            Ok(true) => {
                // Agentenv-managed symlink at the destination — drop it
                // before re-creating so a moved source repoints cleanly.
                SymlinkManager::remove(&dest)?;
            },
            Ok(false) => {},
            Err(reason) => {
                outcome.report.drops.push(reason);
                continue;
            },
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        SymlinkManager::create_symlink(&skill.source_dir, &dest)?;
        outcome.state_links.push(StateLink {
            source: skill.source_dir.clone(),
            target: dest,
            tool: target_name.to_string(),
            mode: "symlink".to_string(),
            plugin: SKILLS_PLUGIN.to_string(),
        });
    }

    Ok(outcome)
}

/// Classify a destination path.
///
/// Returns:
/// - `Ok(false)` — destination does not exist; safe to create fresh.
/// - `Ok(true)`  — destination is an agentenv-managed symlink; safe to
///   remove and re-create (caller does so).
/// - `Err(reason)` — destination holds a user file or foreign symlink;
///   skipped with a warning.
fn check_conflict(dest: &PathBuf, managed: &HashSet<&Path>) -> std::result::Result<bool, String> {
    let meta = match fs::symlink_metadata(dest) {
        Ok(m) => m,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(format!("cannot stat {}: {err}", dest.display())),
    };
    let is_managed = managed.contains(dest.as_path());
    if is_managed {
        return Ok(true);
    }
    if meta.file_type().is_symlink() {
        return Err(format!(
            "{}: existing symlink is not agentenv-managed — refusing to overwrite",
            dest.display()
        ));
    }
    Err(format!(
        "{}: a real file/dir already exists — refusing to overwrite",
        dest.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::CanonicalSkill;
    use tempfile::TempDir;

    fn make_skill(source_dir: &Path, name: &str) -> CanonicalSkill {
        CanonicalSkill {
            name: name.to_string(),
            frontmatter: serde_yaml::Mapping::new(),
            body: String::new(),
            sidecars: vec![],
            source_dir: source_dir.to_path_buf(),
        }
    }

    fn populate_source_skill(source_root: &Path, name: &str) -> PathBuf {
        let dir = source_root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "---\nname: x\n---\nbody").unwrap();
        dir
    }

    #[test]
    fn write_creates_symlink_per_skill_for_each_target() {
        for target in write_targets() {
            let project = TempDir::new().unwrap();
            let source_root = project.path().join(".claude/skills");
            let skill_dir = populate_source_skill(&source_root, "hello");
            let canonical = Canonical {
                source: "claude-code".to_string(),
                skills: vec![make_skill(&skill_dir, "hello")],
            };
            let outcome = write(target, &canonical, project.path(), &State::default()).unwrap();
            assert!(
                outcome.report.drops.is_empty(),
                "{target} drops: {:?}",
                outcome.report.drops
            );
            assert_eq!(outcome.state_links.len(), 1, "target={target}");

            let rel = destination_root(target).unwrap();
            let dest = project.path().join(rel).join("hello");
            assert!(dest.is_symlink(), "missing {}", dest.display());
            assert_eq!(fs::read_link(&dest).unwrap(), skill_dir);
        }
    }

    #[test]
    fn unknown_target_is_a_config_error() {
        let project = TempDir::new().unwrap();
        let canonical = Canonical {
            source: "claude-code".to_string(),
            skills: vec![],
        };
        let err = write("not-real", &canonical, project.path(), &State::default()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not-real"), "got: {msg}");
        assert!(msg.contains("not implemented"), "got: {msg}");
    }

    #[test]
    fn refuses_when_destination_is_user_file() {
        let project = TempDir::new().unwrap();
        let source_root = project.path().join(".claude/skills");
        let skill_dir = populate_source_skill(&source_root, "hello");
        let canonical = Canonical {
            source: "claude-code".to_string(),
            skills: vec![make_skill(&skill_dir, "hello")],
        };
        // Pre-create a regular file where the symlink would go.
        let dest = project.path().join(".cursor/skills/hello");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(&dest, "user content").unwrap();

        let outcome = write("cursor", &canonical, project.path(), &State::default()).unwrap();
        assert_eq!(outcome.state_links.len(), 0);
        assert_eq!(outcome.report.drops.len(), 1);
        assert!(outcome.report.drops[0].contains("real file/dir"));
        // User file preserved.
        assert!(!dest.is_symlink());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "user content");
    }

    #[test]
    #[cfg(unix)]
    fn refuses_when_destination_is_foreign_symlink() {
        let project = TempDir::new().unwrap();
        let source_root = project.path().join(".claude/skills");
        let skill_dir = populate_source_skill(&source_root, "hello");
        let canonical = Canonical {
            source: "claude-code".to_string(),
            skills: vec![make_skill(&skill_dir, "hello")],
        };
        let dest = project.path().join(".cursor/skills/hello");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let foreign_target = project.path().join("elsewhere");
        fs::create_dir_all(&foreign_target).unwrap();
        std::os::unix::fs::symlink(&foreign_target, &dest).unwrap();

        let outcome = write("cursor", &canonical, project.path(), &State::default()).unwrap();
        assert_eq!(outcome.state_links.len(), 0);
        assert_eq!(outcome.report.drops.len(), 1);
        assert!(outcome.report.drops[0].contains("not agentenv-managed"));
        assert_eq!(fs::read_link(&dest).unwrap(), foreign_target);
    }

    #[test]
    #[cfg(unix)]
    fn replaces_agentenv_managed_symlink() {
        let project = TempDir::new().unwrap();
        let source_root = project.path().join(".claude/skills");
        let skill_dir = populate_source_skill(&source_root, "hello");
        let canonical = Canonical {
            source: "claude-code".to_string(),
            skills: vec![make_skill(&skill_dir, "hello")],
        };
        let dest = project.path().join(".cursor/skills/hello");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        // Pre-existing symlink to a stale source — but it IS in old_state,
        // so the writer should overwrite without complaint.
        let stale_source = project.path().join(".old-claude/skills/hello");
        fs::create_dir_all(&stale_source).unwrap();
        std::os::unix::fs::symlink(&stale_source, &dest).unwrap();

        let mut old_state = State::default();
        old_state.links.push(StateLink {
            source: stale_source.clone(),
            target: dest.clone(),
            tool: "cursor".to_string(),
            mode: "symlink".to_string(),
            plugin: SKILLS_PLUGIN.to_string(),
        });

        let outcome = write("cursor", &canonical, project.path(), &old_state).unwrap();
        assert!(
            outcome.report.drops.is_empty(),
            "drops: {:?}",
            outcome.report.drops
        );
        assert_eq!(outcome.state_links.len(), 1);
        assert_eq!(fs::read_link(&dest).unwrap(), skill_dir);
    }

    #[test]
    fn empty_source_dir_is_skipped_with_warning() {
        let project = TempDir::new().unwrap();
        let canonical = Canonical {
            source: "claude-code".to_string(),
            skills: vec![CanonicalSkill {
                name: "ghost".to_string(),
                frontmatter: serde_yaml::Mapping::new(),
                body: String::new(),
                sidecars: vec![],
                source_dir: PathBuf::new(),
            }],
        };
        let outcome = write("cursor", &canonical, project.path(), &State::default()).unwrap();
        assert_eq!(outcome.state_links.len(), 0);
        assert_eq!(outcome.report.drops.len(), 1);
        assert!(outcome.report.drops[0].contains("ghost"));
    }
}
