//! End-to-end integration tests for the source-driven hooks pipeline.
//!
//! Walks the full path: `source: claude-code` + `claude_hooks` populated →
//! canonical YAML on disk → `.cursor/hooks.json` rendered → `~/.codex/config.toml`
//! `[notify]` block + dispatcher script written. Also exercises the
//! refuse-on-conflict gate.

use agentenv_core::config::{Config, MarketplaceConfig, TargetConfig};
use agentenv_core::hooks::{
    canonical_io, pipeline,
    writers::{codex as codex_writer, cursor as cursor_writer},
};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

/// Serialize tests that need an isolated $HOME (codex writer reads it via
/// `dirs::home_dir()`, which goes through process-global env state).
static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn base_config() -> Config {
    let mut marketplaces = HashMap::new();
    marketplaces.insert(
        "default".to_string(),
        MarketplaceConfig {
            path: PathBuf::from("~/.agentenv/marketplace"),
            remote: "https://example.com/m.git".to_string(),
            r#ref: "main".to_string(),
        },
    );
    Config {
        version: 1,
        marketplaces,
        plugins: vec![],
        targets: HashMap::new(),
        sync: Default::default(),
        clean: Default::default(),
        use_claude_config: true,
        gitignore_managed_links: false,
        instruction_files: HashMap::new(),
        claude_hooks: None,
        source: None,
    }
}

fn empty_target() -> TargetConfig {
    TargetConfig {
        r#type: String::new(),
        tools: vec![],
        paths: HashMap::new(),
        source_mappings: HashMap::new(),
    }
}

#[test]
fn end_to_end_materializes_claude_hooks_to_cursor_and_codex() {
    let _guard = HOME_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let saved = std::env::var_os("HOME");
    std::env::set_var("HOME", home.path());

    let mut config = base_config();
    config.source = Some("claude-code".to_string());
    config.targets.insert("cursor".to_string(), empty_target());
    config.targets.insert("codex".to_string(), empty_target());
    config.claude_hooks = Some(serde_json::json!({
        "PreToolUse": [
            {"matcher": "Bash", "hooks": [
                {"type": "command", "command": "echo bash"}
            ]}
        ],
        "Stop": [
            {"matcher": ".*", "hooks": [
                {"type": "command", "command": "notify-done"}
            ]}
        ],
        "PreCompact": [
            {"matcher": ".*", "hooks": [
                {"type": "command", "command": "echo compact"}
            ]}
        ]
    }));

    let report = pipeline::run(&config, project.path()).expect("pipeline succeeds");

    // Canonical artifact landed.
    let canonical_path = report.canonical_path.expect("canonical written");
    assert_eq!(canonical_path, canonical_io::path(project.path()));
    assert!(canonical_path.exists());

    // Cursor: PreToolUse + Stop rendered (PreCompact dropped with warning).
    let cursor_dest = cursor_writer::destination(project.path());
    assert!(cursor_dest.exists(), "missing {}", cursor_dest.display());
    let cursor_body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cursor_dest).unwrap()).unwrap();
    assert_eq!(
        cursor_body[cursor_writer::AGENTENV_MARKER_KEY],
        "managed",
        "marker missing"
    );
    assert!(cursor_body["hooks"]["PreToolUse"].is_array());
    assert!(cursor_body["hooks"]["Stop"].is_array());

    // Codex: notify block written; dispatcher script in project's .agentenv.
    let codex_config = home.path().join(".codex/config.toml");
    assert!(codex_config.exists());
    let codex_body = std::fs::read_to_string(&codex_config).unwrap();
    assert!(codex_body.contains(codex_writer::BEGIN_MARKER));
    assert!(codex_body.contains("notify"));
    let dispatcher = codex_writer::dispatcher_path(project.path());
    assert!(dispatcher.exists());
    let dispatch_body = std::fs::read_to_string(&dispatcher).unwrap();
    assert!(dispatch_body.contains("notify-done"));

    // Drop warnings surfaced from BOTH writers (PreCompact has no cursor or
    // codex counterpart; PreToolUse has no codex counterpart).
    let warnings = report.warnings.join("\n");
    assert!(warnings.contains("PreCompact"), "warnings: {warnings}");
    assert!(warnings.contains("PreToolUse"), "warnings: {warnings}");

    match saved {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn second_run_is_idempotent_on_cursor() {
    let _guard = HOME_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let saved = std::env::var_os("HOME");
    std::env::set_var("HOME", home.path());

    let mut config = base_config();
    config.source = Some("claude-code".to_string());
    config.targets.insert("cursor".to_string(), empty_target());
    config.claude_hooks = Some(serde_json::json!({
        "Stop": [{"matcher": ".*", "hooks": [
            {"type": "command", "command": "x"}
        ]}]
    }));

    pipeline::run(&config, project.path()).unwrap();
    let first = std::fs::read_to_string(cursor_writer::destination(project.path())).unwrap();

    pipeline::run(&config, project.path()).unwrap();
    let second = std::fs::read_to_string(cursor_writer::destination(project.path())).unwrap();

    assert_eq!(first, second, "second pipeline run must be byte-identical");

    match saved {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn refuses_when_cursor_file_is_user_authored() {
    let project = TempDir::new().unwrap();
    let dest = cursor_writer::destination(project.path());
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(
        &dest,
        r#"{ "hooks": { "Stop": [{"matcher": ".*", "hooks": []}] } }"#,
    )
    .unwrap();

    let mut config = base_config();
    config.source = Some("claude-code".to_string());
    config.targets.insert("cursor".to_string(), empty_target());
    config.claude_hooks = Some(serde_json::json!({
        "Stop": [{"matcher": ".*", "hooks": [
            {"type": "command", "command": "would-overwrite"}
        ]}]
    }));

    let err = pipeline::run(&config, project.path()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("user-authored"), "got: {msg}");

    // User file preserved verbatim.
    let after = std::fs::read_to_string(&dest).unwrap();
    assert!(after.contains("\"Stop\""), "user file mutated: {after}");
    assert!(
        !after.contains("would-overwrite"),
        "writer ran anyway: {after}"
    );
}

#[test]
fn source_unset_is_a_silent_noop_even_with_cursor_target() {
    let project = TempDir::new().unwrap();
    let mut config = base_config();
    config.targets.insert("cursor".to_string(), empty_target());
    config.claude_hooks = Some(serde_json::json!({
        "Stop": [{"matcher": ".*", "hooks": [
            {"type": "command", "command": "ignored"}
        ]}]
    }));

    let report = pipeline::run(&config, project.path()).unwrap();
    assert!(report.canonical_path.is_none());
    assert!(!cursor_writer::destination(project.path()).exists());
}

#[test]
fn lossless_canonical_round_trip_preserves_native_claude_events() {
    let project = TempDir::new().unwrap();
    let mut config = base_config();
    config.source = Some("claude-code".to_string());
    config.targets.insert("cursor".to_string(), empty_target());

    let raw_payload = serde_json::json!({
        "matcher": "*",
        "hooks": [{"type": "command", "command": "claude-specific"}]
    });
    config.claude_hooks = Some(serde_json::json!({
        "TeammateIdle": [raw_payload.clone()]
    }));

    pipeline::run(&config, project.path()).unwrap();

    let canonical = canonical_io::read(project.path()).unwrap().unwrap();
    assert_eq!(canonical.source, "claude-code");
    assert_eq!(canonical.hooks.len(), 1);
    let only = &canonical.hooks[0];
    let agentenv_core::hooks::Event::Native(native) = &only.event else {
        panic!("expected Native, got: {:?}", only.event);
    };
    assert_eq!(native.source, "claude-code");
    assert_eq!(native.native_event, "TeammateIdle");
    assert_eq!(native.payload, raw_payload);
}
