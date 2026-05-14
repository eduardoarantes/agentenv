//! Source-target readers for agents.
//!
//! v1 implements only `claude-code` as a source.

pub mod claude_code;

use crate::agents::types::Canonical;
use crate::error::{Error, Result};
use std::path::Path;

/// Dispatch to the right reader based on `source` and produce the canonical
/// artifact. `roots` is the ordered list of directories to walk — typically
/// `[<project>/.claude/agents, <plugin1>/agents, …]`.
///
/// Returns `Ok(None)` when no agent was found across any root.
pub fn read(source: &str, roots: &[&Path]) -> Result<Option<Canonical>> {
    match source {
        "claude-code" => claude_code::read(roots),
        other => Err(Error::Config(format!(
            "agents source `{other}` is not implemented in this version"
        ))),
    }
}
