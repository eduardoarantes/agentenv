//! Read Claude Code-shaped skills into the canonical model.
//!
//! Walks every immediate child of each given root, expecting the
//! [agentskills.io](https://agentskills.io) layout:
//!
//! ```text
//! <root>/
//!   <skill-name>/
//!     SKILL.md          ← required: YAML frontmatter + Markdown body
//!     scripts/, references/, assets/, …  (optional sidecars)
//! ```
//!
//! Roots are project-local (`<project>/.claude/skills`) and plugin-local
//! (`<plugin>/skills`). The reader is shape-agnostic: it produces one
//! [`CanonicalSkill`] per `<skill-name>` directory found under any root.
//!
//! Hidden entries (names starting with `.`) and entries without a
//! `SKILL.md` are skipped silently — same convention as the per-leaf
//! walker in `sync.rs` used to apply to `.gitkeep` files.

use crate::error::{Error, Result};
use crate::skills::types::{Canonical, CanonicalSkill, SidecarFile, SidecarKind};
use std::fs;
use std::path::Path;

const SOURCE_NAME: &str = "claude-code";
const SKILL_FILENAME: &str = "SKILL.md";

/// Build the canonical from a set of skill roots.
///
/// Returns `Ok(None)` when no skill directory was found across any root,
/// so callers can short-circuit cleanly on a fresh project. Skill names are
/// deduplicated by first-wins (project root before plugin roots, plugin
/// roots in the order supplied).
pub fn read(roots: &[&Path]) -> Result<Option<Canonical>> {
    let mut skills: Vec<CanonicalSkill> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in walk_sorted(root)? {
            let leaf_name = entry.file_name();
            let leaf_str = match leaf_name.to_str() {
                Some(s) if !s.starts_with('.') => s.to_string(),
                _ => continue,
            };
            let leaf_path = entry.path();
            if !leaf_path.is_dir() {
                continue;
            }
            let skill_md = leaf_path.join(SKILL_FILENAME);
            if !skill_md.is_file() {
                continue;
            }
            if !seen.insert(leaf_str.clone()) {
                continue;
            }
            let parsed = parse_skill(&leaf_str, &leaf_path)?;
            skills.push(parsed);
        }
    }

    if skills.is_empty() {
        return Ok(None);
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Some(Canonical {
        source: SOURCE_NAME.to_string(),
        skills,
    }))
}

fn walk_sorted(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    Ok(entries)
}

fn parse_skill(name: &str, dir: &Path) -> Result<CanonicalSkill> {
    let raw = fs::read_to_string(dir.join(SKILL_FILENAME))?;
    let (frontmatter, body) = crate::frontmatter::split(&raw)
        .map_err(|err| Error::Config(format!("invalid SKILL.md at {}: {err}", dir.display())))?;
    let sidecars = collect_sidecars(dir)?;
    Ok(CanonicalSkill {
        name: name.to_string(),
        frontmatter,
        body,
        sidecars,
        source_dir: dir.to_path_buf(),
    })
}

/// Walk the skill directory and collect everything except `SKILL.md` itself.
fn collect_sidecars(dir: &Path) -> Result<Vec<SidecarFile>> {
    let mut out = Vec::new();
    walk_sidecars(dir, dir, &mut out)?;
    out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(out)
}

fn walk_sidecars(root: &Path, current: &Path, out: &mut Vec<SidecarFile>) -> Result<()> {
    for entry in walk_sorted(current)? {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk_sidecars(root, &path, out)?;
            continue;
        }
        if !ft.is_file() && !ft.is_symlink() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|err| Error::Config(format!("sidecar path {}: {err}", path.display())))?
            .to_path_buf();
        if relative.as_os_str() == SKILL_FILENAME {
            continue;
        }
        let kind = SidecarKind::from_relative_path(&relative);
        out.push(SidecarFile {
            relative_path: relative,
            kind,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill(root: &Path, name: &str, frontmatter: &str, body: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        let content = format!("---\n{frontmatter}\n---\n{body}");
        fs::write(dir.join(SKILL_FILENAME), content).unwrap();
    }

    #[test]
    fn returns_none_when_no_roots_exist() {
        let scratch = TempDir::new().unwrap();
        let missing = scratch.path().join("does-not-exist");
        assert!(read(&[&missing]).unwrap().is_none());
    }

    #[test]
    fn returns_none_when_root_has_no_skill_dirs() {
        let scratch = TempDir::new().unwrap();
        fs::create_dir_all(scratch.path().join("empty")).unwrap();
        // A `.gitkeep` is fine and ignored.
        fs::write(scratch.path().join(".gitkeep"), "").unwrap();
        assert!(read(&[scratch.path()]).unwrap().is_none());
    }

    #[test]
    fn parses_skill_with_frontmatter_and_body() {
        let scratch = TempDir::new().unwrap();
        write_skill(
            scratch.path(),
            "hello",
            "name: hello\ndescription: Sample",
            "# Hello body\nLine two\n",
        );
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(canonical.source, "claude-code");
        assert_eq!(canonical.skills.len(), 1);
        let s = &canonical.skills[0];
        assert_eq!(s.name, "hello");
        assert_eq!(s.body, "# Hello body\nLine two\n");
        let name = s
            .frontmatter
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|v| v.as_str());
        assert_eq!(name, Some("hello"));
    }

    #[test]
    fn handles_skill_with_no_frontmatter() {
        let scratch = TempDir::new().unwrap();
        let dir = scratch.path().join("plain");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(SKILL_FILENAME), "Just a body, no fences\n").unwrap();
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert!(canonical.skills[0].frontmatter.is_empty());
        assert_eq!(canonical.skills[0].body, "Just a body, no fences\n");
    }

    #[test]
    fn skips_directories_without_skill_md() {
        let scratch = TempDir::new().unwrap();
        fs::create_dir_all(scratch.path().join("not-a-skill")).unwrap();
        write_skill(scratch.path(), "real", "name: real\ndescription: x", "body");
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        let names: Vec<_> = canonical.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn collects_sidecar_files_recursively() {
        let scratch = TempDir::new().unwrap();
        write_skill(scratch.path(), "rich", "name: rich\ndescription: x", "body");
        let dir = scratch.path().join("rich");
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::create_dir_all(dir.join("references")).unwrap();
        fs::write(dir.join("scripts/run.sh"), "#!/bin/sh").unwrap();
        fs::write(dir.join("references/api.md"), "docs").unwrap();
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        let sidecars = &canonical.skills[0].sidecars;
        let paths: Vec<_> = sidecars
            .iter()
            .map(|s| s.relative_path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(paths, vec!["references/api.md", "scripts/run.sh"]);
        assert_eq!(sidecars[0].kind, SidecarKind::Reference);
        assert_eq!(sidecars[1].kind, SidecarKind::Script);
    }

    #[test]
    fn first_root_wins_on_skill_name_collision() {
        let project = TempDir::new().unwrap();
        let plugin = TempDir::new().unwrap();
        write_skill(
            project.path(),
            "dup",
            "name: dup\ndescription: project",
            "p-body",
        );
        write_skill(
            plugin.path(),
            "dup",
            "name: dup\ndescription: plugin",
            "x-body",
        );
        let canonical = read(&[project.path(), plugin.path()]).unwrap().unwrap();
        assert_eq!(canonical.skills.len(), 1);
        assert_eq!(canonical.skills[0].body, "p-body");
    }

    #[test]
    fn output_is_deterministic_across_runs() {
        let scratch = TempDir::new().unwrap();
        write_skill(scratch.path(), "zebra", "name: zebra\ndescription: x", "");
        write_skill(scratch.path(), "alpha", "name: alpha\ndescription: x", "");
        let a = read(&[scratch.path()]).unwrap().unwrap();
        let b = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(a, b);
        assert_eq!(
            a.skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "zebra"]
        );
    }

    #[test]
    fn missing_closing_delimiter_is_a_config_error() {
        let scratch = TempDir::new().unwrap();
        let dir = scratch.path().join("broken");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(SKILL_FILENAME),
            "---\nname: broken\nno closing delim",
        )
        .unwrap();
        let err = read(&[scratch.path()]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing closing"), "got: {msg}");
    }
}
