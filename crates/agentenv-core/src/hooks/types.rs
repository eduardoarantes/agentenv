//! Canonical hook types — the agentenv-internal domain model.
//!
//! Hooks are read losslessly from one source target's native config (today
//! Claude Code's `settings.json`), serialized to
//! `.agentenv/hooks.canonical.yaml`, and rendered out to every other
//! supporting target. The `Event::Native` escape hatch preserves anything
//! the source emits that does not fit a common-core variant, so the
//! source → canonical step never drops information.
//!
//! See `docs/HOOKS.md` for the user-facing spec.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level shape of `.agentenv/hooks.canonical.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Canonical {
    /// Name of the source target this canonical was derived from (echoes
    /// the `source` field in `.agentrc.yaml`).
    pub source: String,
    /// One entry per hook, in stable order.
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

/// A single canonical hook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hook {
    pub event: Event,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<Matcher>,
    pub action: Action,
}

/// Canonical event identifier.
///
/// Common variants serialize as bare PascalCase strings (`event: PreToolUse`).
/// The `Native` variant — the lossless-read escape hatch — serializes as an
/// object preserving the source-tool's verbatim native event name and JSON
/// payload, so anything the source emits round-trips through the canonical
/// model without information loss.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Event {
    Common(CommonEvent),
    Native(NativeEvent),
}

/// Common-core events with cross-target counterparts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommonEvent {
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    Stop,
    SubagentStop,
    Error,
    PreCompact,
    Notification,
}

/// Source-tool-native event preserved verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeEvent {
    /// Target name the event came from (e.g. `claude-code`).
    pub source: String,
    /// Verbatim native event identifier as it appears in the source file.
    pub native_event: String,
    /// Verbatim source-tool JSON payload for the event entry.
    pub payload: serde_json::Value,
}

impl Event {
    /// Display name — PascalCase for common events, the original native
    /// identifier for `Native`.
    pub fn name(&self) -> &str {
        match self {
            Event::Common(c) => c.name(),
            Event::Native(n) => &n.native_event,
        }
    }
}

impl CommonEvent {
    /// PascalCase identifier used in the canonical YAML and (for Claude /
    /// Cursor) in the rendered native config.
    pub fn name(&self) -> &'static str {
        match self {
            CommonEvent::SessionStart => "SessionStart",
            CommonEvent::SessionEnd => "SessionEnd",
            CommonEvent::UserPromptSubmit => "UserPromptSubmit",
            CommonEvent::PreToolUse => "PreToolUse",
            CommonEvent::PostToolUse => "PostToolUse",
            CommonEvent::Stop => "Stop",
            CommonEvent::SubagentStop => "SubagentStop",
            CommonEvent::Error => "Error",
            CommonEvent::PreCompact => "PreCompact",
            CommonEvent::Notification => "Notification",
        }
    }

    /// Parse a PascalCase Claude/Cursor event name back to a common variant,
    /// or `None` if the name is not in the common-core catalog.
    pub fn from_pascal(name: &str) -> Option<Self> {
        match name {
            "SessionStart" => Some(Self::SessionStart),
            "SessionEnd" => Some(Self::SessionEnd),
            "UserPromptSubmit" => Some(Self::UserPromptSubmit),
            "PreToolUse" => Some(Self::PreToolUse),
            "PostToolUse" => Some(Self::PostToolUse),
            "Stop" => Some(Self::Stop),
            "SubagentStop" => Some(Self::SubagentStop),
            "Error" => Some(Self::Error),
            "PreCompact" => Some(Self::PreCompact),
            "Notification" => Some(Self::Notification),
            _ => None,
        }
    }
}

/// Optional event filter. Today only `tool` is modeled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Matcher {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

/// Hook action. Only `command` is documented across all targets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Action {
    Command {
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
    },
}

/// Report returned by a writer: which canonical hooks could not be rendered
/// for this target.
#[derive(Debug, Default, Clone)]
pub struct WriteReport {
    /// Human-readable reasons one canonical hook was dropped.
    pub drops: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_yaml_round_trips_common_event() {
        let canonical = Canonical {
            source: "claude-code".to_string(),
            hooks: vec![Hook {
                event: Event::Common(CommonEvent::PreToolUse),
                matcher: Some(Matcher {
                    tool: Some("Bash".to_string()),
                }),
                action: Action::Command {
                    command: "scripts/audit.sh".to_string(),
                    timeout_ms: Some(5000),
                    cwd: None,
                },
            }],
        };
        let yaml = serde_yaml::to_string(&canonical).unwrap();
        let parsed: Canonical = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(canonical, parsed);
        // The string-form serialization is what we promise users.
        assert!(yaml.contains("event: PreToolUse"));
    }

    #[test]
    fn canonical_yaml_round_trips_native_event() {
        let canonical = Canonical {
            source: "claude-code".to_string(),
            hooks: vec![Hook {
                event: Event::Native(NativeEvent {
                    source: "claude-code".to_string(),
                    native_event: "PreCompact".to_string(),
                    payload: serde_json::json!({"matcher": ".*"}),
                }),
                matcher: None,
                action: Action::Command {
                    command: "echo compact".to_string(),
                    timeout_ms: None,
                    cwd: None,
                },
            }],
        };
        let yaml = serde_yaml::to_string(&canonical).unwrap();
        let parsed: Canonical = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(canonical, parsed);
    }

    #[test]
    fn event_name_returns_pascal_for_common_and_raw_for_native() {
        assert_eq!(Event::Common(CommonEvent::PreToolUse).name(), "PreToolUse");
        let native = Event::Native(NativeEvent {
            source: "cursor".to_string(),
            native_event: "afterFileEdit".to_string(),
            payload: serde_json::Value::Null,
        });
        assert_eq!(native.name(), "afterFileEdit");
    }

    #[test]
    fn common_event_from_pascal_round_trips() {
        for variant in [
            CommonEvent::SessionStart,
            CommonEvent::PreToolUse,
            CommonEvent::Stop,
            CommonEvent::SubagentStop,
            CommonEvent::Notification,
        ] {
            assert_eq!(CommonEvent::from_pascal(variant.name()), Some(variant));
        }
        assert_eq!(CommonEvent::from_pascal("NotAnEvent"), None);
    }
}
