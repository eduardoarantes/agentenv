//! Read Cursor-shaped skills into the canonical model.
//!
//! Cursor stores skills using the same agentskills.io `<name>/SKILL.md`
//! layout that Claude Code does, just rooted under `.cursor/skills/`
//! instead of `.claude/skills/`. The reader is a thin wrapper around the
//! shared layout walker in [`super`].
//!
//! Roots are project-local (`<project>/.cursor/skills`) and plugin-local
//! (`<plugin>/skills` — marketplace plugins are always Claude-shaped). Name
//! collisions resolve first-root-wins (project before plugins).

use crate::error::Result;
use crate::skills::types::Canonical;
use std::path::Path;

const SOURCE_NAME: &str = "cursor";

/// Build the canonical from a set of skill roots.
///
/// Returns `Ok(None)` when no skill directory was found across any root.
pub fn read(roots: &[&Path]) -> Result<Option<Canonical>> {
    super::parse_agentskills_layout(SOURCE_NAME, roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_skill(root: &Path, name: &str, frontmatter: &str, body: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        let content = format!("---\n{frontmatter}\n---\n{body}");
        fs::write(dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn tags_canonical_with_cursor_source() {
        let scratch = TempDir::new().unwrap();
        write_skill(
            scratch.path(),
            "hello",
            "name: hello\ndescription: Sample",
            "body\n",
        );
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(canonical.source, "cursor");
        assert_eq!(canonical.skills[0].name, "hello");
    }

    #[test]
    fn returns_none_when_no_skill_dirs() {
        let scratch = TempDir::new().unwrap();
        assert!(read(&[scratch.path()]).unwrap().is_none());
    }

    #[test]
    fn first_root_wins_on_collision() {
        let project = TempDir::new().unwrap();
        let plugin = TempDir::new().unwrap();
        write_skill(
            project.path(),
            "dup",
            "name: dup\ndescription: project",
            "P\n",
        );
        write_skill(
            plugin.path(),
            "dup",
            "name: dup\ndescription: plugin",
            "X\n",
        );
        let canonical = read(&[project.path(), plugin.path()]).unwrap().unwrap();
        assert_eq!(canonical.skills.len(), 1);
        assert_eq!(canonical.skills[0].body, "P\n");
    }
}
