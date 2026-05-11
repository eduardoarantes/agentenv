//! Import marketplaces, plugins, and hooks from Claude Code `settings.json`.
//!
//! When `.agentrc.yaml` sets `use_claude_config: true`, the loader reads both
//! the user's global `~/.claude/settings.json` and the project's
//! `<root>/.claude/settings.json`, translates Claude's `extraKnownMarketplaces`
//! and `enabledPlugins` into agentenv's native config model, and preserves
//! `hooks` verbatim for surfacing via `agentenv claude-config show`.
//!
//! Layering: project settings win over global on key conflicts. `.agentrc.yaml`
//! wins over both (handled by [`crate::config::Config::merge_claude_import`]).

use crate::config::{Config, MarketplaceConfig, PluginRef};
use crate::error::{Error, Result};
use crate::resolver::ResolvedPlugin;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Normalized view of what a Claude settings.json contributes to agentenv.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ClaudeConfigImport {
    /// Marketplaces translated from `extraKnownMarketplaces`.
    pub marketplaces: HashMap<String, MarketplaceConfig>,

    /// Plugins translated from `enabledPlugins`.
    pub plugins: Vec<PluginRef>,

    /// Hooks preserved verbatim from `hooks`. May be `Value::Null` if neither
    /// settings file declared any.
    pub hooks: Value,
}

/// Name used for the synthesized local-`.claude/` plugin.
pub const LOCAL_CLAUDE_PLUGIN_NAME: &str = "local-claude";

/// Namespace used for the synthesized local-`.claude/` plugin.
const LOCAL_CLAUDE_NAMESPACE: &str = "_local";

/// Synthesize a [`ResolvedPlugin`] from the project's `.claude/` directory so
/// inline-authored agents/skills/commands get propagated to non-Claude targets
/// alongside marketplace plugins.
///
/// Scans `<project_root>/.claude/{agents,skills,commands}` for non-hidden
/// entries. If at least one capability has content, returns a synthetic
/// plugin whose `location` is the project's `.claude/` directory; the sync
/// engine then walks it the same way it walks marketplace plugins. Returns
/// `None` when nothing inline lives under `.claude/` — callers should treat
/// that as "no local plugin to sync."
///
/// The `claude-code` target is dropped during `merge_claude_import`, so the
/// synthesized plugin's leaves only ever land in other targets (cursor,
/// codex, copilot, …) — Claude reads its own `.claude/` directly.
pub fn synthesize_local_claude_plugin(project_root: &Path) -> Option<ResolvedPlugin> {
    let claude_dir = project_root.join(".claude");
    if !claude_dir.is_dir() {
        return None;
    }

    let mut capabilities = Vec::new();
    for cap in ["agents", "skills", "commands"] {
        if capability_has_entries(&claude_dir.join(cap)) {
            capabilities.push(cap.to_string());
        }
    }
    if capabilities.is_empty() {
        return None;
    }

    Some(ResolvedPlugin {
        name: LOCAL_CLAUDE_PLUGIN_NAME.to_string(),
        version: "0.0.0".to_string(),
        namespace: LOCAL_CLAUDE_NAMESPACE.to_string(),
        location: claude_dir.to_string_lossy().to_string(),
        metadata: Value::Null,
        targets: Vec::new(),
        capabilities,
    })
}

/// Built-in instruction-file destinations claimed by each target type when
/// `use_claude_config: true` and the user hasn't written their own
/// `instruction_files:` block. Returning an empty slice means "this target
/// has no opinion on where its instruction sheet lives" (e.g. claude-code
/// itself, since `use_claude_config: true` drops it as a sync destination).
pub(crate) fn default_instruction_destinations_for_target(
    target_type: &str,
) -> &'static [&'static str] {
    match target_type {
        // AGENTS.md is the de-facto cross-tool root instruction sheet —
        // accepted by Codex, Cursor, and Copilot (see
        // docs/platform-standards.md §6.1).
        "codex" | "cursor" | "copilot" => &["AGENTS.md"],
        // Gemini CLI's native name is GEMINI.md; also accepts AGENTS.md.
        "gemini-cli" => &["GEMINI.md", "AGENTS.md"],
        "junie" => &[".junie/AGENTS.md"],
        "antigravity" => &["agents.md"],
        _ => &[],
    }
}

/// When `use_claude_config: true` and the user hasn't written an
/// `instruction_files:` block of their own, populate it with sensible
/// defaults derived from the configured targets. Mirrors the rest of
/// `use_claude_config`'s philosophy: Claude is the source of truth, agentenv
/// auto-wires propagation to the other tools.
///
/// Source-file selection:
/// - prefer `CLAUDE.md` at the project root (matches "Claude as source")
/// - fall back to `AGENTS.md` so projects using the cross-tool naming still
///   get auto-propagation
/// - if neither exists, do nothing (silently — defaults are best-effort, not
///   an error condition)
///
/// Destinations are the union of each configured target's default
/// instruction destinations (see [`default_instruction_destinations_for_target`]),
/// minus the source filename itself to avoid self-references.
pub fn apply_default_instruction_files(config: &mut Config, project_root: &Path) {
    if !config.use_claude_config || !config.instruction_files.is_empty() {
        return;
    }

    let source = if project_root.join("CLAUDE.md").is_file() {
        "CLAUDE.md"
    } else if project_root.join("AGENTS.md").is_file() {
        "AGENTS.md"
    } else {
        return;
    };

    let mut destinations: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for target in config.targets.values() {
        for dest in default_instruction_destinations_for_target(&target.r#type) {
            if *dest == source {
                continue;
            }
            destinations.insert((*dest).to_string());
        }
    }

    if destinations.is_empty() {
        return;
    }

    config
        .instruction_files
        .insert(source.to_string(), destinations.into_iter().collect());
}

/// `true` iff `dir` is a directory containing at least one non-hidden entry.
/// Hidden entries (leading `.`) are ignored to match the sync engine's
/// own `list_capability_leaves` rules.
fn capability_has_entries(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .flatten()
        .any(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
}

/// Reader for Claude `settings.json` files.
pub struct ClaudeConfigLoader;

impl ClaudeConfigLoader {
    /// Load + merge project and global Claude settings using the OS home
    /// directory for the global file.
    pub fn load(project_root: &Path) -> Result<ClaudeConfigImport> {
        let home = dirs::home_dir().ok_or_else(|| {
            Error::Config("cannot determine home directory for Claude config".to_string())
        })?;
        Self::load_with_home(project_root, &home)
    }

    /// Load + merge using an explicit home directory. Test entry point.
    pub fn load_with_home(project_root: &Path, home: &Path) -> Result<ClaudeConfigImport> {
        let global_path = home.join(".claude").join("settings.json");
        let project_path = project_root.join(".claude").join("settings.json");

        let global = read_optional_json(&global_path)?;
        let project = read_optional_json(&project_path)?;

        if global.is_none() && project.is_none() {
            return Err(Error::Config(format!(
                "use_claude_config: true but no settings.json found at {} or {}",
                global_path.display(),
                project_path.display()
            )));
        }

        let global = global.unwrap_or(Value::Null);
        let project = project.unwrap_or(Value::Null);

        let marketplaces = merge_marketplaces(&global, &project);
        let plugins = merge_plugins(&global, &project);
        let hooks = merge_hooks(&global, &project);

        Ok(ClaudeConfigImport {
            marketplaces,
            plugins,
            hooks,
        })
    }
}

fn read_optional_json(path: &Path) -> Result<Option<Value>> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let value: Value = serde_json::from_str(&content).map_err(|err| {
                Error::Config(format!(
                    "failed to parse Claude settings at {}: {}",
                    path.display(),
                    err
                ))
            })?;
            Ok(Some(value))
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(Error::Io(err)),
    }
}

/// Translate `extraKnownMarketplaces` from both files, project winning on
/// conflict. Claude supports several `source.source` types; agentenv only
/// understands those that resolve to a git URL:
///
/// - `{ "source": "git", "url": "https://..." }` → `url` used directly
/// - `{ "source": "github", "repo": "org/name" }` → `https://github.com/org/name.git`
///
/// Entries with any other source type (e.g. `local` paths) are skipped with
/// a `tracing::warn!` rather than erroring, since agentenv has no way to
/// fetch them via its marketplace machinery.
fn merge_marketplaces(global: &Value, project: &Value) -> HashMap<String, MarketplaceConfig> {
    let mut out = HashMap::new();

    for source in [global, project] {
        let Some(obj) = source
            .get("extraKnownMarketplaces")
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (name, entry) in obj {
            match extract_git_url(entry) {
                Some(url) => {
                    let path = PathBuf::from(format!("~/.agentenv/marketplaces/{name}"));
                    out.insert(
                        name.clone(),
                        MarketplaceConfig {
                            path,
                            remote: url,
                            r#ref: "main".to_string(),
                        },
                    );
                },
                None => {
                    tracing::warn!(
                        "skipping Claude marketplace `{name}`: unsupported source shape (need source.url or source.repo)"
                    );
                },
            }
        }
    }

    out
}

/// Pull a git URL out of a Claude `extraKnownMarketplaces` entry, supporting
/// the `git` and `github` source kinds.
fn extract_git_url(entry: &Value) -> Option<String> {
    let source = entry.get("source")?;
    if let Some(url) = source.get("url").and_then(Value::as_str) {
        return Some(url.to_string());
    }
    if let Some(repo) = source.get("repo").and_then(Value::as_str) {
        return Some(format!("https://github.com/{repo}.git"));
    }
    None
}

/// Translate `enabledPlugins` from both files. Keys are `"<name>@<namespace>"`;
/// values are booleans. `false` entries are skipped.
fn merge_plugins(global: &Value, project: &Value) -> Vec<PluginRef> {
    let mut seen: HashMap<(String, String), PluginRef> = HashMap::new();

    for source in [global, project] {
        let Some(obj) = source.get("enabledPlugins").and_then(Value::as_object) else {
            continue;
        };
        for (key, value) in obj {
            if value.as_bool() != Some(true) {
                continue;
            }
            let (name, namespace) = parse_plugin_key(key);
            let dedupe_key = (name.clone(), namespace.clone().unwrap_or_default());
            seen.entry(dedupe_key).or_insert(PluginRef {
                name,
                namespace,
                version: None,
            });
        }
    }

    // Sort for determinism (HashMap iteration is non-deterministic).
    let mut plugins: Vec<PluginRef> = seen.into_values().collect();
    plugins.sort_by(|a, b| a.name.cmp(&b.name).then(a.namespace.cmp(&b.namespace)));
    plugins
}

/// `"plugin-name@marketplace"` → (`"plugin-name"`, `Some("marketplace")`).
/// Bare `"plugin-name"` → (`"plugin-name"`, `None`) (defaults to `"default"`
/// downstream).
fn parse_plugin_key(key: &str) -> (String, Option<String>) {
    if let Some((name, namespace)) = key.split_once('@') {
        (name.to_string(), Some(namespace.to_string()))
    } else {
        (key.to_string(), None)
    }
}

/// Merge `hooks` blocks. Within a hook event (e.g. `PreToolUse`), arrays from
/// both files are concatenated — global hooks come first, then project hooks.
/// This mirrors Claude Code's own resolution: global hooks still fire on top
/// of project hooks.
fn merge_hooks(global: &Value, project: &Value) -> Value {
    let global_hooks = global.get("hooks").cloned();
    let project_hooks = project.get("hooks").cloned();

    match (global_hooks, project_hooks) {
        (None, None) => Value::Null,
        (Some(g), None) => g,
        (None, Some(p)) => p,
        (Some(g), Some(p)) => {
            let (Value::Object(g_obj), Value::Object(p_obj)) = (g, p) else {
                // If either side is not an object, prefer the project value.
                return project.get("hooks").cloned().unwrap_or(Value::Null);
            };
            let mut merged = serde_json::Map::new();
            let mut keys: Vec<&String> = g_obj.keys().chain(p_obj.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                match (g_obj.get(key), p_obj.get(key)) {
                    (Some(Value::Array(g_arr)), Some(Value::Array(p_arr))) => {
                        let mut combined = g_arr.clone();
                        combined.extend(p_arr.clone());
                        merged.insert(key.clone(), Value::Array(combined));
                    },
                    (Some(g_val), None) => {
                        merged.insert(key.clone(), g_val.clone());
                    },
                    (_, Some(p_val)) => {
                        merged.insert(key.clone(), p_val.clone());
                    },
                    (None, None) => {},
                }
            }
            Value::Object(merged)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_settings(dir: &Path, json: &str) {
        let settings_dir = dir.join(".claude");
        fs::create_dir_all(&settings_dir).unwrap();
        fs::write(settings_dir.join("settings.json"), json).unwrap();
    }

    #[test]
    fn translates_extra_known_marketplaces() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_settings(
            project.path(),
            r#"{
                "extraKnownMarketplaces": {
                    "code-plugin-marketplace": {
                        "source": {
                            "source": "git",
                            "url": "https://github.com/example/marketplace.git"
                        }
                    }
                }
            }"#,
        );

        let import = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap();
        let mp = import.marketplaces.get("code-plugin-marketplace").unwrap();
        assert_eq!(mp.remote, "https://github.com/example/marketplace.git");
        assert_eq!(mp.r#ref, "main");
        assert_eq!(
            mp.path.to_string_lossy(),
            "~/.agentenv/marketplaces/code-plugin-marketplace"
        );
    }

    #[test]
    fn translates_enabled_plugins() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_settings(
            project.path(),
            r#"{
                "enabledPlugins": {
                    "typescript-agents@code-plugin-marketplace": true,
                    "disabled-thing@code-plugin-marketplace": false,
                    "bare-plugin": true
                }
            }"#,
        );

        let import = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap();
        assert_eq!(import.plugins.len(), 2);
        let names: Vec<&str> = import.plugins.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"typescript-agents"));
        assert!(names.contains(&"bare-plugin"));

        let ts = import
            .plugins
            .iter()
            .find(|p| p.name == "typescript-agents")
            .unwrap();
        assert_eq!(ts.namespace.as_deref(), Some("code-plugin-marketplace"));

        let bare = import
            .plugins
            .iter()
            .find(|p| p.name == "bare-plugin")
            .unwrap();
        assert!(bare.namespace.is_none());
    }

    #[test]
    fn project_wins_over_global_for_marketplaces() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_settings(
            home.path(),
            r#"{
                "extraKnownMarketplaces": {
                    "shared": {
                        "source": { "source": "git", "url": "https://global.example.com/m.git" }
                    }
                }
            }"#,
        );
        write_settings(
            project.path(),
            r#"{
                "extraKnownMarketplaces": {
                    "shared": {
                        "source": { "source": "git", "url": "https://project.example.com/m.git" }
                    }
                }
            }"#,
        );

        let import = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap();
        let shared = import.marketplaces.get("shared").unwrap();
        assert_eq!(shared.remote, "https://project.example.com/m.git");
    }

    #[test]
    fn unions_plugins_from_global_and_project() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_settings(
            home.path(),
            r#"{ "enabledPlugins": { "global-plugin@m": true } }"#,
        );
        write_settings(
            project.path(),
            r#"{ "enabledPlugins": { "project-plugin@m": true } }"#,
        );

        let import = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap();
        let names: Vec<&str> = import.plugins.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"global-plugin"));
        assert!(names.contains(&"project-plugin"));
        assert_eq!(import.plugins.len(), 2);
    }

    #[test]
    fn dedupes_plugin_appearing_in_both_files() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_settings(home.path(), r#"{ "enabledPlugins": { "shared@m": true } }"#);
        write_settings(
            project.path(),
            r#"{ "enabledPlugins": { "shared@m": true } }"#,
        );

        let import = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap();
        assert_eq!(import.plugins.len(), 1);
    }

    #[test]
    fn merges_hooks_by_concatenating_event_arrays() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_settings(
            home.path(),
            r#"{
                "hooks": {
                    "PreToolUse": [{ "matcher": "Write", "hooks": [{ "type": "command", "command": "echo global" }] }]
                }
            }"#,
        );
        write_settings(
            project.path(),
            r#"{
                "hooks": {
                    "PreToolUse": [{ "matcher": "Edit", "hooks": [{ "type": "command", "command": "echo project" }] }],
                    "Stop": [{ "matcher": ".*", "hooks": [{ "type": "command", "command": "echo stop" }] }]
                }
            }"#,
        );

        let import = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap();
        let pre = import.hooks.get("PreToolUse").unwrap().as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0].get("matcher").unwrap(), "Write");
        assert_eq!(pre[1].get("matcher").unwrap(), "Edit");

        let stop = import.hooks.get("Stop").unwrap().as_array().unwrap();
        assert_eq!(stop.len(), 1);
    }

    #[test]
    fn errors_when_no_settings_file_anywhere() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        let err = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no settings.json found"), "msg was: {msg}");
    }

    #[test]
    fn errors_on_malformed_settings_json() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_settings(project.path(), "{ not valid json");
        let err = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("failed to parse"), "msg was: {msg}");
    }

    #[test]
    fn skips_marketplace_with_unrecognized_source_shape() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_settings(
            project.path(),
            r#"{
                "extraKnownMarketplaces": {
                    "good": { "source": { "source": "git", "url": "https://example.com/g.git" } },
                    "unknown-shape": { "source": { "source": "local", "path": "/somewhere" } }
                }
            }"#,
        );
        let import = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap();
        assert!(import.marketplaces.contains_key("good"));
        assert!(
            !import.marketplaces.contains_key("unknown-shape"),
            "unrecognized source shapes should be silently skipped"
        );
    }

    #[test]
    fn translates_github_repo_source_to_https_url() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_settings(
            project.path(),
            r#"{
                "extraKnownMarketplaces": {
                    "gh-mp": { "source": { "source": "github", "repo": "owner/repo" } }
                }
            }"#,
        );
        let import = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap();
        let mp = import.marketplaces.get("gh-mp").unwrap();
        assert_eq!(mp.remote, "https://github.com/owner/repo.git");
    }

    #[test]
    fn handles_missing_global_with_project_only() {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        write_settings(
            project.path(),
            r#"{ "enabledPlugins": { "only-project": true } }"#,
        );
        let import = ClaudeConfigLoader::load_with_home(project.path(), home.path()).unwrap();
        assert_eq!(import.plugins.len(), 1);
        assert!(import.marketplaces.is_empty());
    }

    fn write_file(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn synthesize_returns_none_when_no_claude_dir() {
        let project = TempDir::new().unwrap();
        assert!(synthesize_local_claude_plugin(project.path()).is_none());
    }

    #[test]
    fn synthesize_returns_none_when_capability_dirs_are_empty() {
        let project = TempDir::new().unwrap();
        std::fs::create_dir_all(project.path().join(".claude/agents")).unwrap();
        std::fs::create_dir_all(project.path().join(".claude/skills")).unwrap();
        // Hidden entries should not count as content.
        write_file(&project.path().join(".claude/agents/.gitkeep"), "");
        assert!(synthesize_local_claude_plugin(project.path()).is_none());
    }

    #[test]
    fn synthesize_picks_up_agents_and_skills() {
        let project = TempDir::new().unwrap();
        write_file(
            &project.path().join(".claude/agents/code-reviewer.md"),
            "---\nname: code-reviewer\n---\nbody",
        );
        write_file(
            &project.path().join(".claude/skills/refactor/SKILL.md"),
            "---\nname: refactor\n---\nbody",
        );
        // commands dir has only hidden entries — should not register
        write_file(&project.path().join(".claude/commands/.gitkeep"), "");

        let plugin = synthesize_local_claude_plugin(project.path()).unwrap();
        assert_eq!(plugin.name, LOCAL_CLAUDE_PLUGIN_NAME);
        assert!(plugin.capabilities.contains(&"agents".to_string()));
        assert!(plugin.capabilities.contains(&"skills".to_string()));
        assert!(!plugin.capabilities.contains(&"commands".to_string()));
        assert!(
            plugin.targets.is_empty(),
            "synthetic plugin should accept all targets"
        );
        assert_eq!(
            plugin.location,
            project.path().join(".claude").to_string_lossy()
        );
    }

    #[test]
    fn synthesize_excludes_capability_when_directory_missing() {
        let project = TempDir::new().unwrap();
        write_file(
            &project.path().join(".claude/agents/only-agent.md"),
            "---\nname: only-agent\n---\nbody",
        );
        let plugin = synthesize_local_claude_plugin(project.path()).unwrap();
        assert_eq!(plugin.capabilities, vec!["agents".to_string()]);
    }

    use crate::config::{CleanConfig, Config, SyncConfig, TargetConfig};

    fn config_with_targets(targets: &[(&str, &str)]) -> Config {
        let mut config = Config {
            version: 1,
            marketplaces: HashMap::new(),
            plugins: vec![],
            targets: HashMap::new(),
            sync: SyncConfig::default(),
            clean: CleanConfig::default(),
            use_claude_config: true,
            gitignore_managed_links: false,
            instruction_files: HashMap::new(),
            claude_hooks: None,
        };
        for (name, ttype) in targets {
            config.targets.insert(
                (*name).to_string(),
                TargetConfig {
                    r#type: (*ttype).to_string(),
                    tools: vec![],
                    paths: HashMap::new(),
                    source_mappings: HashMap::new(),
                },
            );
        }
        config
    }

    #[test]
    fn defaults_prefer_claude_md_then_fall_back_to_agents_md() {
        let project = TempDir::new().unwrap();
        std::fs::write(project.path().join("CLAUDE.md"), "x").unwrap();
        let mut config = config_with_targets(&[("cursor", "cursor")]);
        apply_default_instruction_files(&mut config, project.path());
        assert!(config.instruction_files.contains_key("CLAUDE.md"));
        assert!(!config.instruction_files.contains_key("AGENTS.md"));
    }

    #[test]
    fn defaults_use_agents_md_when_claude_md_absent() {
        let project = TempDir::new().unwrap();
        std::fs::write(project.path().join("AGENTS.md"), "x").unwrap();
        let mut config = config_with_targets(&[("junie", "junie")]);
        apply_default_instruction_files(&mut config, project.path());
        let dests = config
            .instruction_files
            .get("AGENTS.md")
            .expect("AGENTS.md should be the source when no CLAUDE.md");
        assert_eq!(dests, &vec![".junie/AGENTS.md".to_string()]);
    }

    #[test]
    fn defaults_skip_silently_when_no_source_exists() {
        let project = TempDir::new().unwrap();
        let mut config = config_with_targets(&[("cursor", "cursor")]);
        apply_default_instruction_files(&mut config, project.path());
        assert!(config.instruction_files.is_empty());
    }

    #[test]
    fn defaults_skip_when_use_claude_config_is_false() {
        let project = TempDir::new().unwrap();
        std::fs::write(project.path().join("CLAUDE.md"), "x").unwrap();
        let mut config = config_with_targets(&[("cursor", "cursor")]);
        config.use_claude_config = false;
        apply_default_instruction_files(&mut config, project.path());
        assert!(config.instruction_files.is_empty());
    }

    #[test]
    fn defaults_skip_when_user_already_set_instruction_files() {
        let project = TempDir::new().unwrap();
        std::fs::write(project.path().join("CLAUDE.md"), "x").unwrap();
        let mut config = config_with_targets(&[("cursor", "cursor")]);
        config
            .instruction_files
            .insert("MINE.md".to_string(), vec!["TARGET.md".to_string()]);
        apply_default_instruction_files(&mut config, project.path());
        // User block is preserved verbatim; nothing else added.
        assert_eq!(config.instruction_files.len(), 1);
        assert_eq!(
            config.instruction_files.get("MINE.md").unwrap(),
            &vec!["TARGET.md".to_string()]
        );
    }

    #[test]
    fn defaults_union_destinations_across_configured_targets() {
        let project = TempDir::new().unwrap();
        std::fs::write(project.path().join("CLAUDE.md"), "x").unwrap();
        let mut config = config_with_targets(&[
            ("cursor", "cursor"),
            ("junie", "junie"),
            ("antigravity", "antigravity"),
        ]);
        apply_default_instruction_files(&mut config, project.path());
        let mut dests = config.instruction_files.get("CLAUDE.md").unwrap().clone();
        dests.sort();
        assert_eq!(
            dests,
            vec![
                ".junie/AGENTS.md".to_string(),
                "AGENTS.md".to_string(),
                "agents.md".to_string(),
            ]
        );
    }

    #[test]
    fn defaults_omit_destination_equal_to_source() {
        let project = TempDir::new().unwrap();
        // Source is AGENTS.md (CLAUDE.md absent); cursor's default destination
        // is also AGENTS.md — must be skipped to avoid a self-reference.
        std::fs::write(project.path().join("AGENTS.md"), "x").unwrap();
        let mut config = config_with_targets(&[("cursor", "cursor")]);
        apply_default_instruction_files(&mut config, project.path());
        // No destinations means no entry was inserted.
        assert!(config.instruction_files.is_empty());
    }

    #[test]
    fn defaults_for_gemini_cli_include_gemini_md_and_agents_md() {
        let project = TempDir::new().unwrap();
        std::fs::write(project.path().join("CLAUDE.md"), "x").unwrap();
        let mut config = config_with_targets(&[("gemini-cli", "gemini-cli")]);
        apply_default_instruction_files(&mut config, project.path());
        let mut dests = config.instruction_files.get("CLAUDE.md").unwrap().clone();
        dests.sort();
        assert_eq!(
            dests,
            vec!["AGENTS.md".to_string(), "GEMINI.md".to_string()]
        );
    }

    #[test]
    fn defaults_skip_unknown_target_types() {
        let project = TempDir::new().unwrap();
        std::fs::write(project.path().join("CLAUDE.md"), "x").unwrap();
        let mut config = config_with_targets(&[("custom", "custom-tool")]);
        apply_default_instruction_files(&mut config, project.path());
        // Unknown target contributes no destinations.
        assert!(config.instruction_files.is_empty());
    }
}
