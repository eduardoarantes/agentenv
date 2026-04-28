//! Default target configurations

use crate::config::{SourceMapping, TargetConfig};
use std::collections::HashMap;
use std::path::PathBuf;

/// Default target configurations
pub struct TargetDefaults;

impl TargetDefaults {
    /// Get default configuration for Claude Code
    pub fn claude_code() -> TargetConfig {
        let mut source_mappings = HashMap::new();

        // Skills mappings
        let skills_mapping = vec![SourceMapping {
            source: PathBuf::from("~/.agentenv/marketplace/skills"),
            target: PathBuf::from(".claude-code/skills"),
            mode: "symlink".to_string(),
        }];
        source_mappings.insert("skills".to_string(), skills_mapping);

        // Commands mappings
        let commands_mapping = vec![SourceMapping {
            source: PathBuf::from("~/.agentenv/marketplace/commands"),
            target: PathBuf::from(".claude-code/commands"),
            mode: "symlink".to_string(),
        }];
        source_mappings.insert("commands".to_string(), commands_mapping);

        // Agents mappings
        let agents_mapping = vec![SourceMapping {
            source: PathBuf::from("~/.agentenv/marketplace/agents"),
            target: PathBuf::from(".claude-code/agents"),
            mode: "symlink".to_string(),
        }];
        source_mappings.insert("agents".to_string(), agents_mapping);

        let mut paths = HashMap::new();
        paths.insert(
            "config".to_string(),
            "~/.vscode/extensions/github.claude-code".to_string(),
        );

        TargetConfig {
            r#type: "vscode-extension".to_string(),
            tools: vec!["claude-code".to_string()],
            paths,
            source_mappings,
        }
    }

    /// Get default configuration for Cursor
    pub fn cursor() -> TargetConfig {
        let mut source_mappings = HashMap::new();

        // Skills mappings
        let skills_mapping = vec![SourceMapping {
            source: PathBuf::from("~/.agentenv/marketplace/skills"),
            target: PathBuf::from("extensions/skills"),
            mode: "symlink".to_string(),
        }];
        source_mappings.insert("skills".to_string(), skills_mapping);

        // Commands mappings
        let commands_mapping = vec![SourceMapping {
            source: PathBuf::from("~/.agentenv/marketplace/commands"),
            target: PathBuf::from("extensions/commands"),
            mode: "symlink".to_string(),
        }];
        source_mappings.insert("commands".to_string(), commands_mapping);

        // Agents mappings
        let agents_mapping = vec![SourceMapping {
            source: PathBuf::from("~/.agentenv/marketplace/agents"),
            target: PathBuf::from("extensions/agents"),
            mode: "symlink".to_string(),
        }];
        source_mappings.insert("agents".to_string(), agents_mapping);

        let mut paths = HashMap::new();
        paths.insert("config".to_string(), "~/.cursor/extensions".to_string());

        TargetConfig {
            r#type: "cursor-extension".to_string(),
            tools: vec!["cursor".to_string()],
            paths,
            source_mappings,
        }
    }

    /// Get default configuration for JetBrains
    pub fn jetbrains() -> TargetConfig {
        let mut source_mappings = HashMap::new();

        // Skills mappings
        let skills_mapping = vec![SourceMapping {
            source: PathBuf::from("~/.agentenv/marketplace/skills"),
            target: PathBuf::from("plugins/agentenv/skills"),
            mode: "symlink".to_string(),
        }];
        source_mappings.insert("skills".to_string(), skills_mapping);

        // Commands mappings
        let commands_mapping = vec![SourceMapping {
            source: PathBuf::from("~/.agentenv/marketplace/commands"),
            target: PathBuf::from("plugins/agentenv/commands"),
            mode: "symlink".to_string(),
        }];
        source_mappings.insert("commands".to_string(), commands_mapping);

        // Agents mappings
        let agents_mapping = vec![SourceMapping {
            source: PathBuf::from("~/.agentenv/marketplace/agents"),
            target: PathBuf::from("plugins/agentenv/agents"),
            mode: "symlink".to_string(),
        }];
        source_mappings.insert("agents".to_string(), agents_mapping);

        let mut paths = HashMap::new();
        paths.insert("config".to_string(), "~/.config/JetBrains".to_string());

        TargetConfig {
            r#type: "jetbrains-plugin".to_string(),
            tools: vec!["jetbrains".to_string()],
            paths,
            source_mappings,
        }
    }

    /// Get default configuration by name
    pub fn get(name: &str) -> Option<TargetConfig> {
        match name {
            "claude-code" => Some(Self::claude_code()),
            "cursor" => Some(Self::cursor()),
            "jetbrains" => Some(Self::jetbrains()),
            _ => None,
        }
    }

    /// Get all available default target names
    pub fn available_targets() -> Vec<&'static str> {
        vec!["claude-code", "cursor", "jetbrains"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_code_default_config() {
        let config = TargetDefaults::claude_code();
        assert_eq!(config.r#type, "vscode-extension");
        assert!(config.tools.contains(&"claude-code".to_string()));
        assert!(config.source_mappings.contains_key("skills"));
        assert!(config.source_mappings.contains_key("commands"));
        assert!(config.source_mappings.contains_key("agents"));
    }

    #[test]
    fn test_cursor_default_config() {
        let config = TargetDefaults::cursor();
        assert_eq!(config.r#type, "cursor-extension");
        assert!(config.tools.contains(&"cursor".to_string()));
        assert!(config.source_mappings.contains_key("skills"));
    }

    #[test]
    fn test_jetbrains_default_config() {
        let config = TargetDefaults::jetbrains();
        assert_eq!(config.r#type, "jetbrains-plugin");
        assert!(config.tools.contains(&"jetbrains".to_string()));
        assert!(config.source_mappings.contains_key("skills"));
    }

    #[test]
    fn test_get_default_by_name() {
        assert!(TargetDefaults::get("claude-code").is_some());
        assert!(TargetDefaults::get("cursor").is_some());
        assert!(TargetDefaults::get("jetbrains").is_some());
        assert!(TargetDefaults::get("unknown").is_none());
    }

    #[test]
    fn test_available_targets() {
        let targets = TargetDefaults::available_targets();
        assert_eq!(targets.len(), 3);
        assert!(targets.contains(&"claude-code"));
        assert!(targets.contains(&"cursor"));
        assert!(targets.contains(&"jetbrains"));
    }

    #[test]
    fn test_default_skills_mapping_mode() {
        let config = TargetDefaults::claude_code();
        let skills_mapping = &config.source_mappings.get("skills").unwrap()[0];
        assert_eq!(skills_mapping.mode, "symlink");
    }
}
