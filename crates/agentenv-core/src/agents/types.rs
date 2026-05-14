//! Canonical agent types — the agentenv-internal domain model.
//!
//! Agents are read losslessly from one source target's native layout (today
//! Claude Code's `<scope>/.claude/agents/<name>.md` + plugin
//! `<plugin>/agents/<name>.md`), serialized to
//! `.agentenv/agents.canonical.yaml`, and rendered out to every other
//! supporting target. `frontmatter` is an open YAML mapping so any field
//! the source emits round-trips through the canonical without information
//! loss — writers consume only the keys they understand and surface the
//! rest via [`WriteReport`].
//!
//! Mirrors [`crate::skills::types`] — same top-level `Canonical { source,
//! <items> }` shape, sorted vector, open-map escape hatch.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level shape of `.agentenv/agents.canonical.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Canonical {
    /// Name of the source target (echoes `.agentrc.yaml` `source`).
    pub source: String,
    /// One entry per agent, sorted by `name` for stable diffs.
    #[serde(default)]
    pub agents: Vec<CanonicalAgent>,
}

/// A single agent captured losslessly from the source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalAgent {
    /// Agent identity — equals the source file stem (e.g. `code-reviewer`
    /// for `.claude/agents/code-reviewer.md`).
    pub name: String,
    /// Verbatim YAML frontmatter. Open-map: every key the source wrote
    /// is preserved. Writers ignore keys they don't understand and surface
    /// them via [`WriteReport::drops`].
    #[serde(default)]
    pub frontmatter: serde_yaml::Mapping,
    /// Markdown body after the closing `---` delimiter — the agent's
    /// system prompt.
    #[serde(default)]
    pub body: String,
    /// Absolute path of the source file. Used by writers that symlink
    /// rather than materialize. Not serialized into the canonical YAML
    /// because it's environment-dependent.
    #[serde(skip)]
    pub source_file: PathBuf,
}

/// Report returned by a writer: which canonical agents (or fields) could
/// not be rendered for this target.
#[derive(Debug, Default, Clone)]
pub struct WriteReport {
    /// Human-readable reasons one canonical agent (or one of its fields)
    /// was dropped.
    pub drops: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Canonical {
        let mut frontmatter = serde_yaml::Mapping::new();
        frontmatter.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String("rev".to_string()),
        );
        frontmatter.insert(
            serde_yaml::Value::String("description".to_string()),
            serde_yaml::Value::String("Reviews PRs".to_string()),
        );
        Canonical {
            source: "claude-code".to_string(),
            agents: vec![CanonicalAgent {
                name: "rev".to_string(),
                frontmatter,
                body: "You are a reviewer.\n".to_string(),
                source_file: PathBuf::from("/abs/.claude/agents/rev.md"),
            }],
        }
    }

    #[test]
    fn canonical_round_trips_through_yaml() {
        let original = sample();
        let yaml = serde_yaml::to_string(&original).unwrap();
        let mut parsed: Canonical = serde_yaml::from_str(&yaml).unwrap();
        // `source_file` is `#[serde(skip)]` — restore before comparing.
        parsed.agents[0].source_file = original.agents[0].source_file.clone();
        assert_eq!(parsed, original);
    }

    #[test]
    fn frontmatter_preserves_arbitrary_keys() {
        let mut fm = serde_yaml::Mapping::new();
        fm.insert(
            serde_yaml::Value::String("permissionMode".to_string()),
            serde_yaml::Value::String("readonly".to_string()),
        );
        fm.insert(
            serde_yaml::Value::String("mcpServers".to_string()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("github".to_string())]),
        );
        let c = Canonical {
            source: "claude-code".to_string(),
            agents: vec![CanonicalAgent {
                name: "weird".to_string(),
                frontmatter: fm,
                body: String::new(),
                source_file: PathBuf::new(),
            }],
        };
        let yaml = serde_yaml::to_string(&c).unwrap();
        // Both keys survive round-trip; writers can choose to drop or keep them.
        assert!(yaml.contains("permissionMode"));
        assert!(yaml.contains("mcpServers"));
    }
}
