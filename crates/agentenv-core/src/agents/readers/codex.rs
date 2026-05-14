//! Read Codex-shaped agents (`<project>/.codex/agents/<name>.toml`) into
//! the canonical model.
//!
//! Codex agents use TOML, not Markdown + YAML. The reader inverts the
//! transform the [Codex agents writer][writer] applies:
//!
//! - Every top-level TOML key except `prompt` becomes a YAML frontmatter
//!   entry, so the canonical is lossless even for non-Codex schema fields
//!   the source happens to declare.
//! - The `prompt` key becomes the canonical body.
//! - The leading `# >>> agentenv managed …` sentinel comment that the
//!   writer emits is skipped (it isn't a TOML key, just a marker for
//!   refuse-on-conflict).
//!
//! Filename suffix is `.toml`; non-`.toml` files and subdirectories are
//! skipped silently. Name collisions resolve first-root-wins across the
//! supplied roots — same convention as the Markdown agent reader.
//!
//! [writer]: crate::agents::writers::codex

use crate::agents::types::{Canonical, CanonicalAgent};
use crate::error::{Error, Result};
use std::fs;
use std::path::Path;

const SOURCE_NAME: &str = "codex";
const NAME_SUFFIX: &str = ".toml";

/// Build the canonical from a set of agent roots.
///
/// Returns `Ok(None)` when no agent file was found across any root.
pub fn read(roots: &[&Path]) -> Result<Option<Canonical>> {
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
            let Some(stem) = file_str.strip_suffix(NAME_SUFFIX) else {
                continue;
            };
            if stem.is_empty() {
                continue;
            }
            let name = stem.to_string();
            if !seen.insert(name.clone()) {
                continue;
            }
            agents.push(parse_agent(&name, &path)?);
        }
    }

    if agents.is_empty() {
        return Ok(None);
    }
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Some(Canonical {
        source: SOURCE_NAME.to_string(),
        agents,
    }))
}

fn parse_agent(name: &str, file: &Path) -> Result<CanonicalAgent> {
    let raw = fs::read_to_string(file)?;
    let parsed: toml::Value = toml::from_str(&raw).map_err(|err| {
        Error::Config(format!("invalid codex agent at {}: {err}", file.display()))
    })?;
    let table = parsed.as_table().ok_or_else(|| {
        Error::Config(format!(
            "codex agent at {} is not a TOML table",
            file.display()
        ))
    })?;

    let body = match table.get("prompt") {
        Some(value) => match value.as_str() {
            Some(s) => s.to_string(),
            None => {
                return Err(Error::Config(format!(
                    "codex agent at {}: `prompt` must be a string, got {}",
                    file.display(),
                    toml_type_name(value)
                )));
            },
        },
        None => String::new(),
    };

    let mut frontmatter = serde_yaml::Mapping::new();
    for (key, value) in table {
        if key == "prompt" {
            continue;
        }
        let yaml_value = toml_to_yaml_value(value);
        frontmatter.insert(serde_yaml::Value::String(key.clone()), yaml_value);
    }

    Ok(CanonicalAgent {
        name: name.to_string(),
        frontmatter,
        body,
        source_file: file.to_path_buf(),
    })
}

/// Human-readable type tag for a TOML value, used in error messages so
/// users can tell *why* a field was rejected (e.g. `prompt = 42` reports
/// `integer`).
fn toml_type_name(v: &toml::Value) -> &'static str {
    match v {
        toml::Value::String(_) => "string",
        toml::Value::Integer(_) => "integer",
        toml::Value::Float(_) => "float",
        toml::Value::Boolean(_) => "boolean",
        toml::Value::Array(_) => "array",
        toml::Value::Table(_) => "table",
        toml::Value::Datetime(_) => "datetime",
    }
}

/// Translate a TOML value into the matching `serde_yaml::Value`. Lossless
/// for every concrete TOML type — preserves arrays, tables, and dates as
/// untyped strings where YAML has no direct equivalent.
fn toml_to_yaml_value(value: &toml::Value) -> serde_yaml::Value {
    match value {
        toml::Value::String(s) => serde_yaml::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_yaml::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_yaml::Value::Number(serde_yaml::Number::from(*f)),
        toml::Value::Boolean(b) => serde_yaml::Value::Bool(*b),
        toml::Value::Array(items) => {
            serde_yaml::Value::Sequence(items.iter().map(toml_to_yaml_value).collect())
        },
        toml::Value::Table(table) => {
            let mut out = serde_yaml::Mapping::new();
            for (k, v) in table {
                out.insert(serde_yaml::Value::String(k.clone()), toml_to_yaml_value(v));
            }
            serde_yaml::Value::Mapping(out)
        },
        // TOML datetimes have no YAML scalar counterpart; the agent schema
        // doesn't use them but we keep the value lossless as a string so
        // round-tripping back through a Codex writer never silently drops
        // it.
        toml::Value::Datetime(dt) => serde_yaml::Value::String(dt.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_codex_agent(root: &Path, name: &str, toml_body: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join(format!("{name}.toml")), toml_body).unwrap();
    }

    #[test]
    fn returns_none_when_no_toml_files() {
        let scratch = TempDir::new().unwrap();
        fs::create_dir_all(scratch.path()).unwrap();
        fs::write(scratch.path().join("notes.md"), "ignored").unwrap();
        assert!(read(&[scratch.path()]).unwrap().is_none());
    }

    #[test]
    fn parses_documented_fields_and_prompt() {
        let scratch = TempDir::new().unwrap();
        write_codex_agent(
            scratch.path(),
            "rev",
            r#"name = "rev"
description = "Reviews PRs"
model = "gpt-5"
tools = ["Read", "Grep"]
prompt = """You are a reviewer.
Line 2.
"""
"#,
        );
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(canonical.source, "codex");
        assert_eq!(canonical.agents.len(), 1);
        let a = &canonical.agents[0];
        assert_eq!(a.name, "rev");
        assert_eq!(a.body, "You are a reviewer.\nLine 2.\n");
        let name = a
            .frontmatter
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|v| v.as_str());
        assert_eq!(name, Some("rev"));
        let model = a
            .frontmatter
            .get(serde_yaml::Value::String("model".to_string()))
            .and_then(|v| v.as_str());
        assert_eq!(model, Some("gpt-5"));
        let tools = a
            .frontmatter
            .get(serde_yaml::Value::String("tools".to_string()))
            .and_then(|v| v.as_sequence())
            .unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].as_str(), Some("Read"));
    }

    #[test]
    fn skips_agentenv_managed_sentinel_comment() {
        // The writer prepends a `# >>> agentenv managed …` line; TOML
        // ignores comments, so the parser already handles this. Test that
        // contract explicitly so a future writer change can't silently
        // poison the read path.
        let scratch = TempDir::new().unwrap();
        write_codex_agent(
            scratch.path(),
            "rev",
            "# >>> agentenv managed (do not edit; regenerated by `agentenv sync`) <<<\n\
             name = \"rev\"\n\
             prompt = \"body\"\n",
        );
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(canonical.agents.len(), 1);
        assert_eq!(canonical.agents[0].body, "body");
    }

    #[test]
    fn preserves_unknown_keys_as_frontmatter() {
        // Lossless contract: the reader does not filter to a documented
        // subset. Anything the file declared survives in `frontmatter`.
        let scratch = TempDir::new().unwrap();
        write_codex_agent(
            scratch.path(),
            "rev",
            "name = \"rev\"\n\
             custom_key = \"keep me\"\n\
             prompt = \"body\"\n",
        );
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        let custom = canonical.agents[0]
            .frontmatter
            .get(serde_yaml::Value::String("custom_key".to_string()))
            .and_then(|v| v.as_str());
        assert_eq!(custom, Some("keep me"));
    }

    #[test]
    fn missing_prompt_yields_empty_body() {
        let scratch = TempDir::new().unwrap();
        write_codex_agent(scratch.path(), "rev", "name = \"rev\"\n");
        let canonical = read(&[scratch.path()]).unwrap().unwrap();
        assert_eq!(canonical.agents[0].body, "");
    }

    #[test]
    fn first_root_wins_on_name_collision() {
        let project = TempDir::new().unwrap();
        let plugin = TempDir::new().unwrap();
        write_codex_agent(project.path(), "dup", "prompt = \"project\"\n");
        write_codex_agent(plugin.path(), "dup", "prompt = \"plugin\"\n");
        let canonical = read(&[project.path(), plugin.path()]).unwrap().unwrap();
        assert_eq!(canonical.agents.len(), 1);
        assert_eq!(canonical.agents[0].body, "project");
    }

    #[test]
    fn malformed_toml_is_a_config_error() {
        let scratch = TempDir::new().unwrap();
        write_codex_agent(scratch.path(), "broken", "not = toml = at = all");
        let err = read(&[scratch.path()]).unwrap_err();
        assert!(err.to_string().contains("invalid codex agent"));
    }

    #[test]
    fn non_string_prompt_is_a_config_error() {
        // A `prompt = 42` (or any non-string) would previously be silently
        // dropped — body emptied AND the integer omitted from frontmatter
        // because the loop unconditionally skips the `prompt` key. The
        // lossless contract requires we surface this as `Error::Config`
        // instead of degrading to a useless empty agent.
        let scratch = TempDir::new().unwrap();
        write_codex_agent(scratch.path(), "bad", "name = \"bad\"\nprompt = 42\n");
        let err = read(&[scratch.path()]).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("`prompt` must be a string"),
            "expected prompt-type error, got: {msg}"
        );
        assert!(msg.contains("integer"), "expected type tag, got: {msg}");
    }
}
