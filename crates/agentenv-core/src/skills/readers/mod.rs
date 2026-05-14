//! Source-target readers: turn a target's native skill layout into the
//! canonical model losslessly.
//!
//! v1 implements only `claude-code` as a source. Other source targets are
//! rejected at config validation.

pub mod claude_code;

use crate::error::{Error, Result};
use crate::skills::types::Canonical;
use std::path::Path;

/// Dispatch to the right reader based on `source` and produce the canonical
/// artifact. `roots` is the ordered list of directories to walk — typically
/// `[<project>/.claude/skills, <plugin1>/skills, <plugin2>/skills, …]`.
///
/// Returns `Ok(None)` when no skill was found across any root.
pub fn read(source: &str, roots: &[&Path]) -> Result<Option<Canonical>> {
    match source {
        "claude-code" => claude_code::read(roots),
        other => Err(Error::Config(format!(
            "skills source `{other}` is not implemented in this version"
        ))),
    }
}
