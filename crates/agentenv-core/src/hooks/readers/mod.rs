//! Source-target readers: turn a target's native hooks file into the
//! canonical model losslessly.
//!
//! Each reader reads its source's native filesystem layout directly from
//! `project_root` and returns a
//! [`Canonical`](crate::hooks::types::Canonical) containing every hook the
//! source declared. Unknown / source-specific events are preserved via
//! [`Event::Native`](crate::hooks::types::Event::Native).
//!
//! v1 implements only `claude-code` as a source. Other source targets
//! return a clear "not yet implemented" error from
//! [`crate::config::Config::validate`].

pub mod claude_code;

use crate::error::{Error, Result};
use crate::hooks::types::Canonical;
use std::path::Path;

/// Dispatch to the right reader based on `source` and produce the canonical
/// artifact.
///
/// Returns `Ok(None)` when the source target declared no hooks (e.g. the
/// project has no `.claude/settings.json` or it has no `hooks` block).
/// Returns `Err` only on malformed source data — a missing source is not an
/// error here; the call site decides whether to require one.
pub fn read(source: &str, project_root: &Path) -> Result<Option<Canonical>> {
    match source {
        "claude-code" => claude_code::read(project_root),
        other => Err(Error::Config(format!(
            "hooks source `{other}` is not implemented in this version"
        ))),
    }
}
