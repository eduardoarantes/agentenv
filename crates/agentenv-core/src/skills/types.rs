//! Canonical skill types — the agentenv-internal domain model.
//!
//! Skills are read losslessly from one source target's native layout (today
//! Claude Code's `<scope>/.claude/skills/<name>/SKILL.md` + plugin
//! `<plugin>/skills/<name>/SKILL.md`), serialized to
//! `.agentenv/skills.canonical.yaml`, and rendered out to every other
//! supporting target. `frontmatter` is an open YAML mapping so any field the
//! source emits round-trips through the canonical model without information
//! loss — writers consume only the keys they understand and report the rest
//! via [`WriteReport`].
//!
//! Mirrors [`crate::hooks::types`] in spirit: a top-level `Canonical` with
//! a deterministic, sorted vector of items, plus an open-map escape hatch.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level shape of `.agentenv/skills.canonical.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Canonical {
    /// Name of the source target this canonical was derived from (echoes
    /// the `source` field in `.agentrc.yaml`).
    pub source: String,
    /// One entry per skill, sorted by `name` for stable diffs.
    #[serde(default)]
    pub skills: Vec<CanonicalSkill>,
}

/// A single skill captured losslessly from the source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalSkill {
    /// Skill identity. Must match the parent directory name per the
    /// [agentskills.io](https://agentskills.io) spec.
    pub name: String,
    /// Verbatim YAML frontmatter from `SKILL.md`. Open-map: every key the
    /// source wrote is preserved. Writers ignore keys they don't understand
    /// and surface them via [`WriteReport::drops`].
    #[serde(default)]
    pub frontmatter: serde_yaml::Mapping,
    /// Markdown body after the closing `---` frontmatter delimiter.
    #[serde(default)]
    pub body: String,
    /// Files that live alongside `SKILL.md` in the source skill directory
    /// (`scripts/`, `references/`, `assets/`, …). Writers that materialize
    /// rather than symlink need this list to know what to copy through.
    #[serde(default)]
    pub sidecars: Vec<SidecarFile>,
    /// Absolute path of the source skill directory. Writers that symlink
    /// (most of them) point at this path; not serialized into the canonical
    /// YAML because it's environment-dependent.
    #[serde(skip)]
    pub source_dir: PathBuf,
}

/// A file inside a skill directory that is not `SKILL.md` itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidecarFile {
    /// Path relative to the skill directory (e.g. `scripts/run.sh`).
    pub relative_path: PathBuf,
    /// Classification — informational; writers do not currently dispatch on it.
    #[serde(default)]
    pub kind: SidecarKind,
}

/// Coarse classification of sidecar files. Matches the agentskills.io
/// convention sections (`scripts/`, `references/`, `assets/`, anything else).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SidecarKind {
    Script,
    Reference,
    Asset,
    #[default]
    Other,
}

impl SidecarKind {
    /// Infer the kind from a path's first component.
    pub fn from_relative_path(path: &std::path::Path) -> Self {
        match path
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str())
        {
            Some("scripts") => Self::Script,
            Some("references") => Self::Reference,
            Some("assets") => Self::Asset,
            _ => Self::Other,
        }
    }
}

/// Report returned by a writer: which canonical skills (or fields) could not
/// be rendered for this target.
#[derive(Debug, Default, Clone)]
pub struct WriteReport {
    /// Human-readable reasons one canonical skill (or one of its fields) was
    /// dropped.
    pub drops: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Canonical {
        let mut frontmatter = serde_yaml::Mapping::new();
        frontmatter.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String("hello".to_string()),
        );
        frontmatter.insert(
            serde_yaml::Value::String("description".to_string()),
            serde_yaml::Value::String("Sample skill".to_string()),
        );
        Canonical {
            source: "claude-code".to_string(),
            skills: vec![CanonicalSkill {
                name: "hello".to_string(),
                frontmatter,
                body: "# Hello\n".to_string(),
                sidecars: vec![SidecarFile {
                    relative_path: PathBuf::from("scripts/run.sh"),
                    kind: SidecarKind::Script,
                }],
                source_dir: PathBuf::from("/abs/.claude/skills/hello"),
            }],
        }
    }

    #[test]
    fn canonical_round_trips_through_yaml() {
        let original = sample();
        let yaml = serde_yaml::to_string(&original).unwrap();
        let mut parsed: Canonical = serde_yaml::from_str(&yaml).unwrap();
        // `source_dir` is `#[serde(skip)]` — restore it before comparing so
        // round-trip equality holds on the parts that are persisted.
        parsed.skills[0].source_dir = original.skills[0].source_dir.clone();
        assert_eq!(parsed, original);
    }

    #[test]
    fn sidecar_kind_inferred_from_first_path_component() {
        assert_eq!(
            SidecarKind::from_relative_path(std::path::Path::new("scripts/run.sh")),
            SidecarKind::Script
        );
        assert_eq!(
            SidecarKind::from_relative_path(std::path::Path::new("references/api.md")),
            SidecarKind::Reference
        );
        assert_eq!(
            SidecarKind::from_relative_path(std::path::Path::new("assets/logo.png")),
            SidecarKind::Asset
        );
        assert_eq!(
            SidecarKind::from_relative_path(std::path::Path::new("README.md")),
            SidecarKind::Other
        );
    }
}
