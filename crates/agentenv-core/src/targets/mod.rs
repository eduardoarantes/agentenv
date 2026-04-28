//! Target configurations and defaults

pub mod defaults;

pub use defaults::TargetDefaults;

/// Supported target tools
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TargetType {
    /// Claude Code (VS Code extension)
    ClaudeCode,
    /// Cursor IDE
    Cursor,
    /// JetBrains IDEs
    JetBrains,
}

impl std::fmt::Display for TargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetType::ClaudeCode => write!(f, "claude-code"),
            TargetType::Cursor => write!(f, "cursor"),
            TargetType::JetBrains => write!(f, "jetbrains"),
        }
    }
}
