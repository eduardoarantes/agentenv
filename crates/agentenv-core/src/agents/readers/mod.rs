//! Source-target readers for agents.
//!
//! Every Markdown-style source (claude-code, cursor, copilot, gemini-cli,
//! junie, …) stores agents as one Markdown file per agent with YAML
//! frontmatter; only the filename suffix and the project-local root vary
//! between them. The per-source files (`claude_code.rs`, `cursor.rs`, …)
//! are thin wrappers that hand the shared walker a different
//! `source_name`, root list, and `name_suffix`.
//!
//! Codex agents use TOML, not Markdown — but the directory-walk shape is
//! the same (iterate roots, skip hidden + non-files, strip a name suffix,
//! dedupe by name, sort). The shared [`parse_agents_by_suffix`] helper
//! takes a per-file parser callback so codex passes its TOML parser and
//! the Markdown wrappers pass the YAML-frontmatter parser. Only the
//! per-file body differs between sources.

pub mod claude_code;
pub mod codex;
pub mod copilot;
pub mod cursor;

use crate::agents::types::{Canonical, CanonicalAgent};
use crate::error::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Dispatch to the right reader based on `source` and produce the canonical
/// artifact. `roots` is the ordered list of directories to walk — typically
/// `[<project>/<source-agents-dir>, <plugin1>/agents, …]`.
///
/// Returns `Ok(None)` when no agent was found across any root.
pub fn read(source: &str, roots: &[&Path]) -> Result<Option<Canonical>> {
    match source {
        "claude-code" => claude_code::read(roots),
        "cursor" => cursor::read(roots),
        "codex" => codex::read(roots),
        "copilot" => copilot::read(roots),
        other => Err(Error::Config(format!(
            "agents source `{other}` is not implemented in this version"
        ))),
    }
}

/// Project-local agents directory the source target stores agent files
/// under, relative to the project root.
///
/// Pipelines call this to assemble the source-specific project root before
/// invoking [`read`]; plugin roots are always Claude-shaped (`<plugin>/agents`).
pub fn project_source_dir(source: &str, project_root: &Path) -> Option<PathBuf> {
    let rel = match source {
        "claude-code" => ".claude/agents",
        "cursor" => ".cursor/agents",
        "codex" => ".codex/agents",
        "copilot" => ".github/agents",
        _ => return None,
    };
    Some(project_root.join(rel))
}

/// Walk every root for files ending in `name_suffix` and produce a
/// canonical artifact tagged with `source_name`, parsing each file's
/// contents through the caller-supplied `parse_one`.
///
/// Centralizes the directory-walk semantics shared by every per-file agent
/// source (Markdown + YAML frontmatter for claude-code/cursor/copilot/…,
/// TOML for codex): iterate roots in order, skip hidden entries, skip
/// non-files, require the configured `name_suffix`, dedupe by name with
/// first-root-wins, and emit a sorted [`Canonical`]. Only the per-file
/// parser (e.g. YAML-frontmatter split vs TOML deserialize) varies between
/// callers.
pub(super) fn parse_agents_by_suffix<F>(
    source_name: &str,
    roots: &[&Path],
    name_suffix: &str,
    parse_one: F,
) -> Result<Option<Canonical>>
where
    F: Fn(&str, &Path) -> Result<CanonicalAgent>,
{
    debug_assert!(
        name_suffix.starts_with('.') && !name_suffix.is_empty(),
        "name_suffix must look like \".md\" or \".toml\""
    );

    let mut agents: Vec<CanonicalAgent> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let mut entries: Vec<fs::DirEntry> = fs::read_dir(root)?.collect::<std::io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let file_name = entry.file_name();
            let file_str = match file_name.to_str() {
                Some(s) if !s.starts_with('.') => s,
                _ => continue,
            };
            let path = entry.path();
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Some(stem) = file_str.strip_suffix(name_suffix) else {
                continue;
            };
            if stem.is_empty() {
                continue;
            }
            let name = stem.to_string();
            if !seen.insert(name.clone()) {
                continue;
            }
            agents.push(parse_one(&name, &path)?);
        }
    }

    if agents.is_empty() {
        return Ok(None);
    }
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Some(Canonical {
        source: source_name.to_string(),
        agents,
    }))
}

/// Walk every root for files ending in `name_suffix` (e.g. `".md"` for
/// Claude / Cursor / Gemini / Junie, `".agent.md"` for Copilot) and produce
/// a canonical artifact tagged with `source_name`.
///
/// Thin wrapper over [`parse_agents_by_suffix`] that fixes the per-file
/// parser to YAML-frontmatter + Markdown body. Codex (TOML) instead calls
/// `parse_agents_by_suffix` directly with its own TOML parser.
pub(super) fn parse_markdown_agents(
    source_name: &str,
    roots: &[&Path],
    name_suffix: &str,
) -> Result<Option<Canonical>> {
    parse_agents_by_suffix(source_name, roots, name_suffix, parse_agent)
}

fn parse_agent(name: &str, file: &Path) -> Result<CanonicalAgent> {
    let raw = fs::read_to_string(file)?;
    let (frontmatter, body) = crate::frontmatter::split(&raw)
        .map_err(|err| Error::Config(format!("invalid agent at {}: {err}", file.display())))?;
    Ok(CanonicalAgent {
        name: name.to_string(),
        frontmatter,
        body,
        source_file: file.to_path_buf(),
    })
}
