//! Read Claude Code hooks directly from `<project>/.claude/settings.json`
//! and produce the canonical model.
//!
//! Source-driven contract: when `source: claude-code` is set, this reader
//! is the single source of truth for hooks — it reads the project's
//! `settings.json` from disk on every pipeline run.
//!
//! Input shape (the `hooks` value inside `settings.json`):
//!
//! ```jsonc
//! {
//!   "PreToolUse": [
//!     { "matcher": "Bash",
//!       "hooks": [{ "type": "command", "command": "..." }] }
//!   ],
//!   "Stop": [...]
//! }
//! ```
//!
//! The reader emits one canonical [`Hook`] per inner action entry. Event
//! names recognised by [`CommonEvent::from_pascal`] become common-core
//! variants; anything else (Claude-specific or future events) becomes a
//! [`NativeEvent`] preserving the raw matcher block, so the canonical
//! artifact is lossless.

use crate::error::{Error, Result};
use crate::hooks::types::{Action, Canonical, CommonEvent, Event, Hook, Matcher, NativeEvent};
use serde_json::Value;
use std::path::Path;

const SOURCE_NAME: &str = "claude-code";

/// Build the canonical by reading `<project_root>/.claude/settings.json`.
/// Returns `Ok(None)` when the file is absent or has no `hooks` block;
/// otherwise the canonical contains one [`Hook`] per inner action across
/// every event key. Returns `Err` only when the file is present but
/// malformed.
pub fn read(project_root: &Path) -> Result<Option<Canonical>> {
    let settings_path = project_root.join(".claude").join("settings.json");
    let content = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::Io(err)),
    };
    let parsed: Value = serde_json::from_str(&content).map_err(|err| {
        Error::Config(format!(
            "failed to parse {}: {err}",
            settings_path.display()
        ))
    })?;
    let Some(raw) = parsed.get("hooks") else {
        return Ok(None);
    };
    let Value::Object(events) = raw else {
        return Ok(None);
    };
    if events.is_empty() {
        return Ok(None);
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
                        source: SOURCE_NAME.to_string(),
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
        return Ok(None);
    }
    Ok(Some(Canonical {
        source: SOURCE_NAME.to_string(),
        hooks,
    }))
}

fn make_hook(
    event_name: &str,
    common: Option<CommonEvent>,
    matcher_str: Option<&str>,
    full_matcher_entry: &Value,
    action: Action,
) -> Hook {
    let event = match common {
        Some(c) => Event::Common(c),
        None => Event::Native(NativeEvent {
            source: SOURCE_NAME.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    /// Build a project root with `.claude/settings.json` whose top-level
    /// `hooks` key equals `hooks_value`. Returns the TempDir so the caller
    /// keeps it alive for the duration of the test.
    fn project_with_hooks(hooks_value: Value) -> TempDir {
        let project = TempDir::new().unwrap();
        let claude_dir = project.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.json"),
            json!({ "hooks": hooks_value }).to_string(),
        )
        .unwrap();
        project
    }

    #[test]
    fn read_returns_none_when_settings_json_absent() {
        let project = TempDir::new().unwrap();
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn read_returns_none_when_no_hooks_key() {
        let project = TempDir::new().unwrap();
        let claude_dir = project.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn read_returns_none_when_empty_object() {
        let project = project_with_hooks(json!({}));
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn maps_pretooluse_with_matcher_to_common_event() {
        let project = project_with_hooks(json!({
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [
                        {"type": "command", "command": "echo bash", "timeout_ms": 5000}
                    ]
                }
            ]
        }));
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.source, "claude-code");
        assert_eq!(canonical.hooks.len(), 1);
        let hook = &canonical.hooks[0];
        assert!(matches!(hook.event, Event::Common(CommonEvent::PreToolUse)));
        assert_eq!(hook.matcher.as_ref().unwrap().tool.as_deref(), Some("Bash"));
        let Action::Command {
            command,
            timeout_ms,
            cwd,
        } = &hook.action;
        assert_eq!(command, "echo bash");
        assert_eq!(*timeout_ms, Some(5000));
        assert_eq!(*cwd, None);
    }

    #[test]
    fn preserves_unknown_event_as_native() {
        let project = project_with_hooks(json!({
            "PreCompact": [
                {
                    "matcher": ".*",
                    "hooks": [{"type": "command", "command": "echo compact"}]
                }
            ],
            "TeammateIdle": [
                {"matcher": "*", "hooks": [{"type": "command", "command": "echo idle"}]}
            ]
        }));
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 2);

        // PreCompact IS in the common-core catalog — should map to Common.
        assert!(canonical
            .hooks
            .iter()
            .any(|h| matches!(h.event, Event::Common(CommonEvent::PreCompact))));

        // TeammateIdle is NOT — should be preserved as Native.
        let idle = canonical
            .hooks
            .iter()
            .find(|h| match &h.event {
                Event::Native(n) => n.native_event == "TeammateIdle",
                _ => false,
            })
            .expect("TeammateIdle must be Native");
        if let Event::Native(n) = &idle.event {
            assert_eq!(n.source, "claude-code");
            // payload should be the verbatim matcher entry
            assert_eq!(
                n.payload,
                json!({"matcher": "*", "hooks": [{"type": "command", "command": "echo idle"}]})
            );
        }
    }

    #[test]
    fn fans_out_multiple_inner_actions_to_separate_hooks() {
        let project = project_with_hooks(json!({
            "Stop": [
                {
                    "matcher": ".*",
                    "hooks": [
                        {"type": "command", "command": "notify-a"},
                        {"type": "command", "command": "notify-b"}
                    ]
                }
            ]
        }));
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 2);
        let cmds: Vec<&str> = canonical
            .hooks
            .iter()
            .map(|h| {
                let Action::Command { command, .. } = &h.action;
                command.as_str()
            })
            .collect();
        assert!(cmds.contains(&"notify-a"));
        assert!(cmds.contains(&"notify-b"));
    }

    #[test]
    fn output_is_deterministic_across_runs() {
        let raw = json!({
            "Stop": [{"matcher": ".*", "hooks": [{"type": "command", "command": "z"}]}],
            "PreToolUse": [{"matcher": "Write", "hooks": [{"type": "command", "command": "a"}]}]
        });
        let p_a = project_with_hooks(raw.clone());
        let p_b = project_with_hooks(raw);
        let a = read(p_a.path()).unwrap().unwrap();
        let b = read(p_b.path()).unwrap().unwrap();
        assert_eq!(a, b);
        // Sorted keys → PreToolUse comes before Stop.
        assert_eq!(a.hooks[0].event.name(), "PreToolUse");
        assert_eq!(a.hooks[1].event.name(), "Stop");
    }

    #[test]
    fn malformed_settings_json_returns_config_error() {
        let project = TempDir::new().unwrap();
        let claude_dir = project.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(claude_dir.join("settings.json"), "not json").unwrap();
        let err = read(project.path()).unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
    }
}
