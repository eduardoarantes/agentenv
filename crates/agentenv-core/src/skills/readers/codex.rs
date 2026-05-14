//! Read Codex-shaped skills into the canonical model.
//!
//! Codex re-uses the cross-tool `.agents/skills/<name>/SKILL.md` directory
//! (the same alias every non-Claude tool can pick up), so the on-disk
//! shape is identical to claude-code's and the reader is a thin wrapper
//! around the shared layout walker in [`super`].
//!
//! Roots are project-local (`<project>/.agents/skills`) and plugin-local
//! (`<plugin>/skills`). Plugins ship Claude-shaped per the v1 marketplace
//! contract regardless of the project's `source`.

use crate::error::Result;
use crate::skills::types::Canonical;
use std::path::Path;

const SOURCE_NAME: &str = "codex";

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
    fn tags_canonical_with_codex_source() {
        let scratch = TempDir::new().unwrap();
        let dir = scratch.path().join("hello");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: hello\ndescription: x\n---\nbody\n",
        )
        .unwrap();
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(canonical.source, "codex");
        assert_eq!(canonical.skills[0].name, "hello");
    }
}
