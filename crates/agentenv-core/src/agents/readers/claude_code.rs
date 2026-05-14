//! Read Claude Code-shaped agents into the canonical model.
//!
//! Walks each root for `<name>.md` files (flat-file layout, not directory
//! form — agents are single Markdown files with YAML frontmatter):
//!
//! ```text
//! <root>/
//!   code-reviewer.md      ← frontmatter + body
//!   refactor-bot.md
//! ```
//!
//! Roots are project-local (`<project>/.claude/agents`) and plugin-local
//! (`<plugin>/agents`). Hidden entries, non-`.md` files, and subdirectories
//! are skipped silently. Name collisions resolve first-root-wins (project
//! wins over plugins, plugins in the order supplied).

use crate::agents::types::{Canonical, CanonicalAgent};
use crate::error::{Error, Result};
use std::fs;
use std::path::Path;

const SOURCE_NAME: &str = "claude-code";
const AGENT_EXTENSION: &str = "md";

/// Build the canonical from a set of agent roots.
///
/// Returns `Ok(None)` when no agent file was found across any root.
pub fn read(roots: &[&Path]) -> Result<Option<Canonical>> {
    let mut agents: Vec<CanonicalAgent> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let mut entries: Vec<fs::DirEntry> = fs::read_dir(root)?.collect::<std::io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let file_name = entry.file_name();
            match file_name.to_str() {
                Some(s) if !s.starts_with('.') => {},
                _ => continue,
            }
            let path = entry.path();
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Some(stem) = path
                .extension()
                .and_then(|e| e.to_str())
                .filter(|ext| ext.eq_ignore_ascii_case(AGENT_EXTENSION))
                .and_then(|_| path.file_stem().and_then(|s| s.to_str()))
            else {
                continue;
            };
            let name = stem.to_string();
            if !seen.insert(name.clone()) {
                continue;
            }
            agents.push(parse_agent(&name, &path)?);
        }
    }

    if agents.is_empty() {
        return Ok(None);
    }
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Some(Canonical {
        source: SOURCE_NAME.to_string(),
        agents,
    }))
}

fn parse_agent(name: &str, file: &Path) -> Result<CanonicalAgent> {
    let raw = fs::read_to_string(file)?;
    let (frontmatter, body) = crate::frontmatter::split(&raw)
        .map_err(|err| Error::Config(format!("invalid agent at {}: {err}", file.display())))?;
    Ok(CanonicalAgent {
        name: name.to_string(),
        frontmatter,
        body,
        source_file: file.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_agent(root: &Path, name: &str, frontmatter: &str, body: &str) {
        fs::create_dir_all(root).unwrap();
        let content = format!("---\n{frontmatter}\n---\n{body}");
        fs::write(root.join(format!("{name}.md")), content).unwrap();
    }

    #[test]
    fn returns_none_when_no_roots_exist() {
        let scratch = TempDir::new().unwrap();
        let missing = scratch.path().join("does-not-exist");
        assert!(read(&[&missing]).unwrap().is_none());
    }

    #[test]
    fn returns_none_when_root_has_no_md_files() {
        let scratch = TempDir::new().unwrap();
        fs::create_dir_all(scratch.path()).unwrap();
        fs::write(scratch.path().join("notes.txt"), "ignored").unwrap();
        fs::write(scratch.path().join(".gitkeep"), "").unwrap();
        assert!(read(&[scratch.path()]).unwrap().is_none());
    }

    #[test]
    fn parses_agent_with_frontmatter_and_body() {
        let scratch = TempDir::new().unwrap();
        write_agent(
            scratch.path(),
            "code-reviewer",
            "name: code-reviewer\ndescription: Reviews PRs\ntools: [Read, Grep]",
            "You are a code reviewer.\n",
        );
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(canonical.source, "claude-code");
        assert_eq!(canonical.agents.len(), 1);
        let a = &canonical.agents[0];
        assert_eq!(a.name, "code-reviewer");
        assert_eq!(a.body, "You are a code reviewer.\n");
        let tools = a
            .frontmatter
            .get(serde_yaml::Value::String("tools".to_string()))
            .and_then(|v| v.as_sequence())
            .unwrap();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn handles_agent_with_no_frontmatter() {
        let scratch = TempDir::new().unwrap();
        fs::create_dir_all(scratch.path()).unwrap();
        fs::write(scratch.path().join("plain.md"), "no frontmatter at all\n").unwrap();
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert!(canonical.agents[0].frontmatter.is_empty());
        assert_eq!(canonical.agents[0].body, "no frontmatter at all\n");
    }

    #[test]
    fn skips_subdirectories() {
        let scratch = TempDir::new().unwrap();
        fs::create_dir_all(scratch.path().join("group")).unwrap();
        fs::write(
            scratch.path().join("group/nested.md"),
            "---\nname: x\n---\nbody",
        )
        .unwrap();
        write_agent(scratch.path(), "real", "name: real\ndescription: r", "p");
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        let names: Vec<_> = canonical.agents.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn first_root_wins_on_agent_name_collision() {
        let project = TempDir::new().unwrap();
        let plugin = TempDir::new().unwrap();
        write_agent(
            project.path(),
            "dup",
            "name: dup\ndescription: project",
            "P",
        );
        write_agent(plugin.path(), "dup", "name: dup\ndescription: plugin", "X");
        let canonical = read(&[project.path(), plugin.path()]).unwrap().unwrap();
        assert_eq!(canonical.agents.len(), 1);
        assert_eq!(canonical.agents[0].body, "P");
    }

    #[test]
    fn output_is_deterministic_across_runs() {
        let scratch = TempDir::new().unwrap();
        write_agent(scratch.path(), "zebra", "name: zebra\ndescription: x", "");
        write_agent(scratch.path(), "alpha", "name: alpha\ndescription: x", "");
        let a = read(&[scratch.path()]).unwrap().unwrap();
        let b = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(a, b);
        let names: Vec<_> = a.agents.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zebra"]);
    }

    #[test]
    fn missing_closing_delimiter_is_a_config_error() {
        let scratch = TempDir::new().unwrap();
        fs::create_dir_all(scratch.path()).unwrap();
        fs::write(
            scratch.path().join("broken.md"),
            "---\nname: x\nno closing\n",
        )
        .unwrap();
        let err = read(&[scratch.path()]).unwrap_err();
        assert!(err.to_string().contains("missing closing"));
    }

    #[test]
    fn skips_non_markdown_files() {
        let scratch = TempDir::new().unwrap();
        fs::create_dir_all(scratch.path()).unwrap();
        fs::write(
            scratch.path().join("README.txt"),
            "---\nname: skipme\n---\n",
        )
        .unwrap();
        fs::write(scratch.path().join("data.json"), "{}").unwrap();
        write_agent(scratch.path(), "real", "name: real\ndescription: r", "p");
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        let names: Vec<_> = canonical.agents.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["real"]);
    }
}
