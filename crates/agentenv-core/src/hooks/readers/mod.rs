//! Source-target readers: turn a target's native hooks file into the
//! canonical model losslessly.
//!
//! Each reader reads its source's native filesystem layout directly from
//! `project_root` and returns a
//! [`Canonical`](crate::hooks::types::Canonical) containing every hook the
//! source declared. Unknown / source-specific events are preserved via
//! [`Event::Native`](crate::hooks::types::Event::Native).
//!
//! Sources whose native shape matches Claude's `settings.json` (a top-level
//! `hooks` object keyed by event names) — currently `claude-code` and
//! `cursor` — share the parser in [`parse_claude_shape_hooks`]; the
//! per-source files differ only in path + source name.
//! Sources with diverging shapes (TOML-based Codex) implement their own
//! parser.

pub mod claude_code;
pub mod codex;
pub mod cursor;

use crate::error::{Error, Result};
use crate::hooks::types::{Action, Canonical, CommonEvent, Event, Hook, Matcher, NativeEvent};
use serde_json::Value;
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
        "cursor" => cursor::read(project_root),
        "codex" => codex::read(project_root),
        other => Err(Error::Config(format!(
            "hooks source `{other}` is not implemented in this version"
        ))),
    }
}

/// Parse a Claude-shaped `hooks` object (the value of the top-level
/// `hooks` key in Claude Code's `settings.json` or Cursor's `hooks.json`)
/// into a canonical artifact tagged with `source_name`.
///
/// Returns `Ok(None)` when the value is not an object or has no entries
/// that yielded a parseable hook. Returns `Err` only on a malformed inner
/// `hooks: [...]` shape that the schema explicitly forbids; unknown event
/// names round-trip through [`Event::Native`].
pub(super) fn parse_claude_shape_hooks(
    source_name: &str,
    hooks_value: &Value,
) -> Option<Canonical> {
    let Value::Object(events) = hooks_value else {
        return None;
    };
    if events.is_empty() {
        return None;
    }

    let mut hooks = Vec::new();
    // Sort keys for deterministic canonical output across runs.
    let mut keys: Vec<&String> = events.keys().collect();
    keys.sort();
    for event_name in keys {
        let matchers = match events.get(event_name) {
            Some(Value::Array(arr)) => arr,
            _ => continue,
        };
        let common = CommonEvent::from_pascal(event_name);

        for matcher_entry in matchers {
            let entry_obj = match matcher_entry {
                Value::Object(m) => m,
                _ => continue,
            };
            let matcher_str = entry_obj.get("matcher").and_then(Value::as_str);
            let inner = entry_obj.get("hooks").and_then(Value::as_array);

            if let Some(actions) = inner {
                for action_entry in actions {
                    if let Some(action) = parse_action(action_entry) {
                        hooks.push(make_hook(
                            source_name,
                            event_name,
                            common,
                            matcher_str,
                            matcher_entry,
                            action,
                        ));
                    }
                }
            } else {
                // No inner `hooks` array. Preserve the whole entry as
                // Native so the canonical is still lossless.
                hooks.push(Hook {
                    event: Event::Native(NativeEvent {
                        source: source_name.to_string(),
                        native_event: event_name.clone(),
                        payload: matcher_entry.clone(),
                    }),
                    matcher: None,
                    action: Action::Command {
                        command: String::new(),
                        timeout_ms: None,
                        cwd: None,
                    },
                });
            }
        }
    }

    if hooks.is_empty() {
        return None;
    }
    Some(Canonical {
        source: source_name.to_string(),
        hooks,
    })
}

fn make_hook(
    source_name: &str,
    event_name: &str,
    common: Option<CommonEvent>,
    matcher_str: Option<&str>,
    full_matcher_entry: &Value,
    action: Action,
) -> Hook {
    let event = match common {
        Some(c) => Event::Common(c),
        None => Event::Native(NativeEvent {
            source: source_name.to_string(),
            native_event: event_name.to_string(),
            payload: full_matcher_entry.clone(),
        }),
    };
    let matcher = matcher_str.map(|s| Matcher {
        tool: Some(s.to_string()),
    });
    Hook {
        event,
        matcher,
        action,
    }
}

fn parse_action(value: &Value) -> Option<Action> {
    let obj = value.as_object()?;
    let ty = obj.get("type").and_then(Value::as_str)?;
    if ty != "command" {
        return None;
    }
    let command = obj.get("command").and_then(Value::as_str)?.to_string();
    let timeout_ms = obj
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .or_else(|| obj.get("timeout").and_then(Value::as_u64));
    let cwd = obj
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from);
    Some(Action::Command {
        command,
        timeout_ms,
        cwd,
    })
}
