use agentenv_core::ConfigLoader;

#[test]
fn loads_config_with_defaults() {
    let config = ConfigLoader::load_from_string(
        r#"
version: 1
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/example/marketplace.git
plugins:
  - name: engineering-standards
targets:
  claude-code: {}
sync:
  onOpen: true
"#,
    )
    .unwrap();

    let target = config.get_target("claude-code").unwrap();
    assert_eq!(config.version, 1);
    assert!(config.marketplaces.contains_key("default"));
    assert_eq!(config.plugins.len(), 1);
    assert_eq!(target.r#type, "vscode-extension");
    assert_eq!(target.tools, vec!["claude-code"]);
    assert!(target.source_mappings.contains_key("skills"));
    assert!(config.sync.on_open);
    assert_eq!(config.sync.mode, "symlink");
}

#[test]
fn supports_multiple_marketplaces_and_namespace_helpers() {
    let config = ConfigLoader::load_from_string(
        r#"
version: 1
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/example/marketplace.git
  custom:
    path: ~/.agentenv/custom
    remote: https://custom.com/marketplace.git
plugins:
  - name: plugin1
    namespace: default
  - name: plugin2
    namespace: default
  - name: plugin3
    namespace: custom
targets:
  claude-code: {}
"#,
    )
    .unwrap();

    assert_eq!(config.marketplaces.len(), 2);
    assert_eq!(config.get_plugins_in_namespace("default").len(), 2);
    assert_eq!(config.get_plugins_in_namespace("custom").len(), 1);
}

#[test]
fn user_target_config_overrides_defaults_selectively() {
    let config = ConfigLoader::load_from_string(
        r#"
version: 1
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/example/marketplace.git
targets:
  claude-code:
    paths:
      config: /custom/vscode/path
    source_mappings:
      skills:
        - source: ~/.agentenv/marketplace/custom-skills
          target: .claude-code/custom-skills
"#,
    )
    .unwrap();

    let target = config.get_target("claude-code").unwrap();
    assert_eq!(
        target.paths.get("config").map(String::as_str),
        Some("/custom/vscode/path")
    );
    assert_eq!(
        target.get_mappings("skills").unwrap()[0]
            .target
            .to_string_lossy(),
        ".claude-code/custom-skills"
    );
    assert!(target.get_mappings("commands").is_some());
    assert!(target.get_mappings("agents").is_some());
}

#[test]
fn rejects_invalid_configurations() {
    let no_marketplaces = ConfigLoader::load_from_string(
        r#"
version: 1
marketplaces: {}
targets:
  claude-code: {}
"#,
    );
    assert!(no_marketplaces.is_err());

    let unknown_namespace = ConfigLoader::load_from_string(
        r#"
version: 1
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/example/marketplace.git
plugins:
  - name: my-plugin
    namespace: unknown
targets:
  claude-code: {}
"#,
    );
    assert!(unknown_namespace.is_err());
}
