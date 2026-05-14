//! Copilot hooks reader — semantic stub.
//!
//! GitHub Copilot has no public hook convention as of this release. The
//! reader exists so `source: copilot` is a usable source for skills and
//! agents even when the user also has hook write targets (cursor/codex)
//! configured: the hooks pipeline calls this and gets a clean `Ok(None)`,
//! short-circuits, and proceeds with the other capabilities.
//!
//! When Copilot publishes a hook spec, this stub becomes a real reader
//! against whatever file shape they define.

use crate::error::Result;
use crate::hooks::types::Canonical;
use std::path::Path;

/// Always returns `Ok(None)` — Copilot has no hook layer to read.
///
/// `project_root` is accepted (and ignored) so the signature matches the
/// dispatch in [`super::read`].
pub fn read(_project_root: &Path) -> Result<Option<Canonical>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn returns_none_even_when_project_has_unrelated_files() {
        let project = TempDir::new().unwrap();
        // Sanity: there's no `.copilot/hooks.*` file shape we read, so
        // even arbitrary `.github/` content should be ignored.
        let dir = project.path().join(".github");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hooks.json"), r#"{"hooks":{}}"#).unwrap();
        assert!(read(project.path()).unwrap().is_none());
    }
}
