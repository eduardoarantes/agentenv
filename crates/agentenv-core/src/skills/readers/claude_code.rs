//! Read Claude Code-shaped skills into the canonical model.
//!
//! Claude Code stores skills in the
//! [agentskills.io](https://agentskills.io) `<name>/SKILL.md` directory
//! layout — identical to the layout every other documented source target
//! uses. The reader is therefore a thin wrapper around the shared layout
//! walker in [`super`].
//!
//! Roots are project-local (`<project>/.claude/skills`) and plugin-local
//! (`<plugin>/skills`). Hidden entries and directories without a `SKILL.md`
//! are skipped silently. Name collisions resolve first-root-wins (project
//! wins over plugins, plugins in the order supplied).

use crate::error::Result;
use crate::skills::types::Canonical;
use std::path::Path;

const SOURCE_NAME: &str = "claude-code";

/// Build the canonical from a set of skill roots.
///
/// Returns `Ok(None)` when no skill directory was found across any root,
/// so callers can short-circuit cleanly on a fresh project.
pub fn read(roots: &[&Path]) -> Result<Option<Canonical>> {
    super::parse_agentskills_layout(SOURCE_NAME, roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::SidecarKind;
    use std::fs;
    use tempfile::TempDir;

    fn write_skill(root: &Path, name: &str, frontmatter: &str, body: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        let content = format!("---\n{frontmatter}\n---\n{body}");
        fs::write(dir.join("SKILL.md"), content).unwrap();
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
        fs::write(dir.join("SKILL.md"), "Just a body, no fences\n").unwrap();
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
        fs::write(dir.join("SKILL.md"), "---\nname: broken\nno closing delim").unwrap();
        let err = read(&[scratch.path()]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing closing"), "got: {msg}");
    }
}
