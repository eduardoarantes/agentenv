//! Default target configurations.
//!
//! Paths come from `docs/platform-standards.md` §8. Skills follow the
//! cross-tool [Agent Skills](https://agentskills.io) convention; subagents
//! and slash-commands have per-tool conventions that diverge in format
//! (Markdown vs TOML) and in directory layout.
//!
//! Defaults are project-scoped: the install paths are relative to the
//! workspace root. Users who want user-scoped installs override per target
//! in their `.agentrc.yaml`.

use crate::config::{SourceMapping, TargetConfig};
use std::collections::HashMap;
use std::path::PathBuf;

/// Default target configurations.
pub struct TargetDefaults;

/// Build a single capability mapping at `target_path`.
fn mapping(target_path: &str) -> Vec<SourceMapping> {
    vec![SourceMapping {
        target: PathBuf::from(target_path),
        mode: "symlink".to_string(),
    }]
}

/// Pack `(capability, target_path)` pairs into a source-mappings map.
fn capability_mappings(entries: &[(&str, &str)]) -> HashMap<String, Vec<SourceMapping>> {
    let mut mappings = HashMap::new();
    for (capability, path) in entries {
        mappings.insert((*capability).to_string(), mapping(path));
    }
    mappings
}

impl TargetDefaults {
    /// Claude Code — `<scope>/.claude/`. Skills follow the cross-tool standard;
    /// commands are the legacy form Claude Code still accepts; subagents are
    /// Markdown with YAML frontmatter.
    pub fn claude_code() -> TargetConfig {
        TargetConfig {
            r#type: "claude-code".to_string(),
            tools: vec!["claude-code".to_string()],
            paths: HashMap::new(),
            source_mappings: capability_mappings(&[
                ("skills", ".claude/skills"),
                ("commands", ".claude/commands"),
                ("agents", ".claude/agents"),
            ]),
        }
    }

    /// OpenAI Codex — skills under `.agents/skills/`, subagents under
    /// `.codex/agents/` (TOML, not Markdown — plugins must ship `.toml` files).
    pub fn codex() -> TargetConfig {
        TargetConfig {
            r#type: "codex".to_string(),
            tools: vec!["codex".to_string()],
            paths: HashMap::new(),
            source_mappings: capability_mappings(&[
                ("skills", ".agents/skills"),
                ("agents", ".codex/agents"),
            ]),
        }
    }

    /// Cursor — `<scope>/.cursor/`. Skills, subagents both Markdown.
    pub fn cursor() -> TargetConfig {
        TargetConfig {
            r#type: "cursor".to_string(),
            tools: vec!["cursor".to_string()],
            paths: HashMap::new(),
            source_mappings: capability_mappings(&[
                ("skills", ".cursor/skills"),
                ("agents", ".cursor/agents"),
            ]),
        }
    }

    /// GitHub Copilot — `.github/`. Subagent files use the `.agent.md` suffix
    /// rather than bare `.md`; plugin authors must name leaves accordingly.
    pub fn copilot() -> TargetConfig {
        TargetConfig {
            r#type: "copilot".to_string(),
            tools: vec!["copilot".to_string()],
            paths: HashMap::new(),
            source_mappings: capability_mappings(&[
                ("skills", ".github/skills"),
                ("commands", ".github/prompts"),
                ("agents", ".github/agents"),
            ]),
        }
    }

    /// Gemini CLI — `<scope>/.gemini/`. Commands are TOML, not Markdown.
    pub fn gemini_cli() -> TargetConfig {
        TargetConfig {
            r#type: "gemini-cli".to_string(),
            tools: vec!["gemini-cli".to_string()],
            paths: HashMap::new(),
            source_mappings: capability_mappings(&[
                ("skills", ".gemini/skills"),
                ("commands", ".gemini/commands"),
                ("agents", ".gemini/agents"),
            ]),
        }
    }

    /// JetBrains Junie — `<scope>/.junie/`. Markdown subagents.
    pub fn junie() -> TargetConfig {
        TargetConfig {
            r#type: "junie".to_string(),
            tools: vec!["junie".to_string()],
            paths: HashMap::new(),
            source_mappings: capability_mappings(&[
                ("skills", ".junie/skills"),
                ("agents", ".junie/agents"),
            ]),
        }
    }

    /// Google Antigravity — skills under `.agent/skills/` (singular!),
    /// commands under `.agents/workflows/` (plural — different concept).
    /// Subagents are not directory-based here, so `agents` is intentionally
    /// not mapped.
    pub fn antigravity() -> TargetConfig {
        TargetConfig {
            r#type: "antigravity".to_string(),
            tools: vec!["antigravity".to_string()],
            paths: HashMap::new(),
            source_mappings: capability_mappings(&[
                ("skills", ".agent/skills"),
                ("commands", ".agents/workflows"),
            ]),
        }
    }

    /// Get default configuration by name.
    pub fn get(name: &str) -> Option<TargetConfig> {
        match name {
            "claude-code" => Some(Self::claude_code()),
            "codex" => Some(Self::codex()),
            "cursor" => Some(Self::cursor()),
            "copilot" => Some(Self::copilot()),
            "gemini-cli" => Some(Self::gemini_cli()),
            "junie" => Some(Self::junie()),
            "antigravity" => Some(Self::antigravity()),
            _ => None,
        }
    }

    /// All target names with built-in defaults.
    pub fn available_targets() -> Vec<&'static str> {
        vec![
            "claude-code",
            "codex",
            "cursor",
            "copilot",
            "gemini-cli",
            "junie",
            "antigravity",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_maps_skills_commands_and_agents() {
        let config = TargetDefaults::claude_code();
        assert_eq!(
            config.source_mappings.get("skills").unwrap()[0]
                .target
                .to_string_lossy(),
            ".claude/skills"
        );
        assert!(config.source_mappings.contains_key("commands"));
        assert!(config.source_mappings.contains_key("agents"));
    }

    #[test]
    fn codex_skills_use_dot_agents_alias() {
        let config = TargetDefaults::codex();
        assert_eq!(
            config.source_mappings.get("skills").unwrap()[0]
                .target
                .to_string_lossy(),
            ".agents/skills"
        );
        assert_eq!(
            config.source_mappings.get("agents").unwrap()[0]
                .target
                .to_string_lossy(),
            ".codex/agents"
        );
        assert!(!config.source_mappings.contains_key("commands"));
    }

    #[test]
    fn antigravity_uses_singular_dot_agent_for_skills() {
        let config = TargetDefaults::antigravity();
        assert_eq!(
            config.source_mappings.get("skills").unwrap()[0]
                .target
                .to_string_lossy(),
            ".agent/skills"
        );
        assert_eq!(
            config.source_mappings.get("commands").unwrap()[0]
                .target
                .to_string_lossy(),
            ".agents/workflows"
        );
        assert!(!config.source_mappings.contains_key("agents"));
    }

    #[test]
    fn copilot_commands_route_to_prompts_directory() {
        let config = TargetDefaults::copilot();
        assert_eq!(
            config.source_mappings.get("commands").unwrap()[0]
                .target
                .to_string_lossy(),
            ".github/prompts"
        );
    }

    #[test]
    fn defaults_are_symlink_mode() {
        for name in TargetDefaults::available_targets() {
            let config = TargetDefaults::get(name).unwrap();
            for mappings in config.source_mappings.values() {
                for mapping in mappings {
                    assert_eq!(mapping.mode, "symlink", "{name} mapping not symlink");
                }
            }
        }
    }

    #[test]
    fn available_targets_covers_every_get() {
        for name in TargetDefaults::available_targets() {
            assert!(TargetDefaults::get(name).is_some(), "missing {name}");
        }
        assert!(TargetDefaults::get("jetbrains").is_none());
        assert!(TargetDefaults::get("unknown").is_none());
    }
}
