use serde::{Deserialize, Serialize};

/// Supported target tools
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Target {
    /// Claude Code (VS Code extension)
    #[serde(rename = "claude-code")]
    ClaudeCode,

    /// Cursor IDE
    #[serde(rename = "cursor")]
    Cursor,

    /// JetBrains IDEs
    #[serde(rename = "jetbrains")]
    JetBrains,

    /// Generic VS Code
    #[serde(rename = "vscode")]
    VsCode,
}

impl Target {
    /// Get configuration directory for target
    pub fn config_dir(&self) -> Option<&'static str> {
        match self {
            Target::ClaudeCode => Some("~/.vscode/extensions/claude-code"),
            Target::Cursor => Some("~/.cursor/extensions"),
            Target::JetBrains => Some("~/.config/JetBrains"),
            Target::VsCode => Some("~/.vscode"),
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::ClaudeCode => write!(f, "claude-code"),
            Target::Cursor => write!(f, "cursor"),
            Target::JetBrains => write!(f, "jetbrains"),
            Target::VsCode => write!(f, "vscode"),
        }
    }
}
