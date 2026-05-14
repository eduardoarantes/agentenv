//! Read Copilot-shaped agents into the canonical model.
//!
//! Copilot agents are flat Markdown files with YAML frontmatter, like
//! Claude and Cursor, but with the distinctive `.agent.md` suffix instead
//! of plain `.md`. The reader is a thin wrapper around the shared walker
//! in [`super`] with `name_suffix = ".agent.md"`.
//!
//! Roots are project-local (`<project>/.github/agents`) and plugin-local
//! (`<plugin>/agents` — Claude-shaped per the v1 marketplace contract).
//! Plain `.md` files in the same directory are intentionally skipped —
//! only `.agent.md` files are agents on Copilot.

use crate::agents::types::Canonical;
use crate::error::Result;
use std::path::Path;

const SOURCE_NAME: &str = "copilot";
const NAME_SUFFIX: &str = ".agent.md";

/// Build the canonical from a set of agent roots.
///
/// Returns `Ok(None)` when no `.agent.md` file was found across any root.
pub fn read(roots: &[&Path]) -> Result<Option<Canonical>> {
    super::parse_markdown_agents(SOURCE_NAME, roots, NAME_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_copilot_agent(root: &Path, name: &str, frontmatter: &str, body: &str) {
        fs::create_dir_all(root).unwrap();
        let content = format!("---\n{frontmatter}\n---\n{body}");
        fs::write(root.join(format!("{name}.agent.md")), content).unwrap();
    }

    #[test]
    fn tags_canonical_with_copilot_source_and_strips_agent_suffix() {
        let scratch = TempDir::new().unwrap();
        write_copilot_agent(
            scratch.path(),
            "rev",
            "name: rev\ndescription: Reviews PRs",
            "body\n",
        );
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(canonical.source, "copilot");
        // Canonical name is the stem WITHOUT `.agent.md` (so cross-tool
        // writers produce `<name>.md` etc. consistently).
        assert_eq!(canonical.agents[0].name, "rev");
    }

    #[test]
    fn ignores_plain_md_files_in_same_directory() {
        let scratch = TempDir::new().unwrap();
        // Plain `.md` (without `.agent`) is NOT a copilot agent.
        fs::create_dir_all(scratch.path()).unwrap();
        fs::write(
            scratch.path().join("README.md"),
            "---\nname: ignored\n---\nbody",
        )
        .unwrap();
        write_copilot_agent(scratch.path(), "real", "name: real\ndescription: r", "p");

        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        let names: Vec<_> = canonical.agents.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn returns_none_when_no_agent_md_files() {
        let scratch = TempDir::new().unwrap();
        assert!(read(&[scratch.path()]).unwrap().is_none());
    }
}
