//! Source-target readers: turn a target's native hooks file into the
//! canonical model losslessly.
//!
//! Each reader receives a [`Config`](crate::config::Config) (which already
//! holds the imported source data when `use_claude_config: true`) and
//! returns a [`Canonical`](crate::hooks::types::Canonical) containing every
//! hook the source declared. Unknown / source-specific events are preserved
//! via [`Event::Native`](crate::hooks::types::Event::Native).
//!
//! v1 implements only `claude-code` as a source. Other source targets
//! return a clear "not yet implemented" error from
//! [`crate::config::Config::validate`].

pub mod claude_code;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::hooks::types::Canonical;

/// Dispatch to the right reader based on `source` and produce the canonical
/// artifact.
///
/// Returns `Ok(None)` when the source target declared no hooks (e.g. user
/// has no Claude `hooks` block). Returns `Err` only on malformed source
/// data — a missing source is not an error here; the call site decides
/// whether to require one.
pub fn read(config: &Config, source: &str) -> Result<Option<Canonical>> {
    match source {
        "claude-code" => claude_code::read(config),
        other => Err(Error::Config(format!(
            "hooks source `{other}` is not implemented in this version"
        ))),
    }
}
