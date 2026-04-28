use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn agentenv() -> Command {
    Command::cargo_bin("agentenv").unwrap()
}

fn write_plugin(marketplace: &Path, name: &str, capabilities: &[&str]) {
    let plugin_dir = marketplace.join("plugins").join(name);
    let manifest_dir = plugin_dir.join(".claude-plugin");
    fs::create_dir_all(&manifest_dir).unwrap();
    for capability in capabilities {
        fs::create_dir_all(plugin_dir.join(capability)).unwrap();
    }
    let capabilities_json = capabilities
        .iter()
        .map(|c| format!(r#""{c}""#))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        manifest_dir.join("plugin.json"),
        format!(
            r#"{{"name":"{name}","version":"1.0.0","description":"{name}","targets":["claude-code"],"capabilities":[{capabilities_json}],"metadata":{{}}}}"#
        ),
    )
    .unwrap();
}

#[test]
fn init_writes_starter_config() {
    let project = TempDir::new().unwrap();

    agentenv()
        .args(["--project", project.path().to_str().unwrap(), "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"));

    let config = project.path().join(".agentrc.yaml");
    assert!(config.exists());
    let contents = fs::read_to_string(&config).unwrap();
    assert!(contents.contains("version: 1"));
}

#[test]
fn init_refuses_to_overwrite_without_force() {
    let project = TempDir::new().unwrap();
    fs::write(project.path().join(".agentrc.yaml"), "version: 99\n").unwrap();

    agentenv()
        .args(["--project", project.path().to_str().unwrap(), "init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    let contents = fs::read_to_string(project.path().join(".agentrc.yaml")).unwrap();
    assert_eq!(contents, "version: 99\n");
}

#[test]
fn init_force_overwrites_existing_config() {
    let project = TempDir::new().unwrap();
    fs::write(project.path().join(".agentrc.yaml"), "version: 99\n").unwrap();

    agentenv()
        .args([
            "--project",
            project.path().to_str().unwrap(),
            "init",
            "--force",
        ])
        .assert()
        .success();

    let contents = fs::read_to_string(project.path().join(".agentrc.yaml")).unwrap();
    assert!(contents.contains("version: 1"));
}

#[test]
fn sync_links_local_marketplace_plugin_into_target() {
    let marketplace = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write_plugin(marketplace.path(), "demo", &["skills", "commands"]);

    let config = format!(
        r#"version: 1
marketplaces:
  default:
    path: {marketplace}
    remote: https://example.com/marketplace.git
plugins:
  - name: demo
targets:
  claude-code: {{}}
"#,
        marketplace = marketplace.path().display()
    );
    fs::write(project.path().join(".agentrc.yaml"), config).unwrap();

    agentenv()
        .args(["--project", project.path().to_str().unwrap(), "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("linked"));

    assert!(project.path().join(".claude-code/skills/demo").is_symlink());
    assert!(project
        .path()
        .join(".claude-code/commands/demo")
        .is_symlink());
}

#[test]
fn sync_is_idempotent_via_cli() {
    let marketplace = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write_plugin(marketplace.path(), "demo", &["skills"]);

    let config = format!(
        r#"version: 1
marketplaces:
  default:
    path: {marketplace}
    remote: https://example.com/marketplace.git
plugins:
  - name: demo
targets:
  claude-code: {{}}
"#,
        marketplace = marketplace.path().display()
    );
    fs::write(project.path().join(".agentrc.yaml"), config).unwrap();

    let project_arg = project.path().to_str().unwrap();
    agentenv()
        .args(["--project", project_arg, "sync"])
        .assert()
        .success();
    agentenv()
        .args(["--project", project_arg, "sync"])
        .assert()
        .success();

    assert!(project.path().join(".claude-code/skills/demo").is_symlink());
}

#[test]
fn sync_fails_when_config_missing() {
    let project = TempDir::new().unwrap();
    agentenv()
        .args(["--project", project.path().to_str().unwrap(), "sync"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(".agentrc.yaml"));
}
