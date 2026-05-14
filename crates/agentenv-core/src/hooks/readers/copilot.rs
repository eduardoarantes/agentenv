//! Read GitHub Copilot hooks directly from
//! `<project>/.github/hooks/*.json` and produce the canonical model.
//!
//! Copilot publishes a repo-local hook spec: any number of `.json` files
//! under `.github/hooks/`, each with the shape
//!
//! ```jsonc
//! {
//!   "version": 1,
//!   "disableAllHooks": false,
//!   "hooks": {
//!     "preToolUse": [
//!       { "type": "command", "bash": "echo hi",
//!         "matcher": "^bash$", "timeoutSec": 10 }
//!     ]
//!   }
//! }
//! ```
//!
//! Event keys arrive in camelCase (`preToolUse`) but Copilot also accepts
//! the VS Code PascalCase aliases (`PreToolUse`). Both forms map to the
//! same [`CommonEvent`] where one exists; anything outside the common-core
//! catalog (e.g. `permissionRequest`, `subagentStart`) round-trips through
//! [`Event::Native`] so the canonical stays lossless.
//!
//! Personal user-level hooks under `~/.copilot/hooks/*.json` are
//! deliberately out of scope for this reader — only the repo-local layout
//! is consumed.

use crate::error::{Error, Result};
use crate::hooks::types::{Action, Canonical, CommonEvent, Event, Hook, Matcher, NativeEvent};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

const SOURCE_NAME: &str = "copilot";

/// Build the canonical by walking `<project_root>/.github/hooks/*.json`.
///
/// Returns `Ok(None)` when:
/// - the `.github/hooks/` directory is absent,
/// - the directory exists but has no `.json` files,
/// - every `.json` file in the directory sets `disableAllHooks: true`, or
/// - the merged result has no hook entries (all entries were malformed or
///   of unrecognised `type`).
///
/// Returns `Err(Error::Config)` on any malformed JSON, with the offending
/// file's path embedded in the error message.
pub fn read(project_root: &Path) -> Result<Option<Canonical>> {
    let hooks_dir = project_root.join(".github").join("hooks");
    let read_dir = match std::fs::read_dir(&hooks_dir) {
        Ok(rd) => rd,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::Io(err)),
    };

    // Collect `.json` files sorted by filename for deterministic merging.
    let mut json_files: Vec<PathBuf> = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(Error::Io)?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("json"))
            .unwrap_or(false)
        {
            json_files.push(path);
        }
    }
    if json_files.is_empty() {
        return Ok(None);
    }
    json_files.sort();

    let mut hooks: Vec<Hook> = Vec::new();
    for path in &json_files {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(Error::Io(err)),
        };
        let trimmed = content.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: Value = serde_json::from_str(trimmed)
            .map_err(|err| Error::Config(format!("failed to parse {}: {err}", path.display())))?;

        // File-level kill switch.
        if parsed
            .get("disableAllHooks")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }

        let Some(events) = parsed.get("hooks").and_then(Value::as_object) else {
            continue;
        };

        // Iterate event keys in sorted order within each file.
        let mut keys: Vec<&String> = events.keys().collect();
        keys.sort();
        for event_name in keys {
            let Some(entries) = events.get(event_name).and_then(Value::as_array) else {
                continue;
            };
            let common = from_copilot(event_name);
            for entry in entries {
                let Value::Object(entry_obj) = entry else {
                    continue;
                };
                if let Some(hook) = parse_entry(event_name, common, entry, entry_obj) {
                    hooks.push(hook);
                }
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

/// Map a Copilot event identifier (camelCase or PascalCase) to the
/// canonical common-core variant, returning `None` for any name outside the
/// common-core catalog.
///
/// Note that Copilot's `userPromptSubmitted` / `UserPromptSubmitted` maps
/// to canonical `UserPromptSubmit` — the spelling differs (Copilot ends in
/// "Submitted", canonical ends in "Submit") but the semantics match.
fn from_copilot(name: &str) -> Option<CommonEvent> {
    match name {
        "sessionStart" | "SessionStart" => Some(CommonEvent::SessionStart),
        "sessionEnd" | "SessionEnd" => Some(CommonEvent::SessionEnd),
        "userPromptSubmitted" | "UserPromptSubmitted" => Some(CommonEvent::UserPromptSubmit),
        "preToolUse" | "PreToolUse" => Some(CommonEvent::PreToolUse),
        "postToolUse" | "PostToolUse" => Some(CommonEvent::PostToolUse),
        "agentStop" | "AgentStop" | "Stop" => Some(CommonEvent::Stop),
        "subagentStop" | "SubagentStop" => Some(CommonEvent::SubagentStop),
        "errorOccurred" | "ErrorOccurred" => Some(CommonEvent::Error),
        "preCompact" | "PreCompact" => Some(CommonEvent::PreCompact),
        "notification" | "Notification" => Some(CommonEvent::Notification),
        _ => None,
    }
}

/// Build a single canonical hook from one Copilot entry. Returns `None`
/// only when the entry is unstructured enough to skip silently (no `type`
/// field, unknown `type` value).
fn parse_entry(
    event_name: &str,
    common: Option<CommonEvent>,
    full_entry: &Value,
    entry_obj: &Map<String, Value>,
) -> Option<Hook> {
    let ty = entry_obj.get("type").and_then(Value::as_str)?;
    let matcher = entry_obj
        .get("matcher")
        .and_then(Value::as_str)
        .map(|s| Matcher {
            tool: Some(s.to_string()),
        });

    match ty {
        "command" => {
            if let Some(action) = parse_command_action(entry_obj) {
                let event = match common {
                    Some(c) => Event::Common(c),
                    None => Event::Native(NativeEvent {
                        source: SOURCE_NAME.to_string(),
                        native_event: event_name.to_string(),
                        payload: full_entry.clone(),
                    }),
                };
                Some(Hook {
                    event,
                    matcher,
                    action,
                })
            } else {
                // `command` entry that has `env`, multiple shell fields, or
                // no shell field at all: preserve verbatim as Native.
                Some(native_hook(event_name, full_entry, matcher))
            }
        },
        // No canonical analogue for http / prompt entries — always Native.
        "http" | "prompt" => Some(native_hook(event_name, full_entry, matcher)),
        // Unknown `type` value: skip the entry (matches the claude-shape
        // reader's lenient behaviour for unrecognised action types).
        _ => None,
    }
}

/// Try to express a Copilot `type: command` entry as a canonical
/// [`Action::Command`]. Returns `None` when the entry doesn't qualify —
/// the caller then preserves the entry as Native.
///
/// Qualification rules:
/// - Exactly one of `bash`, `command`, `powershell` is present.
/// - No `env` field is present (env semantics aren't representable in
///   canonical `Action::Command`).
fn parse_command_action(entry_obj: &Map<String, Value>) -> Option<Action> {
    if entry_obj.contains_key("env") {
        return None;
    }
    let shell_fields: Vec<&str> = ["bash", "command", "powershell"]
        .iter()
        .copied()
        .filter(|k| entry_obj.contains_key(*k))
        .collect();
    if shell_fields.len() != 1 {
        return None;
    }
    let command = entry_obj.get(shell_fields[0]).and_then(Value::as_str)?;
    let timeout_ms = entry_obj
        .get("timeoutSec")
        .and_then(Value::as_u64)
        .map(|s| s.saturating_mul(1000));
    let cwd = entry_obj
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from);
    Some(Action::Command {
        command: command.to_string(),
        timeout_ms,
        cwd,
    })
}

/// Wrap a verbatim entry in a Native event with a synthetic empty
/// `Action::Command` (the same sentinel the claude-shape reader uses when
/// it can't extract a canonical action).
fn native_hook(event_name: &str, full_entry: &Value, matcher: Option<Matcher>) -> Hook {
    Hook {
        event: Event::Native(NativeEvent {
            source: SOURCE_NAME.to_string(),
            native_event: event_name.to_string(),
            payload: full_entry.clone(),
        }),
        matcher,
        action: Action::Command {
            command: String::new(),
            timeout_ms: None,
            cwd: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    /// Build a project with `.github/hooks/<filename>` set to `body`.
    fn project_with_file(filename: &str, body: Value) -> TempDir {
        let project = TempDir::new().unwrap();
        let dir = project.path().join(".github").join("hooks");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(filename), body.to_string()).unwrap();
        project
    }

    #[test]
    fn returns_none_when_hooks_dir_missing() {
        // No `.github/` at all.
        let project = TempDir::new().unwrap();
        assert!(read(project.path()).unwrap().is_none());

        // `.github/` present but `.github/hooks/` missing.
        let project = TempDir::new().unwrap();
        std::fs::create_dir_all(project.path().join(".github")).unwrap();
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn returns_none_when_hooks_dir_empty() {
        let project = TempDir::new().unwrap();
        std::fs::create_dir_all(project.path().join(".github").join("hooks")).unwrap();
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn returns_none_when_only_non_json_files() {
        let project = TempDir::new().unwrap();
        let dir = project.path().join(".github").join("hooks");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), "# notes\n").unwrap();
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn returns_none_when_disable_all_hooks_true() {
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "disableAllHooks": true,
                "hooks": {
                    "preToolUse": [
                        {"type": "command", "bash": "echo skipped"}
                    ]
                }
            }),
        );
        assert!(read(project.path()).unwrap().is_none());
    }

    #[test]
    fn parses_simple_command_pretooluse_with_matcher() {
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "preToolUse": [
                        {
                            "type": "command",
                            "bash": "echo hi",
                            "matcher": "^bash$",
                            "timeoutSec": 10
                        }
                    ]
                }
            }),
        );
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.source, "copilot");
        assert_eq!(canonical.hooks.len(), 1);
        let hook = &canonical.hooks[0];
        assert!(matches!(hook.event, Event::Common(CommonEvent::PreToolUse)));
        assert_eq!(
            hook.matcher.as_ref().unwrap().tool.as_deref(),
            Some("^bash$")
        );
        let Action::Command {
            command,
            timeout_ms,
            cwd,
        } = &hook.action;
        assert_eq!(command, "echo hi");
        assert_eq!(*timeout_ms, Some(10_000));
        assert_eq!(*cwd, None);
    }

    #[test]
    fn pascalcase_event_alias_maps_to_common() {
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "PreToolUse": [
                        {"type": "command", "bash": "echo hi"}
                    ]
                }
            }),
        );
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 1);
        assert!(matches!(
            canonical.hooks[0].event,
            Event::Common(CommonEvent::PreToolUse)
        ));
    }

    #[test]
    fn usersubmitted_event_maps_to_userpromptsubmit_common() {
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "userPromptSubmitted": [
                        {"type": "command", "bash": "echo prompt"}
                    ]
                }
            }),
        );
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 1);
        assert!(matches!(
            canonical.hooks[0].event,
            Event::Common(CommonEvent::UserPromptSubmit)
        ));
    }

    #[test]
    fn command_with_env_preserved_as_native() {
        let entry = json!({
            "type": "command",
            "bash": "echo hi",
            "env": {"FOO": "bar"}
        });
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "preToolUse": [entry.clone()]
                }
            }),
        );
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 1);
        let Event::Native(n) = &canonical.hooks[0].event else {
            panic!("expected Native, got {:?}", canonical.hooks[0].event);
        };
        assert_eq!(n.source, "copilot");
        assert_eq!(n.native_event, "preToolUse");
        assert_eq!(n.payload, entry);
        // Payload must preserve the env field.
        assert_eq!(n.payload["env"], json!({"FOO": "bar"}));
    }

    #[test]
    fn command_with_both_bash_and_powershell_preserved_as_native() {
        let entry = json!({
            "type": "command",
            "bash": "echo unix",
            "powershell": "Write-Host win"
        });
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "preToolUse": [entry.clone()]
                }
            }),
        );
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 1);
        let Event::Native(n) = &canonical.hooks[0].event else {
            panic!("expected Native");
        };
        assert_eq!(n.payload, entry);
    }

    #[test]
    fn http_entry_preserved_as_native() {
        let entry = json!({
            "type": "http",
            "url": "https://example.com/hook",
            "matcher": ".*"
        });
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "postToolUse": [entry.clone()]
                }
            }),
        );
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 1);
        let Event::Native(n) = &canonical.hooks[0].event else {
            panic!("expected Native");
        };
        assert_eq!(n.payload["url"], "https://example.com/hook");
        // Matcher should still be carried verbatim alongside the Native event.
        assert_eq!(
            canonical.hooks[0].matcher.as_ref().unwrap().tool.as_deref(),
            Some(".*")
        );
    }

    #[test]
    fn prompt_entry_preserved_as_native() {
        let entry = json!({
            "type": "prompt",
            "prompt": "/help"
        });
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "sessionStart": [entry.clone()]
                }
            }),
        );
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 1);
        let Event::Native(n) = &canonical.hooks[0].event else {
            panic!("expected Native");
        };
        assert_eq!(n.payload, entry);
    }

    #[test]
    fn unknown_event_name_preserved_as_native() {
        let entry = json!({
            "type": "command",
            "bash": "echo perm"
        });
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "permissionRequest": [entry.clone()]
                }
            }),
        );
        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 1);
        let Event::Native(n) = &canonical.hooks[0].event else {
            panic!("expected Native");
        };
        assert_eq!(n.source, "copilot");
        assert_eq!(n.native_event, "permissionRequest");
        // Even though Native, the command DID qualify for Action::Command.
        let Action::Command { command, .. } = &canonical.hooks[0].action;
        assert_eq!(command, "echo perm");
    }

    #[test]
    fn merges_multiple_files_in_filename_sorted_order() {
        let project = TempDir::new().unwrap();
        let dir = project.path().join(".github").join("hooks");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.json"),
            json!({
                "version": 1,
                "hooks": {
                    "preToolUse": [
                        {"type": "command", "bash": "from-a"}
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.join("b.json"),
            json!({
                "version": 1,
                "hooks": {
                    "postToolUse": [
                        {"type": "command", "bash": "from-b"}
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();

        let canonical = read(project.path()).unwrap().unwrap();
        assert_eq!(canonical.hooks.len(), 2);
        let Action::Command { command: cmd0, .. } = &canonical.hooks[0].action;
        let Action::Command { command: cmd1, .. } = &canonical.hooks[1].action;
        assert_eq!(cmd0, "from-a");
        assert_eq!(cmd1, "from-b");
    }

    #[test]
    fn malformed_json_is_a_config_error() {
        let project = TempDir::new().unwrap();
        let dir = project.path().join(".github").join("hooks");
        std::fs::create_dir_all(&dir).unwrap();
        let bad_path = dir.join("broken.json");
        std::fs::write(&bad_path, "{ this is not json").unwrap();
        let err = read(project.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("failed to parse"), "got: {msg}");
        assert!(
            msg.contains("broken.json"),
            "error must name the offending file, got: {msg}"
        );
    }

    #[test]
    fn cwd_field_round_trips() {
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "preToolUse": [
                        {
                            "type": "command",
                            "bash": "echo cwd",
                            "cwd": "/abs/path"
                        }
                    ]
                }
            }),
        );
        let canonical = read(project.path()).unwrap().unwrap();
        let Action::Command { cwd, .. } = &canonical.hooks[0].action;
        assert_eq!(cwd.as_deref(), Some(std::path::Path::new("/abs/path")));
    }

    #[test]
    fn timeoutsec_converts_to_milliseconds() {
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "preToolUse": [
                        {
                            "type": "command",
                            "bash": "echo t",
                            "timeoutSec": 5
                        }
                    ]
                }
            }),
        );
        let canonical = read(project.path()).unwrap().unwrap();
        let Action::Command { timeout_ms, .. } = &canonical.hooks[0].action;
        assert_eq!(*timeout_ms, Some(5_000));
    }

    #[test]
    fn entry_without_type_field_is_skipped() {
        let project = project_with_file(
            "repo.json",
            json!({
                "version": 1,
                "hooks": {
                    "preToolUse": [
                        {"bash": "echo no-type"}
                    ]
                }
            }),
        );
        // Sole entry was skipped → nothing to emit → Ok(None).
        assert!(read(project.path()).unwrap().is_none());
    }
}
