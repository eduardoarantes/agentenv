//! Read Cursor hooks directly from `<project>/.cursor/hooks.json` and
//! produce the canonical model.
//!
//! Cursor's `hooks.json` is a single-purpose, fully managed file. Its
//! shape mirrors Claude's `settings.json.hooks` block exactly, so parsing
//! is delegated to [`super::parse_claude_shape_hooks`] after reading the
//! file's top-level `hooks` field.
//!
//! Input shape:
//!
//! ```jsonc
//! {
//!   "_agentenv": "managed",   // optional marker; ignored by the reader
//!   "hooks": {
//!     "PreToolUse": [
//!       { "matcher": "Bash",
//!         "hooks": [{ "type": "command", "command": "..." }] }
//!     ]
//!   }
//! }
//! ```
//!
//! The `_agentenv` marker is the writer's anti-clobber signal; the reader
//! parses agentenv-managed and hand-authored files identically. Production
//! sync skips writing back to the source target, so a "source: cursor"
//! project's `hooks.json` should always be hand-authored — but the reader
//! is deliberately neutral about that.

use crate::error::{Error, Result};
use crate::hooks::types::Canonical;
use serde_json::Value;
use std::path::Path;

const SOURCE_NAME: &str = "cursor";

/// Build the canonical by reading `<project_root>/.cursor/hooks.json`.
/// Returns `Ok(None)` when the file is absent, has no `hooks` block, or
/// the block is empty; returns `Err` only when the file is present but
/// malformed JSON.
pub fn read(project_root: &Path) -> Result<Option<Canonical>> {
    let hooks_path = project_root.join(".cursor").join("hooks.json");
    let content = match std::fs::read_to_string(&hooks_path) {
        Ok(c) => c,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::Io(err)),
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed: Value = serde_json::from_str(trimmed)
        .map_err(|err| Error::Config(format!("failed to parse {}: {err}", hooks_path.display())))?;
    let Some(raw) = parsed.get("hooks") else {
        return Ok(None);
    };
    Ok(super::parse_claude_shape_hooks(SOURCE_NAME, raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::types::{Action, CommonEvent, Event};
    use serde_json::json;
    use tempfile::TempDir;

    /// Build a project root with `.cursor/hooks.json` containing the given
    /// raw JSON value.
    fn project_with_raw(raw: Value) -> TempDir {
        let project = TempDir::new().unwrap();
        let dir = project.path().join(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hooks.json"), raw.to_string()).unwrap();
        project
    }

    #[test]
    fn returns_none_when_file_absent() {
        let project = TempDir::new().unwrap();
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn returns_none_when_file_empty() {
        let project = TempDir::new().unwrap();
        let dir = project.path().join(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hooks.json"), "   \n").unwrap();
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn returns_none_when_no_hooks_key() {
        let project = project_with_raw(json!({}));
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn returns_none_when_hooks_block_empty() {
        let project = project_with_raw(json!({ "hooks": {} }));
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn ignores_agentenv_marker_and_parses_hooks() {
        let project = project_with_raw(json!({
            "_agentenv": "managed",
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [{"type": "command", "command": "echo bash"}]
                    }
                ]
            }
        }));
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.source, "cursor");
        assert_eq!(canonical.hooks.len(), 1);
        assert!(matches!(
            canonical.hooks[0].event,
            Event::Common(CommonEvent::PreToolUse)
        ));
        let Action::Command { command, .. } = &canonical.hooks[0].action;
        assert_eq!(command, "echo bash");
    }

    #[test]
    fn parses_hand_authored_file_without_marker() {
        let project = project_with_raw(json!({
            "hooks": {
                "Stop": [
                    {"matcher": ".*", "hooks": [{"type": "command", "command": "notify"}]}
                ]
            }
        }));
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 1);
        assert!(matches!(
            canonical.hooks[0].event,
            Event::Common(CommonEvent::Stop)
        ));
    }

    #[test]
    fn preserves_unknown_event_as_native_tagged_with_cursor_source() {
        let project = project_with_raw(json!({
            "hooks": {
                "afterFileEdit": [
                    {"matcher": "*", "hooks": [{"type": "command", "command": "fmt"}]}
                ]
            }
        }));
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 1);
        let Event::Native(n) = &canonical.hooks[0].event else {
            panic!("expected Native event");
        };
        assert_eq!(n.source, "cursor");
        assert_eq!(n.native_event, "afterFileEdit");
    }

    #[test]
    fn malformed_json_returns_config_error() {
        let project = TempDir::new().unwrap();
        let dir = project.path().join(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hooks.json"), "not json").unwrap();
        let err = read(project.path()).unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
    }
}
