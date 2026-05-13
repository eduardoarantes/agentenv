//! Cursor writer: emit `.cursor/hooks.json` from the canonical model.
//!
//! `.cursor/hooks.json` is single-purpose (hooks only), so agentenv treats
//! it as a **fully managed file**. The marker is the top-level
//! `"_agentenv": "managed"` field — if the file exists without that field,
//! the writer refuses to clobber it.
//!
//! Cursor's native hook shape mirrors Claude's, so the common-core events
//! pass through 1:1. Events with no Cursor counterpart (e.g. `PreCompact`,
//! `Notification`, `Error`) are dropped with a [`WriteReport`] entry.

use crate::error::{Error, Result};
use crate::hooks::types::{Action, Canonical, CommonEvent, Event, WriteReport};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// Marker key written at the top level of `.cursor/hooks.json` so the next
/// sync recognises its own output.
pub const AGENTENV_MARKER_KEY: &str = "_agentenv";
const AGENTENV_MARKER_VALUE: &str = "managed";

const RELATIVE_DEST: &str = ".cursor/hooks.json";

/// Path the cursor hooks file lives at for a given project root.
pub fn destination(project_root: &Path) -> PathBuf {
    project_root.join(RELATIVE_DEST)
}

/// Native event names Cursor accepts (Claude-shaped).
fn supports(common: &CommonEvent) -> bool {
    matches!(
        common,
        CommonEvent::SessionStart
            | CommonEvent::SessionEnd
            | CommonEvent::UserPromptSubmit
            | CommonEvent::PreToolUse
            | CommonEvent::PostToolUse
            | CommonEvent::Stop
    )
}

/// Render the canonical model and write it to `.cursor/hooks.json`.
pub fn write(canonical: &Canonical, project_root: &Path) -> Result<WriteReport> {
    let dest = destination(project_root);
    detect_conflict(&dest)?;

    let mut report = WriteReport::default();
    let mut events: Map<String, Value> = Map::new();

    for hook in &canonical.hooks {
        let Some(name) = native_event_name(&hook.event, &mut report) else {
            continue;
        };
        let action_obj = render_action(&hook.action);
        let matcher_value = hook
            .matcher
            .as_ref()
            .and_then(|m| m.tool.clone())
            .unwrap_or_else(|| ".*".to_string());

        // Append into the per-event list, grouped by matcher value.
        let event_entry = events
            .entry(name.to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let arr = event_entry.as_array_mut().expect("array");
        // Try to extend an existing matcher block with the same matcher
        // string; otherwise push a new one.
        let mut placed = false;
        for entry in arr.iter_mut() {
            let m = entry.get("matcher").and_then(Value::as_str);
            if m == Some(matcher_value.as_str()) {
                if let Some(Value::Array(inner)) = entry.get_mut("hooks") {
                    inner.push(action_obj.clone());
                    placed = true;
                    break;
                }
            }
        }
        if !placed {
            arr.push(json!({
                "matcher": matcher_value,
                "hooks": [action_obj],
            }));
        }
    }

    let mut top = Map::new();
    top.insert(
        AGENTENV_MARKER_KEY.to_string(),
        Value::String(AGENTENV_MARKER_VALUE.to_string()),
    );
    top.insert("hooks".to_string(), Value::Object(events));

    let content = serde_json::to_string_pretty(&Value::Object(top))
        .map_err(|err| Error::Config(format!("failed to render .cursor/hooks.json: {err}")))?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&dest, content)?;
    Ok(report)
}

/// Inspect the destination file. Refuse with [`Error::Config`] if it exists
/// and was not previously written by agentenv (no marker field).
pub fn detect_conflict(dest: &Path) -> Result<()> {
    if !dest.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(dest)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let parsed: Value = serde_json::from_str(trimmed).map_err(|err| {
        Error::Config(format!(
            "{} already exists and is not valid JSON ({err}); agentenv refuses to overwrite \
             arbitrary content. Move it aside or set `source` to a different target.",
            dest.display()
        ))
    })?;
    let marker = parsed.get(AGENTENV_MARKER_KEY).and_then(Value::as_str);
    if marker == Some(AGENTENV_MARKER_VALUE) {
        return Ok(());
    }
    let has_hooks = parsed
        .get("hooks")
        .map(|h| h.is_object() && !h.as_object().unwrap().is_empty())
        .unwrap_or(false);
    if has_hooks {
        return Err(Error::Config(format!(
            "{} already contains user-authored hooks. agentenv refuses to overwrite. Either \
             remove these hooks or set `source` to a different target.",
            dest.display()
        )));
    }
    // File exists, is JSON, has no hooks and no marker — could be a stub.
    // Refuse to be safe — the user clearly put something there.
    Err(Error::Config(format!(
        "{} exists but is not an agentenv-managed hooks file. Remove or move it before re-syncing.",
        dest.display()
    )))
}

fn native_event_name(event: &Event, report: &mut WriteReport) -> Option<String> {
    match event {
        Event::Common(c) => {
            if supports(c) {
                Some(c.name().to_string())
            } else {
                report.drops.push(format!(
                    "cursor: dropping `{}` (no Cursor-native counterpart)",
                    c.name()
                ));
                None
            }
        },
        Event::Native(native) => {
            // Pass through native events that originated in claude-code,
            // since Cursor docs claim Claude-shaped hooks are accepted.
            if native.source == "claude-code" {
                Some(native.native_event.clone())
            } else {
                report.drops.push(format!(
                    "cursor: dropping native event `{}` from `{}` (not Claude-shaped)",
                    native.native_event, native.source
                ));
                None
            }
        },
    }
}

fn render_action(action: &Action) -> Value {
    let Action::Command {
        command,
        timeout_ms,
        cwd,
    } = action;
    let mut obj = Map::new();
    obj.insert("type".to_string(), Value::String("command".to_string()));
    obj.insert("command".to_string(), Value::String(command.clone()));
    if let Some(t) = timeout_ms {
        obj.insert("timeout_ms".to_string(), json!(*t));
    }
    if let Some(c) = cwd {
        obj.insert(
            "cwd".to_string(),
            Value::String(c.to_string_lossy().into_owned()),
        );
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::types::{Action, CommonEvent, Event, Hook, Matcher, NativeEvent};
    use tempfile::TempDir;

    fn canonical_with(hooks: Vec<Hook>) -> Canonical {
        Canonical {
            source: "claude-code".to_string(),
            hooks,
        }
    }

    fn cmd(s: &str) -> Action {
        Action::Command {
            command: s.to_string(),
            timeout_ms: None,
            cwd: None,
        }
    }

    #[test]
    fn writes_pretooluse_with_matcher() {
        let project = TempDir::new().unwrap();
        let canonical = canonical_with(vec![Hook {
            event: Event::Common(CommonEvent::PreToolUse),
            matcher: Some(Matcher {
                tool: Some("Bash".to_string()),
            }),
            action: cmd("echo bash"),
        }]);
        let report = write(&canonical, project.path()).unwrap();
        assert!(report.drops.is_empty());

        let raw = fs::read_to_string(destination(project.path())).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v[AGENTENV_MARKER_KEY], "managed");
        let entry = &v["hooks"]["PreToolUse"][0];
        assert_eq!(entry["matcher"], "Bash");
        assert_eq!(entry["hooks"][0]["command"], "echo bash");
    }

    #[test]
    fn drops_pre_compact_with_warning() {
        let project = TempDir::new().unwrap();
        let canonical = canonical_with(vec![Hook {
            event: Event::Common(CommonEvent::PreCompact),
            matcher: None,
            action: cmd("echo compact"),
        }]);
        let report = write(&canonical, project.path()).unwrap();
        assert_eq!(report.drops.len(), 1);
        assert!(report.drops[0].contains("PreCompact"));
        let raw = fs::read_to_string(destination(project.path())).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert!(v["hooks"].as_object().unwrap().is_empty());
    }

    #[test]
    fn passes_through_native_event_from_claude_source() {
        let project = TempDir::new().unwrap();
        let canonical = canonical_with(vec![Hook {
            event: Event::Native(NativeEvent {
                source: "claude-code".to_string(),
                native_event: "TeammateIdle".to_string(),
                payload: serde_json::Value::Null,
            }),
            matcher: None,
            action: cmd("echo idle"),
        }]);
        let report = write(&canonical, project.path()).unwrap();
        assert!(report.drops.is_empty(), "drops: {:?}", report.drops);
        let raw = fs::read_to_string(destination(project.path())).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert!(v["hooks"]["TeammateIdle"].is_array());
    }

    #[test]
    fn groups_hooks_under_same_matcher() {
        let project = TempDir::new().unwrap();
        let canonical = canonical_with(vec![
            Hook {
                event: Event::Common(CommonEvent::PreToolUse),
                matcher: Some(Matcher {
                    tool: Some("Bash".to_string()),
                }),
                action: cmd("a"),
            },
            Hook {
                event: Event::Common(CommonEvent::PreToolUse),
                matcher: Some(Matcher {
                    tool: Some("Bash".to_string()),
                }),
                action: cmd("b"),
            },
        ]);
        write(&canonical, project.path()).unwrap();
        let raw = fs::read_to_string(destination(project.path())).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        let entries = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "same matcher should group");
        assert_eq!(entries[0]["hooks"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn refuses_user_authored_file() {
        let project = TempDir::new().unwrap();
        let dest = destination(project.path());
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        // File without our marker but with hooks → must refuse.
        fs::write(
            &dest,
            r#"{ "hooks": { "PreToolUse": [{"matcher": "Bash", "hooks": []}] } }"#,
        )
        .unwrap();
        let canonical = canonical_with(vec![Hook {
            event: Event::Common(CommonEvent::PreToolUse),
            matcher: None,
            action: cmd("would-overwrite"),
        }]);
        let err = write(&canonical, project.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("user-authored"), "got: {msg}");
        // File preserved.
        let after = fs::read_to_string(&dest).unwrap();
        assert!(after.contains("PreToolUse"));
        assert!(!after.contains(AGENTENV_MARKER_KEY));
    }

    #[test]
    fn idempotent_overwrites_own_output() {
        let project = TempDir::new().unwrap();
        let canonical = canonical_with(vec![Hook {
            event: Event::Common(CommonEvent::Stop),
            matcher: None,
            action: cmd("notify"),
        }]);
        write(&canonical, project.path()).unwrap();
        // Second write must succeed — marker recognises our own output.
        write(&canonical, project.path()).unwrap();
    }
}
