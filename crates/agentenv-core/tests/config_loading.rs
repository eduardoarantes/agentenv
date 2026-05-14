//! Integration tests for `.agentrc.yaml` loading under the source-driven
//! model. `TargetConfig` is now an empty struct — opting a target in is
//! enough; per-capability defaults live entirely in the writers.

use agentenv_core::ConfigLoader;

#[test]
fn loads_source_driven_config() {
    let config = ConfigLoader::load_from_string(
        r#"
version: 1
source: claude-code
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/example/marketplace.git
plugins:
  - name: engineering-standards
targets:
  cursor: {}
  codex: {}
sync:
  onOpen: true
"#,
    )
    .unwrap();

    assert_eq!(config.version, 1);
    assert_eq!(config.source.as_deref(), Some("claude-code"));
    assert!(config.marketplaces.contains_key("default"));
    assert_eq!(config.plugins.len(), 1);
    assert!(config.targets.contains_key("cursor"));
    assert!(config.targets.contains_key("codex"));
    assert!(config.sync.on_open);
}

#[test]
fn supports_multiple_marketplaces_and_namespace_helpers() {
    let config = ConfigLoader::load_from_string(
        r#"
version: 1
source: claude-code
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
  cursor: {}
"#,
    )
    .unwrap();

    assert_eq!(config.marketplaces.len(), 2);
    assert_eq!(config.get_plugins_in_namespace("default").len(), 2);
    assert_eq!(config.get_plugins_in_namespace("custom").len(), 1);
}

#[test]
fn rejects_invalid_configurations() {
    let no_marketplaces = ConfigLoader::load_from_string(
        r#"
version: 1
source: claude-code
marketplaces: {}
targets:
  cursor: {}
"#,
    );
    assert!(no_marketplaces.is_err());

    let missing_source = ConfigLoader::load_from_string(
        r#"
version: 1
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/example/marketplace.git
targets:
  cursor: {}
"#,
    );
    assert!(missing_source.is_err());

    let unknown_namespace = ConfigLoader::load_from_string(
        r#"
version: 1
source: claude-code
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/example/marketplace.git
plugins:
  - name: my-plugin
    namespace: unknown
targets:
  cursor: {}
"#,
    );
    assert!(unknown_namespace.is_err());

    let unknown_target = ConfigLoader::load_from_string(
        r#"
version: 1
source: claude-code
marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/example/marketplace.git
targets:
  no-such-thing: {}
"#,
    );
    assert!(unknown_target.is_err());
}
