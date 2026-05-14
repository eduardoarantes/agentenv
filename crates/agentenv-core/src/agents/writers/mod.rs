//! Per-target writers for agents.
//!
//! Five flavors:
//!
//! 1. **Passthrough symlink** (cursor, gemini-cli, junie) — destination is
//!    `<target>/agents/<name>.md` pointing at the canonical's `source_file`.
//!    No content rewrite; the Markdown is identical across these tools.
//! 2. **Renamed symlink** (copilot) — destination uses the `.agent.md`
//!    suffix; everything else is a passthrough.
//! 3. **Materialize-to-TOML** (codex) — Markdown frontmatter is translated
//!    to a documented TOML schema and the body becomes `prompt = "…"`.
//!    Refuse-on-conflict via a sentinel comment.
//! 4. **Skip-with-warning** (antigravity) — Antigravity uses a single
//!    repo-root `agents.md`, not per-agent files. We emit one warning per
//!    canonical agent so the user knows nothing was installed.
//! 5. **Source** (claude-code) — never written; the source is read-only.
//!
//! Conflict handling matches the skills writers: warn-and-skip per agent
//! for the symlink writers (NEVER-OVERRIDE rule), hard-fail for the
//! materialized Codex writer (matches `hooks::writers::codex`).

pub mod codex;

use crate::agents::types::{Canonical, WriteReport};
use crate::error::{Error, Result};
use crate::state::{State, StateLink};
use crate::symlink::SymlinkManager;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// State-link `plugin` field used for agents installs. Parallels
/// [`crate::skills::writers::SKILLS_PLUGIN`].
pub const AGENTS_PLUGIN: &str = "_agents";

/// Outcome of a single writer run.
#[derive(Debug, Default, Clone)]
pub struct WriterOutcome {
    /// State entries the syncer should fold into `.agentenv/state.json`.
    pub state_links: Vec<StateLink>,
    /// Per-agent drops/warnings surfaced by this writer.
    pub report: WriteReport,
}

/// Target names this module knows how to write to.
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
pub fn write(
    target: &str,
    canonical: &Canonical,
    project_root: &Path,
    old_state: &State,
) -> Result<WriterOutcome> {
    match target {
        "cursor" => install_symlinks(canonical, project_root, ".cursor/agents", ".md", "cursor", old_state),
        "copilot" => install_symlinks(
            canonical,
            project_root,
            ".github/agents",
            ".agent.md",
            "copilot",
            old_state,
        ),
        "gemini-cli" => install_symlinks(
            canonical,
            project_root,
            ".gemini/agents",
            ".md",
            "gemini-cli",
            old_state,
        ),
        "junie" => install_symlinks(
            canonical,
            project_root,
            ".junie/agents",
            ".md",
            "junie",
            old_state,
        ),
        "codex" => codex::write(canonical, project_root, old_state),
        "antigravity" => Ok(skip_all(canonical, "antigravity",
            "antigravity uses a single repo-root `agents.md` file, not per-agent files — agent skipped",
        )),
        other => Err(Error::Config(format!(
            "agents writer for target `{other}` is not implemented in this version"
        ))),
    }
}

fn install_symlinks(
    canonical: &Canonical,
    project_root: &Path,
    target_dir_rel: &str,
    filename_suffix: &str,
    target_name: &str,
    old_state: &State,
) -> Result<WriterOutcome> {
    let dest_root = project_root.join(target_dir_rel);
    let mut outcome = WriterOutcome::default();

    let managed: HashSet<&Path> = old_state.links.iter().map(|l| l.target.as_path()).collect();

    for agent in &canonical.agents {
        // `source_file` is `#[serde(skip)]` — empty after deserializing from
        // disk. Production sync always re-runs the reader before invoking
        // writers (see `crate::agents::pipeline`), so this is only reachable
        // from callers that hand us a canonical built without a reader
        // (today: tests). Defensive guard.
        if agent.source_file.as_os_str().is_empty() {
            outcome.report.drops.push(format!(
                "{target_name}: agent `{}` has no source_file captured — skipping",
                agent.name
            ));
            continue;
        }
        let dest = dest_root.join(format!("{}{filename_suffix}", agent.name));
        match check_conflict(&dest, &managed) {
            Ok(true) => SymlinkManager::remove(&dest)?,
            Ok(false) => {},
            Err(reason) => {
                outcome.report.drops.push(reason);
                continue;
            },
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        SymlinkManager::create_symlink(&agent.source_file, &dest)?;
        outcome.state_links.push(StateLink {
            source: agent.source_file.clone(),
            target: dest,
            tool: target_name.to_string(),
            mode: "symlink".to_string(),
            plugin: AGENTS_PLUGIN.to_string(),
        });
    }

    Ok(outcome)
}

/// Build a `WriterOutcome` with one drop entry per canonical agent and no
/// state links — used for targets that cannot represent per-agent
/// definitions (Antigravity).
fn skip_all(canonical: &Canonical, target_name: &str, reason: &str) -> WriterOutcome {
    let mut outcome = WriterOutcome::default();
    for agent in &canonical.agents {
        outcome.report.drops.push(format!(
            "{target_name}: dropping `{}` — {reason}",
            agent.name
        ));
    }
    outcome
}

/// Classify a destination path — see [`crate::skills::writers`] for the
/// same logic applied to skills.
fn check_conflict(dest: &PathBuf, managed: &HashSet<&Path>) -> std::result::Result<bool, String> {
    let meta = match fs::symlink_metadata(dest) {
        Ok(m) => m,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(format!("cannot stat {}: {err}", dest.display())),
    };
    if managed.contains(dest.as_path()) {
        return Ok(true);
    }
    if meta.file_type().is_symlink() {
        return Err(format!(
            "{}: existing symlink is not agentenv-managed — refusing to overwrite",
            dest.display()
        ));
    }
    Err(format!(
        "{}: a real file already exists — refusing to overwrite",
        dest.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::types::CanonicalAgent;
    use tempfile::TempDir;

    fn make_agent(source: &Path, name: &str) -> CanonicalAgent {
        CanonicalAgent {
            name: name.to_string(),
            frontmatter: serde_yaml::Mapping::new(),
            body: "body".to_string(),
            source_file: source.to_path_buf(),
        }
    }

    fn populate_source(project: &Path, name: &str) -> PathBuf {
        let dir = project.join(".claude/agents");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{name}.md"));
        fs::write(&file, "---\nname: x\n---\nbody").unwrap();
        file
    }

    #[test]
    fn cursor_symlinks_md_passthrough() {
        let project = TempDir::new().unwrap();
        let src = populate_source(project.path(), "rev");
        let canonical = Canonical {
            source: "claude-code".to_string(),
            agents: vec![make_agent(&src, "rev")],
        };
        let outcome = write("cursor", &canonical, project.path(), &State::default()).unwrap();
        assert!(outcome.report.drops.is_empty());
        assert_eq!(outcome.state_links.len(), 1);
        let dest = project.path().join(".cursor/agents/rev.md");
        assert!(dest.is_symlink());
        assert_eq!(fs::read_link(&dest).unwrap(), src);
    }

    #[test]
    fn copilot_uses_agent_md_suffix() {
        let project = TempDir::new().unwrap();
        let src = populate_source(project.path(), "rev");
        let canonical = Canonical {
            source: "claude-code".to_string(),
            agents: vec![make_agent(&src, "rev")],
        };
        let outcome = write("copilot", &canonical, project.path(), &State::default()).unwrap();
        assert!(outcome.report.drops.is_empty());
        let dest = project.path().join(".github/agents/rev.agent.md");
        assert!(dest.is_symlink(), "expected {}", dest.display());
        assert!(!project.path().join(".github/agents/rev.md").exists());
    }

    #[test]
    fn gemini_and_junie_passthrough_md() {
        for (target, rel) in [
            ("gemini-cli", ".gemini/agents/rev.md"),
            ("junie", ".junie/agents/rev.md"),
        ] {
            let project = TempDir::new().unwrap();
            let src = populate_source(project.path(), "rev");
            let canonical = Canonical {
                source: "claude-code".to_string(),
                agents: vec![make_agent(&src, "rev")],
            };
            let outcome = write(target, &canonical, project.path(), &State::default()).unwrap();
            assert!(
                outcome.report.drops.is_empty(),
                "{target}: {:?}",
                outcome.report.drops
            );
            assert!(project.path().join(rel).is_symlink());
        }
    }

    #[test]
    fn antigravity_drops_every_agent_with_warning() {
        let project = TempDir::new().unwrap();
        let src = populate_source(project.path(), "rev");
        let canonical = Canonical {
            source: "claude-code".to_string(),
            agents: vec![make_agent(&src, "rev"), make_agent(&src, "two")],
        };
        let outcome = write("antigravity", &canonical, project.path(), &State::default()).unwrap();
        assert_eq!(outcome.state_links.len(), 0);
        assert_eq!(outcome.report.drops.len(), 2);
        assert!(outcome.report.drops[0].contains("antigravity"));
        assert!(outcome.report.drops[0].contains("rev"));
    }

    #[test]
    fn unknown_target_is_a_config_error() {
        let project = TempDir::new().unwrap();
        let canonical = Canonical {
            source: "claude-code".to_string(),
            agents: vec![],
        };
        let err = write("not-real", &canonical, project.path(), &State::default()).unwrap_err();
        assert!(err.to_string().contains("not-real"));
    }

    #[test]
    fn cursor_refuses_user_file_at_destination() {
        let project = TempDir::new().unwrap();
        let src = populate_source(project.path(), "rev");
        let dest = project.path().join(".cursor/agents/rev.md");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(&dest, "user wrote this").unwrap();
        let canonical = Canonical {
            source: "claude-code".to_string(),
            agents: vec![make_agent(&src, "rev")],
        };
        let outcome = write("cursor", &canonical, project.path(), &State::default()).unwrap();
        assert_eq!(outcome.state_links.len(), 0);
        assert_eq!(outcome.report.drops.len(), 1);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "user wrote this");
    }

    #[test]
    #[cfg(unix)]
    fn cursor_replaces_agentenv_managed_symlink() {
        let project = TempDir::new().unwrap();
        let src = populate_source(project.path(), "rev");
        let dest = project.path().join(".cursor/agents/rev.md");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        // Stale managed link pointing somewhere else.
        let stale = project.path().join(".old/rev.md");
        fs::create_dir_all(stale.parent().unwrap()).unwrap();
        fs::write(&stale, "stale").unwrap();
        std::os::unix::fs::symlink(&stale, &dest).unwrap();

        let mut state = State::default();
        state.links.push(StateLink {
            source: stale,
            target: dest.clone(),
            tool: "cursor".to_string(),
            mode: "symlink".to_string(),
            plugin: AGENTS_PLUGIN.to_string(),
        });

        let canonical = Canonical {
            source: "claude-code".to_string(),
            agents: vec![make_agent(&src, "rev")],
        };
        let outcome = write("cursor", &canonical, project.path(), &state).unwrap();
        assert!(outcome.report.drops.is_empty());
        assert_eq!(outcome.state_links.len(), 1);
        assert_eq!(fs::read_link(&dest).unwrap(), src);
    }
}
