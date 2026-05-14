//! Source-target readers: turn a target's native skill layout into the
//! canonical model losslessly.
//!
//! Every documented source target (claude-code, cursor, codex, copilot, …)
//! uses the [agentskills.io](https://agentskills.io) `<name>/SKILL.md`
//! directory layout — the same `SKILL.md` + sidecars shape claude-code
//! defined. The per-source files (`claude_code.rs`, `cursor.rs`, …) are
//! thin wrappers that hand the shared layout walker a different
//! `source_name` + project-local root; if a future tool diverges from the
//! agentskills.io shape, its reader can implement its own walk without
//! touching the others.

pub mod claude_code;
pub mod codex;
pub mod copilot;
pub mod cursor;

use crate::error::{Error, Result};
use crate::skills::types::{Canonical, CanonicalSkill, SidecarFile, SidecarKind};
use std::fs;
use std::path::{Path, PathBuf};

const SKILL_FILENAME: &str = "SKILL.md";

/// Dispatch to the right reader based on `source` and produce the canonical
/// artifact. `roots` is the ordered list of directories to walk — typically
/// `[<project>/<source-skills-dir>, <plugin1>/skills, <plugin2>/skills, …]`.
///
/// Returns `Ok(None)` when no skill was found across any root.
pub fn read(source: &str, roots: &[&Path]) -> Result<Option<Canonical>> {
    match source {
        "claude-code" => claude_code::read(roots),
        "cursor" => cursor::read(roots),
        "codex" => codex::read(roots),
        "copilot" => copilot::read(roots),
        other => Err(Error::Config(format!(
            "skills source `{other}` is not implemented in this version"
        ))),
    }
}

/// Project-local skills directory the source target stores SKILL.md trees
/// under, relative to the project root.
///
/// Pipelines call this to assemble the source-specific project root before
/// invoking [`read`]; plugin roots are always Claude-shaped (`<plugin>/skills`).
pub fn project_source_dir(source: &str, project_root: &Path) -> Option<PathBuf> {
    let rel = match source {
        "claude-code" => ".claude/skills",
        "cursor" => ".cursor/skills",
        // Codex re-uses the cross-tool `.agents/skills` alias — matches
        // where the codex writer materializes Claude-shaped skills.
        "codex" => ".agents/skills",
        "copilot" => ".github/skills",
        _ => return None,
    };
    Some(project_root.join(rel))
}

/// Walk the agentskills.io `<name>/SKILL.md` layout under every root and
/// produce a canonical artifact tagged with `source_name`.
///
/// Source-agnostic helper used by every reader whose native skill layout is
/// the agentskills.io shape (every documented target today). Roots are
/// scanned in order; on name collision the first-root-wins (project before
/// plugins, plugins in supplied order). Hidden entries and directories
/// without a `SKILL.md` are silently skipped.
pub(super) fn parse_agentskills_layout(
    source_name: &str,
    roots: &[&Path],
) -> Result<Option<Canonical>> {
    let mut skills: Vec<CanonicalSkill> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in walk_sorted(root)? {
            let leaf_name = entry.file_name();
            let leaf_str = match leaf_name.to_str() {
                Some(s) if !s.starts_with('.') => s.to_string(),
                _ => continue,
            };
            let leaf_path = entry.path();
            if !leaf_path.is_dir() {
                continue;
            }
            let skill_md = leaf_path.join(SKILL_FILENAME);
            if !skill_md.is_file() {
                continue;
            }
            if !seen.insert(leaf_str.clone()) {
                continue;
            }
            skills.push(parse_skill(&leaf_str, &leaf_path)?);
        }
    }

    if skills.is_empty() {
        return Ok(None);
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Some(Canonical {
        source: source_name.to_string(),
        skills,
    }))
}

fn walk_sorted(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    Ok(entries)
}

fn parse_skill(name: &str, dir: &Path) -> Result<CanonicalSkill> {
    let raw = fs::read_to_string(dir.join(SKILL_FILENAME))?;
    let (frontmatter, body) = crate::frontmatter::split(&raw)
        .map_err(|err| Error::Config(format!("invalid SKILL.md at {}: {err}", dir.display())))?;
    let sidecars = collect_sidecars(dir)?;
    Ok(CanonicalSkill {
        name: name.to_string(),
        frontmatter,
        body,
        sidecars,
        source_dir: dir.to_path_buf(),
    })
}

/// Walk the skill directory and collect everything except `SKILL.md` itself.
fn collect_sidecars(dir: &Path) -> Result<Vec<SidecarFile>> {
    let mut out = Vec::new();
    walk_sidecars(dir, dir, &mut out)?;
    out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(out)
}

fn walk_sidecars(root: &Path, current: &Path, out: &mut Vec<SidecarFile>) -> Result<()> {
    for entry in walk_sorted(current)? {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk_sidecars(root, &path, out)?;
            continue;
        }
        if !ft.is_file() && !ft.is_symlink() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|err| Error::Config(format!("sidecar path {}: {err}", path.display())))?;
        if relative.as_os_str() == SKILL_FILENAME {
            continue;
        }
        // Canonical YAML must be portable across OSes. Join components with
        // `/` so Windows and Unix produce identical canonicals.
        let relative_str: String = relative
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/");
        let relative_buf = std::path::PathBuf::from(relative_str);
        let kind = SidecarKind::from_relative_path(&relative_buf);
        out.push(SidecarFile {
            relative_path: relative_buf,
            kind,
        });
    }
    Ok(())
}
