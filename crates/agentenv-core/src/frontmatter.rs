//! Shared YAML-frontmatter splitter used by source readers.
//!
//! Both skill (`SKILL.md`) and agent (`<name>.md`) files in Claude Code's
//! native layout follow the same convention:
//!
//! ```text
//! ---
//! <YAML mapping>
//! ---
//! <Markdown body>
//! ```
//!
//! This module is the one place that knows the format, so readers stay
//! focused on capability-specific concerns (sidecar collection, file
//! naming).

/// Split a Markdown source into `(frontmatter mapping, body)`.
///
/// - If the input does not start with `---` (LF or CRLF), the whole input
///   is returned as the body with an empty mapping.
/// - If the input opens with `---` but never closes it, returns
///   `Err(reason)` so callers can surface a precise diagnostic.
/// - Frontmatter must be a YAML mapping; null/empty frontmatter is mapped
///   to an empty `Mapping`. Sequences and scalars are rejected.
pub fn split(raw: &str) -> std::result::Result<(serde_yaml::Mapping, String), String> {
    let after_open = match raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
    {
        Some(rest) => rest,
        None => return Ok((serde_yaml::Mapping::new(), raw.to_string())),
    };

    let mut close_idx: Option<usize> = None;
    let mut cursor = 0usize;
    for line in after_open.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            close_idx = Some(cursor);
            break;
        }
        cursor += line.len();
    }
    let close_idx = close_idx.ok_or_else(|| "missing closing `---` delimiter".to_string())?;

    let yaml = &after_open[..close_idx];
    let after_close = &after_open[close_idx..];
    let body = if let Some(rest) = after_close.strip_prefix("---\n") {
        rest
    } else if let Some(rest) = after_close.strip_prefix("---\r\n") {
        rest
    } else {
        after_close.trim_start_matches("---")
    };

    let parsed: serde_yaml::Value =
        serde_yaml::from_str(yaml).map_err(|err| format!("frontmatter YAML: {err}"))?;
    let mapping = match parsed {
        serde_yaml::Value::Mapping(m) => m,
        serde_yaml::Value::Null => serde_yaml::Mapping::new(),
        other => return Err(format!("frontmatter must be a YAML mapping, got {other:?}")),
    };
    Ok((mapping, body.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_without_fences_is_all_body() {
        let (fm, body) = split("plain text\n").unwrap();
        assert!(fm.is_empty());
        assert_eq!(body, "plain text\n");
    }

    #[test]
    fn parses_typical_frontmatter() {
        let (fm, body) = split("---\nname: hello\ndescription: x\n---\nthe body\n").unwrap();
        assert_eq!(
            fm.get(serde_yaml::Value::String("name".to_string()))
                .unwrap()
                .as_str(),
            Some("hello")
        );
        assert_eq!(body, "the body\n");
    }

    #[test]
    fn missing_close_is_an_error() {
        let err = split("---\nname: x\nno close\n").unwrap_err();
        assert!(err.contains("missing closing"), "got: {err}");
    }

    #[test]
    fn rejects_sequence_frontmatter() {
        let err = split("---\n- a\n- b\n---\nbody").unwrap_err();
        assert!(err.contains("mapping"), "got: {err}");
    }

    #[test]
    fn empty_frontmatter_is_an_empty_mapping() {
        let (fm, body) = split("---\n---\nbody\n").unwrap();
        assert!(fm.is_empty());
        assert_eq!(body, "body\n");
    }

    #[test]
    fn handles_crlf_line_endings() {
        let (fm, body) = split("---\r\nname: x\r\n---\r\nbody\r\n").unwrap();
        assert_eq!(
            fm.get(serde_yaml::Value::String("name".to_string()))
                .unwrap()
                .as_str(),
            Some("x")
        );
        assert!(body.starts_with("body"));
    }
}
