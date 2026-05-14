//! Read Copilot-shaped skills into the canonical model.
//!
//! Copilot stores skills under `<project>/.github/skills/` using the
//! agentskills.io `<name>/SKILL.md` directory shape. The reader is a thin
//! wrapper around the shared layout walker in [`super`].

use crate::error::Result;
use crate::skills::types::Canonical;
use std::path::Path;

const SOURCE_NAME: &str = "copilot";

/// Build the canonical from a set of skill roots.
pub fn read(roots: &[&Path]) -> Result<Option<Canonical>> {
    super::parse_agentskills_layout(SOURCE_NAME, roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn tags_canonical_with_copilot_source() {
        let scratch = TempDir::new().unwrap();
        let dir = scratch.path().join("hello");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: hello\ndescription: x\n---\nbody\n",
        )
        .unwrap();
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(canonical.source, "copilot");
        assert_eq!(canonical.skills[0].name, "hello");
    }
}
