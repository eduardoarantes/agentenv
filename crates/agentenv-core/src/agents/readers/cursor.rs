//! Read Cursor-shaped agents into the canonical model.
//!
//! Cursor agents are flat Markdown files with YAML frontmatter, suffix
//! `.md`, rooted at `<project>/.cursor/agents` (project) or
//! `<plugin>/agents` (plugin — Claude-shaped). The reader is a thin
//! wrapper around the shared walker in [`super`].

use crate::agents::types::Canonical;
use crate::error::Result;
use std::path::Path;

const SOURCE_NAME: &str = "cursor";
const NAME_SUFFIX: &str = ".md";

/// Build the canonical from a set of agent roots.
///
/// Returns `Ok(None)` when no agent file was found across any root.
pub fn read(roots: &[&Path]) -> Result<Option<Canonical>> {
    super::parse_markdown_agents(SOURCE_NAME, roots, NAME_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_agent(root: &Path, name: &str, frontmatter: &str, body: &str) {
        fs::create_dir_all(root).unwrap();
        let content = format!("---\n{frontmatter}\n---\n{body}");
        fs::write(root.join(format!("{name}.md")), content).unwrap();
    }

    #[test]
    fn tags_canonical_with_cursor_source() {
        let scratch = TempDir::new().unwrap();
        write_agent(
            scratch.path(),
            "rev",
            "name: rev\ndescription: Reviews PRs",
            "body\n",
        );
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(canonical.source, "cursor");
        assert_eq!(canonical.agents[0].name, "rev");
    }

    #[test]
    fn returns_none_when_no_md_files() {
        let scratch = TempDir::new().unwrap();
        assert!(read(&[scratch.path()]).unwrap().is_none());
    }
}
