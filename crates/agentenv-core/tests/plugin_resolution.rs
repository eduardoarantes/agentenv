use agentenv_core::{ConfigLoader, Error, PluginResolver};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_plugin(
    marketplace: &Path,
    name: &str,
    version: &str,
    targets: &[&str],
    capabilities: &[&str],
    metadata: &str,
) {
    let plugin_dir = marketplace.join("plugins").join(name);
    let manifest_dir = plugin_dir.join(".claude-plugin");
    fs::create_dir_all(&manifest_dir).unwrap();

    for capability in capabilities {
        fs::create_dir_all(plugin_dir.join(capability)).unwrap();
    }

    let targets_json = targets
        .iter()
        .map(|target| format!(r#""{target}""#))
        .collect::<Vec<_>>()
        .join(", ");
    let capabilities_json = capabilities
        .iter()
        .map(|capability| format!(r#""{capability}""#))
        .collect::<Vec<_>>()
        .join(", ");

    fs::write(
        manifest_dir.join("plugin.json"),
        format!(
            r#"{{
  "name": "{name}",
  "version": "{version}",
  "description": "{name} description",
  "targets": [{targets_json}],
  "capabilities": [{capabilities_json}],
  "metadata": {metadata}
}}"#
        ),
    )
    .unwrap();
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
fn resolves_plugins_from_local_marketplace_manifests() {
    let temp = TempDir::new().unwrap();
    write_plugin(
        temp.path(),
        "my-plugin",
        "1.2.3",
        &["claude-code"],
        &["skills", "commands"],
        r#"{"category":"engineering"}"#,
    );

    let config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: my-plugin",
        "  claude-code: {}",
    ))
    .unwrap();
    let resolved = PluginResolver::resolve_all(&config).unwrap();

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, "my-plugin");
    assert_eq!(resolved[0].version, "1.2.3");
    assert_eq!(resolved[0].namespace, "default");
    assert!(Path::new(&resolved[0].location).ends_with("plugins/my-plugin"));
    assert_eq!(resolved[0].metadata["category"], "engineering");
    assert_eq!(resolved[0].capabilities, vec!["skills", "commands"]);
}

#[test]
fn errors_when_selected_plugin_is_missing() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("plugins")).unwrap();
    let config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: missing-plugin",
        "  claude-code: {}",
    ))
    .unwrap();

    let err = PluginResolver::resolve_all(&config).unwrap_err();
    assert!(matches!(err, Error::PluginResolution(_)));
    assert!(err.to_string().contains("missing-plugin"));
}

#[test]
fn validates_explicit_plugin_version() {
    let temp = TempDir::new().unwrap();
    write_plugin(
        temp.path(),
        "versioned-plugin",
        "2.0.0",
        &["claude-code"],
        &["skills"],
        "{}",
    );

    let ok_config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: versioned-plugin\n    version: 2.0.0",
        "  claude-code: {}",
    ))
    .unwrap();
    assert_eq!(
        PluginResolver::resolve_all(&ok_config).unwrap()[0].version,
        "2.0.0"
    );

    let bad_config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: versioned-plugin\n    version: 1.0.0",
        "  claude-code: {}",
    ))
    .unwrap();
    let err = PluginResolver::resolve_all(&bad_config).unwrap_err();
    assert!(err.to_string().contains("requested version 1.0.0"));
}

#[test]
fn rejects_plugin_without_manifest() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("plugins").join("broken-plugin")).unwrap();

    let config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: broken-plugin",
        "  claude-code: {}",
    ))
    .unwrap();

    let err = PluginResolver::resolve_all(&config).unwrap_err();
    assert!(err.to_string().contains("manifest"));
}

#[test]
fn rejects_plugin_with_unsupported_target() {
    let temp = TempDir::new().unwrap();
    write_plugin(
        temp.path(),
        "cursor-only",
        "1.0.0",
        &["cursor"],
        &["skills"],
        "{}",
    );

    let config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: cursor-only",
        "  claude-code: {}",
    ))
    .unwrap();

    let err = PluginResolver::resolve_all(&config).unwrap_err();
    assert!(err
        .to_string()
        .contains("does not support configured targets"));
}

#[test]
fn rejects_plugin_with_missing_capability_folder() {
    let temp = TempDir::new().unwrap();
    write_plugin(
        temp.path(),
        "missing-folder",
        "1.0.0",
        &["claude-code"],
        &["skills"],
        "{}",
    );
    fs::remove_dir_all(temp.path().join("plugins/missing-folder/skills")).unwrap();

    let config = ConfigLoader::load_from_string(&config_yaml(
        temp.path(),
        "  - name: missing-folder",
        "  claude-code: {}",
    ))
    .unwrap();

    let err = PluginResolver::resolve_all(&config).unwrap_err();
    assert!(err.to_string().contains("missing capability folder"));
}
