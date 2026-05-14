//! Read Codex hooks from `~/.codex/config.toml` and produce the canonical
//! model.
//!
//! Codex exposes a single turn-end hook via the top-level `notify` array
//! in its global config:
//!
//! ```toml
//! notify = ["bash", "scripts/notify.sh"]
//! ```
//!
//! The reader yields exactly one canonical [`Stop`](CommonEvent::Stop)
//! hook whose `command` is the array elements joined with spaces. Any
//! agentenv-managed sentinel block is stripped before parsing so a stale
//! managed `notify` line (e.g. left over after a source switch) is not
//! mistaken for user-authored content — `source: codex` semantically
//! means "the user owns this file directly".
//!
//! When the file is absent, contains no top-level `notify`, or only
//! contains an agentenv-managed `notify`, the reader returns `Ok(None)`.

use crate::error::{Error, Result};
use crate::hooks::types::{Action, Canonical, CommonEvent, Event, Hook};
use crate::hooks::writers::codex::{BEGIN_MARKER, END_MARKER};
use std::fs;
use std::path::{Path, PathBuf};

const SOURCE_NAME: &str = "codex";

/// Build the canonical by reading `~/.codex/config.toml`.
///
/// `project_root` is accepted (and ignored) so the reader's signature
/// matches the dispatch in [`super::read`] — Codex's hooks live in the
/// user-global config, not under the project.
pub fn read(_project_root: &Path) -> Result<Option<Canonical>> {
    let path = config_path()?;
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::Io(err)),
    };
    let unmanaged = strip_managed_block(&content);
    let parsed: toml::Value = toml::from_str(&unmanaged)
        .map_err(|err| Error::Config(format!("failed to parse {}: {err}", path.display())))?;
    let Some(notify) = parsed.get("notify") else {
        return Ok(None);
    };
    let Some(command) = render_notify_command(notify) else {
        return Ok(None);
    };
    if command.is_empty() {
        return Ok(None);
    }
    Ok(Some(Canonical {
        source: SOURCE_NAME.to_string(),
        hooks: vec![Hook {
            event: Event::Common(CommonEvent::Stop),
            matcher: None,
            action: Action::Command {
                command,
                timeout_ms: None,
                cwd: None,
            },
        }],
    }))
}

fn config_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| Error::Config("cannot determine home directory".to_string()))?;
    Ok(home.join(".codex").join("config.toml"))
}

/// Render the Codex `notify = [...]` array into a single shell command
/// string. Codex's docs treat the array as a `bash -c`–style invocation
/// (program plus args).
///
/// - A `toml::Value::String` is returned unchanged — the user has already
///   chosen a single shell-string representation.
/// - A `toml::Value::Array` is joined with spaces. Each element that
///   contains whitespace or shell metacharacters is POSIX single-quoted
///   so element boundaries survive the join: `["bash", "-c", "echo hi
///   there"]` becomes `bash -c 'echo hi there'` instead of the ambiguous
///   `bash -c echo hi there`. Elements with no such characters are
///   emitted unquoted for cleaner output.
/// - Any other value (or an array containing non-string elements) yields
///   `None`.
fn render_notify_command(notify: &toml::Value) -> Option<String> {
    match notify {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Array(items) => {
            let mut parts: Vec<String> = Vec::with_capacity(items.len());
            for item in items {
                let s = item.as_str()?;
                parts.push(shell_quote_if_needed(s));
            }
            Some(parts.join(" "))
        },
        _ => None,
    }
}

/// Return `s` unchanged if it contains no whitespace or shell
/// metacharacters; otherwise wrap it in POSIX single quotes with any
/// literal `'` escaped as `'\''`.
fn shell_quote_if_needed(s: &str) -> String {
    if s.is_empty() || s.chars().any(needs_quoting) {
        shell_single_quote(s)
    } else {
        s.to_string()
    }
}

/// Conservative set of characters that force POSIX shell quoting:
/// whitespace plus any byte the shell treats specially.
fn needs_quoting(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t'
            | '\n'
            | '"'
            | '\''
            | '\\'
            | '$'
            | '`'
            | '|'
            | '&'
            | ';'
            | '<'
            | '>'
            | '('
            | ')'
            | '{'
            | '}'
            | '*'
            | '?'
            | '['
            | ']'
            | '~'
            | '#'
            | '='
    )
}

/// Wrap `s` in single quotes safely for inclusion in a POSIX shell
/// command. Mirrors the helper of the same name in
/// [`super::super::writers::codex`]; duplicated here to avoid promoting a
/// trivial private utility into a cross-module dependency.
fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Strip the agentenv-managed sentinel block from `text`. Mirrors the
/// writer's `splice_block(_, None)` behaviour so the reader sees only the
/// user-authored portion of the config.
fn strip_managed_block(text: &str) -> String {
    let begin = text.find(BEGIN_MARKER);
    let end = text.find(END_MARKER);
    match (begin, end) {
        (Some(b), Some(e)) if e > b => {
            let before = text[..b].trim_end_matches('\n');
            let after_idx = e + END_MARKER.len();
            let after = text[after_idx..].trim_start_matches('\n');
            let mut out = String::with_capacity(before.len() + after.len() + 2);
            out.push_str(before);
            if !before.is_empty() && !after.is_empty() {
                out.push('\n');
            }
            out.push_str(after);
            out
        },
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Run `f` with `$HOME` pointed at a fresh temp dir. Serializes
    /// against [`crate::hooks::HOME_LOCK`] — see its rustdoc for why
    /// per-module locks are not safe here.
    #[cfg(unix)]
    fn with_isolated_home<F: FnOnce(&Path) -> R, R>(f: F) -> (R, TempDir) {
        let _guard = crate::hooks::HOME_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let home = TempDir::new().unwrap();
        let saved = std::env::var_os("HOME");
        std::env::set_var("HOME", home.path());
        let r = f(home.path());
        match saved {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        (r, home)
    }

    #[cfg(unix)]
    fn seed_config(home: &Path, content: &str) {
        let dir = home.join(".codex");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), content).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn returns_none_when_config_missing() {
        let project = TempDir::new().unwrap();
        let (result, _home) = with_isolated_home(|_| read(project.path()));
        assert!(result.unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn returns_none_when_no_notify_key() {
        let project = TempDir::new().unwrap();
        let (result, _home) = with_isolated_home(|home| {
            seed_config(home, "model = \"o4-mini\"\n");
            read(project.path())
        });
        assert!(result.unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn reads_user_authored_notify_as_stop_hook() {
        let project = TempDir::new().unwrap();
        let (result, _home) = with_isolated_home(|home| {
            seed_config(
                home,
                "model = \"o4-mini\"\nnotify = [\"bash\", \"scripts/notify.sh\"]\n",
            );
            read(project.path())
        });
        let canonical = result.unwrap().unwrap();
        assert_eq!(canonical.source, "codex");
        assert_eq!(canonical.hooks.len(), 1);
        assert!(matches!(
            canonical.hooks[0].event,
            Event::Common(CommonEvent::Stop)
        ));
        let Action::Command { command, .. } = &canonical.hooks[0].action;
        assert_eq!(command, "bash scripts/notify.sh");
    }

    #[cfg(unix)]
    #[test]
    fn ignores_agentenv_managed_block() {
        // If the user switched sources, an older sync may have left a
        // managed block behind. When `source: codex`, the user owns the
        // file directly — managed content should be invisible to the
        // reader.
        let project = TempDir::new().unwrap();
        let managed = format!(
            "model = \"o4-mini\"\n\
             \n\
             {BEGIN_MARKER}\n\
             notify = [\"bash\", \"/stale/dispatcher.sh\"]\n\
             {END_MARKER}\n"
        );
        let (result, _home) = with_isolated_home(|home| {
            seed_config(home, &managed);
            read(project.path())
        });
        // Only managed content → no user-authored notify → None.
        assert!(result.unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn user_notify_alongside_managed_block_wins() {
        let project = TempDir::new().unwrap();
        let mixed = format!(
            "notify = [\"my-notify\"]\n\
             {BEGIN_MARKER}\n\
             notify = [\"bash\", \"/stale/dispatcher.sh\"]\n\
             {END_MARKER}\n"
        );
        let (result, _home) = with_isolated_home(|home| {
            seed_config(home, &mixed);
            read(project.path())
        });
        let canonical = result.unwrap().unwrap();
        let Action::Command { command, .. } = &canonical.hooks[0].action;
        assert_eq!(command, "my-notify");
    }

    #[cfg(unix)]
    #[test]
    fn accepts_notify_as_string() {
        // The docs prefer an array, but a bare string is a legitimate
        // shorthand and Codex accepts both — be permissive on the read
        // side.
        let project = TempDir::new().unwrap();
        let (result, _home) = with_isolated_home(|home| {
            seed_config(home, "notify = \"my-notify\"\n");
            read(project.path())
        });
        let canonical = result.unwrap().unwrap();
        let Action::Command { command, .. } = &canonical.hooks[0].action;
        assert_eq!(command, "my-notify");
    }

    #[cfg(unix)]
    #[test]
    fn quotes_array_elements_with_whitespace_or_metachars() {
        // Without quoting, a user-authored `notify = ["bash", "-c",
        // "echo hello world"]` would render as `bash -c echo hello
        // world` — the boundary between the third element and the rest
        // is lost. With POSIX single-quoting, the boundary survives.
        let project = TempDir::new().unwrap();
        let (result, _home) = with_isolated_home(|home| {
            seed_config(home, "notify = [\"bash\", \"-c\", \"echo hello world\"]\n");
            read(project.path())
        });
        let canonical = result.unwrap().unwrap();
        let Action::Command { command, .. } = &canonical.hooks[0].action;
        assert_eq!(command, "bash -c 'echo hello world'");
    }

    #[cfg(unix)]
    #[test]
    fn quotes_array_element_containing_single_quote() {
        // Literal single quotes inside an element must be escaped as
        // `'\''` so the resulting shell string still parses as one arg.
        let project = TempDir::new().unwrap();
        let (result, _home) = with_isolated_home(|home| {
            seed_config(home, "notify = [\"bash\", \"-c\", \"echo it's fine\"]\n");
            read(project.path())
        });
        let canonical = result.unwrap().unwrap();
        let Action::Command { command, .. } = &canonical.hooks[0].action;
        assert_eq!(command, "bash -c 'echo it'\\''s fine'");
    }

    #[cfg(unix)]
    #[test]
    fn malformed_toml_is_a_config_error() {
        let project = TempDir::new().unwrap();
        let (result, _home) = with_isolated_home(|home| {
            seed_config(home, "not = toml = at = all");
            read(project.path())
        });
        let err = result.unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
    }

    #[test]
    fn strip_managed_block_removes_only_the_sentinel_section() {
        let raw = format!("user = 1\n{BEGIN_MARKER}\ninner\n{END_MARKER}\nuser2 = 2\n");
        let out = strip_managed_block(&raw);
        assert!(out.contains("user = 1"));
        assert!(out.contains("user2 = 2"));
        assert!(!out.contains(BEGIN_MARKER));
        assert!(!out.contains("inner"));
    }
}
