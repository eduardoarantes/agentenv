use agentenv_core::{ConfigLoader, Error, PluginResolver};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Plugin entry to register in the marketplace index.
struct PluginSpec<'a> {
    name: &'a str,
    version: &'a str,
    capabilities: &'a [&'a str],
}

/// Build a Claude Code-style marketplace at `marketplace`:
///
/// - `<marketplace>/.claude-plugin/marketplace.json` lists every plugin.
/// - Each plugin lives at `<marketplace>/<name>/` with empty capability subdirs
///   matching `KNOWN_CAPABILITIES` in agentenv-core.
fn write_marketplace(marketplace: &Path, plugins: &[PluginSpec<'_>]) {
    let claude_dir = marketplace.join(".claude-plugin");
    fs::create_dir_all(&claude_dir).unwrap();

    let entries: Vec<String> = plugins
        .iter()
        .map(|p| {
            format!(
                r#"    {{"name":"{name}","source":"./{name}","version":"{version}","description":"{name}"}}"#,
                name = p.name,
                version = p.version,
            )
        })
        .collect();

    fs::write(
        claude_dir.join("marketplace.json"),
        format!(
            r#"{{
  "name": "test-marketplace",
  "owner": {{"name": "test"}},
  "plugins": [
{}
  ]
}}"#,
            entries.join(",\n")
        ),
    )
    .unwrap();

    for plugin in plugins {
        let plugin_dir = marketplace.join(plugin.name);
        for capability in plugin.capabilities {
            fs::create_dir_all(plugin_dir.join(capability)).unwrap();
        }
    }
}

fn config_yaml(marketplace_path: &Path, plugins: &str, targets: &str) -> String {
    format!(
        r#"
version: 1
marketplaces:
  default:
    path: {}
    remote: https://example.com/marketplace.git
plugins:
{}
targets:
{}
"#,
        marketplace_path.display(),
        plugins,
        targets
    )
}

#[test]
fn resolves_plugins_from_local_marketplace() {
    let temp = TempDir::new().unwrap();
    write_marketplace(
        temp.path(),
        &[PluginSpec {
            name: "my-plugin",
            version: "1.2.3",
            capabilities: &["skills", "commands"],
        }],
    );

    let config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: my-plugin",
        "  claude-code: {}",
    ))
    .unwrap();
    let resolved = PluginResolver::resolve_all(&config, temp.path()).unwrap();

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, "my-plugin");
    assert_eq!(resolved[0].version, "1.2.3");
    assert_eq!(resolved[0].namespace, "default");
    assert!(Path::new(&resolved[0].location).ends_with("my-plugin"));
    let mut capabilities = resolved[0].capabilities.clone();
    capabilities.sort();
    assert_eq!(capabilities, vec!["commands", "skills"]);
}

#[test]
fn errors_when_selected_plugin_is_missing() {
    let temp = TempDir::new().unwrap();
    write_marketplace(temp.path(), &[]);
    let config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: missing-plugin",
        "  claude-code: {}",
    ))
    .unwrap();

    let err = PluginResolver::resolve_all(&config, temp.path()).unwrap_err();
    assert!(matches!(err, Error::PluginResolution(_)));
    assert!(err.to_string().contains("missing-plugin"));
}

#[test]
fn validates_explicit_plugin_version() {
    let temp = TempDir::new().unwrap();
    write_marketplace(
        temp.path(),
        &[PluginSpec {
            name: "versioned-plugin",
            version: "2.0.0",
            capabilities: &["skills"],
        }],
    );

    let ok_config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: versioned-plugin\n    version: 2.0.0",
        "  claude-code: {}",
    ))
    .unwrap();
    assert_eq!(
        PluginResolver::resolve_all(&ok_config, temp.path()).unwrap()[0].version,
        "2.0.0"
    );

    let bad_config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: versioned-plugin\n    version: 1.0.0",
        "  claude-code: {}",
    ))
    .unwrap();
    let err = PluginResolver::resolve_all(&bad_config, temp.path()).unwrap_err();
    assert!(err.to_string().contains("requested version 1.0.0"));
}

#[test]
fn rejects_marketplace_without_index() {
    let temp = TempDir::new().unwrap();
    // No `.claude-plugin/marketplace.json`.
    let config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: anything",
        "  claude-code: {}",
    ))
    .unwrap();

    let err = PluginResolver::resolve_all(&config, temp.path()).unwrap_err();
    assert!(err.to_string().contains("marketplace index not found"));
}

#[test]
fn rejects_index_pointing_at_missing_source() {
    let temp = TempDir::new().unwrap();
    let claude_dir = temp.path().join(".claude-plugin");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("marketplace.json"),
        r#"{
  "name": "test",
  "plugins": [{"name":"ghost","source":"./ghost","version":"1.0.0","description":"ghost"}]
}"#,
    )
    .unwrap();

    let config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: ghost",
        "  claude-code: {}",
    ))
    .unwrap();

    let err = PluginResolver::resolve_all(&config, temp.path()).unwrap_err();
    assert!(err.to_string().contains("missing source directory"));
}

#[test]
fn capabilities_are_inferred_from_filesystem() {
    let temp = TempDir::new().unwrap();
    // Plugin has only `commands/` — `skills/` and `agents/` should not appear.
    write_marketplace(
        temp.path(),
        &[PluginSpec {
            name: "commands-only",
            version: "1.0.0",
            capabilities: &["commands"],
        }],
    );

    let config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: commands-only",
        "  claude-code: {}",
    ))
    .unwrap();

    let resolved = PluginResolver::resolve_all(&config, temp.path()).unwrap();
    assert_eq!(resolved[0].capabilities, vec!["commands"]);
}
