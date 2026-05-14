//! Per-target writers: render the canonical model into a target's native
//! hooks file.
//!
//! Each writer is responsible for two things:
//!
//! 1. **Refuse-on-conflict** — before writing, the writer reads the
//!    destination file (if any) and errors out if it contains hooks the
//!    user authored. agentenv-authored files are recognised by a target-
//!    specific marker (a sentinel comment block for TOML, a top-level
//!    `_agentenv` field for JSON).
//! 2. **Render & write** — translate the canonical hooks into the target's
//!    native shape, dropping (with a warning in [`WriteReport`]) any event
//!    that has no native counterpart on this target.

pub mod codex;
pub mod cursor;

use crate::error::{Error, Result};
use crate::hooks::types::{Canonical, WriteReport};
use std::path::{Path, PathBuf};

/// Dispatch to the right writer based on the target name.
pub fn write(target: &str, canonical: &Canonical, project_root: &Path) -> Result<WriteReport> {
    match target {
        "cursor" => cursor::write(canonical, project_root),
        "codex" => codex::write(canonical, project_root),
        other => Err(Error::Config(format!(
            "hooks writer for target `{other}` is not implemented in this version"
        ))),
    }
}

/// Project-rooted artifact path the writer for `target` would touch, if any.
///
/// Used by the gitignore writer to ignore the tool folder that agentenv may
/// have written into even when no state-link points inside it (cursor's
/// `hooks.json` is materialized, not symlinked, so it never appears in
/// state). Returns `None` for writers whose destination lives outside the
/// project root (codex writes to `~/.codex/config.toml`).
pub fn project_artifact(target: &str, project_root: &Path) -> Option<PathBuf> {
    match target {
        "cursor" => Some(cursor::destination(project_root)),
        _ => None,
    }
}
