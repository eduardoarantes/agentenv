//! Read Claude Code hooks from `Config::claude_hooks` and produce the
//! canonical model.
//!
//! Input shape (verbatim Claude `settings.json` `hooks` value):
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

use crate::config::Config;
use crate::error::Result;
use crate::hooks::types::{Action, Canonical, CommonEvent, Event, Hook, Matcher, NativeEvent};
use serde_json::Value;

const SOURCE_NAME: &str = "claude-code";

/// Build the canonical from `config.claude_hooks`. Returns `Ok(None)` when
/// no hooks are present; otherwise the canonical contains one [`Hook`] per
/// inner action across every event key.
pub fn read(config: &Config) -> Result<Option<Canonical>> {
    let Some(raw) = config.claude_hooks.as_ref() else {
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
    use crate::config::Config;
    use serde_json::json;
    use std::collections::HashMap;

    fn config_with_hooks(value: Value) -> Config {
        Config {
            version: 1,
            marketplaces: HashMap::new(),
            plugins: vec![],
            targets: HashMap::new(),
            sync: Default::default(),
            clean: Default::default(),
            use_claude_config: true,
            gitignore_managed_links: false,
            instruction_files: HashMap::new(),
            claude_hooks: Some(value),
            source: Some("claude-code".to_string()),
        }
    }

    #[test]
    fn read_returns_none_when_no_hooks() {
        let mut config = config_with_hooks(json!({}));
        config.claude_hooks = None;
        assert!(read(&config).unwrap().is_none());
    }

    #[test]
    fn read_returns_none_when_empty_object() {
        let config = config_with_hooks(json!({}));
        assert!(read(&config).unwrap().is_none());
    }

    #[test]
    fn maps_pretooluse_with_matcher_to_common_event() {
        let config = config_with_hooks(json!({
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [
                        {"type": "command", "command": "echo bash", "timeout_ms": 5000}
                    ]
                }
            ]
        }));
        let canonical = read(&config).unwrap().unwrap();
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
        let config = config_with_hooks(json!({
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
        let canonical = read(&config).unwrap().unwrap();
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
        let config = config_with_hooks(json!({
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
        let canonical = read(&config).unwrap().unwrap();
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
        let a = read(&config_with_hooks(raw.clone())).unwrap().unwrap();
        let b = read(&config_with_hooks(raw)).unwrap().unwrap();
        assert_eq!(a, b);
        // Sorted keys → PreToolUse comes before Stop.
        assert_eq!(a.hooks[0].event.name(), "PreToolUse");
        assert_eq!(a.hooks[1].event.name(), "Stop");
    }
}
