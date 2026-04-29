use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn agentenv() -> Command {
    Command::cargo_bin("agentenv").unwrap()
}

/// Append a plugin to a Claude Code-style marketplace at `marketplace`.
fn write_plugin(marketplace: &Path, name: &str, capabilities: &[&str]) {
    let plugin_dir = marketplace.join(name);
    for capability in capabilities {
        let cap_dir = plugin_dir.join(capability);
        fs::create_dir_all(&cap_dir).unwrap();
        if *capability == "skills" {
            let skill_dir = cap_dir.join("demo-skill");
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                "---\nname: demo-skill\ndescription: test\n---\n",
            )
            .unwrap();
        } else {
            fs::write(cap_dir.join("demo-leaf.md"), "leaf body\n").unwrap();
        }
    }

    let claude_dir = marketplace.join(".claude-plugin");
    fs::create_dir_all(&claude_dir).unwrap();
    let index_path = claude_dir.join("marketplace.json");

    let mut entries: Vec<serde_json::Value> = if index_path.exists() {
        let raw = fs::read_to_string(&index_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        value
            .get("plugins")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    entries.push(serde_json::json!({
        "name": name,
        "source": format!("./{name}"),
        "version": "1.0.0",
        "description": name,
    }));

    let index = serde_json::json!({
        "name": "test-marketplace",
        "owner": {"name": "test"},
        "plugins": entries,
    });
    fs::write(&index_path, serde_json::to_string_pretty(&index).unwrap()).unwrap();
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

    assert!(project
        .path()
        .join(".claude/skills/demo-skill")
        .is_symlink());
    assert!(project
        .path()
        .join(".claude/commands/demo-leaf.md")
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

    assert!(project
        .path()
        .join(".claude/skills/demo-skill")
        .is_symlink());
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

fn write_config_with_plugins(project: &Path, marketplace: &Path, plugin_lines: &str) {
    let config = format!(
        r#"version: 1
marketplaces:
  default:
    path: {marketplace}
    remote: https://example.com/marketplace.git
plugins:
{plugin_lines}targets:
  claude-code: {{}}
"#,
        marketplace = marketplace.display()
    );
    fs::write(project.join(".agentrc.yaml"), config).unwrap();
}

#[test]
fn list_prints_marketplaces_plugins_and_targets() {
    let marketplace = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write_plugin(marketplace.path(), "demo", &["skills"]);
    write_config_with_plugins(project.path(), marketplace.path(), "  - name: demo\n");

    agentenv()
        .args(["--project", project.path().to_str().unwrap(), "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Marketplaces:"))
        .stdout(predicate::str::contains("Plugins:"))
        .stdout(predicate::str::contains("demo"))
        .stdout(predicate::str::contains("Targets:"))
        .stdout(predicate::str::contains("claude-code"));
}

#[test]
fn explain_describes_planned_actions_without_writing() {
    let marketplace = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write_plugin(marketplace.path(), "demo", &["skills"]);
    write_config_with_plugins(project.path(), marketplace.path(), "  - name: demo\n");

    agentenv()
        .args(["--project", project.path().to_str().unwrap(), "explain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would link"))
        .stdout(predicate::str::contains("demo-skill"));

    // Explain must not have created links or a state file.
    assert!(!project.path().join(".claude/skills/demo-skill").exists());
    assert!(!project.path().join(".agentenv/state.json").exists());
}

#[test]
fn doctor_passes_after_a_successful_sync() {
    let marketplace = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write_plugin(marketplace.path(), "demo", &["skills"]);
    write_config_with_plugins(project.path(), marketplace.path(), "  - name: demo\n");

    let project_arg = project.path().to_str().unwrap();
    agentenv()
        .args(["--project", project_arg, "sync"])
        .assert()
        .success();
    agentenv()
        .args(["--project", project_arg, "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("all checks passed"));
}

#[test]
fn doctor_fails_when_marketplace_directory_missing() {
    let project = TempDir::new().unwrap();
    let phantom = project.path().join("does/not/exist");
    write_config_with_plugins(project.path(), &phantom, "");

    agentenv()
        .args(["--project", project.path().to_str().unwrap(), "doctor"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("missing"));
}

#[test]
fn clean_removes_links_and_state_after_sync() {
    let marketplace = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write_plugin(marketplace.path(), "demo", &["skills"]);
    write_config_with_plugins(project.path(), marketplace.path(), "  - name: demo\n");

    let project_arg = project.path().to_str().unwrap();
    agentenv()
        .args(["--project", project_arg, "sync"])
        .assert()
        .success();
    assert!(project
        .path()
        .join(".claude/skills/demo-skill")
        .is_symlink());

    agentenv()
        .args(["--project", project_arg, "clean"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));

    assert!(!project.path().join(".claude/skills/demo-skill").exists());
    assert!(!project.path().join(".agentenv/state.json").exists());
}

#[test]
fn sync_removes_stale_links_when_plugin_dropped_from_config() {
    let marketplace = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write_plugin(marketplace.path(), "demo", &["skills"]);
    write_config_with_plugins(project.path(), marketplace.path(), "  - name: demo\n");

    let project_arg = project.path().to_str().unwrap();
    agentenv()
        .args(["--project", project_arg, "sync"])
        .assert()
        .success();
    assert!(project
        .path()
        .join(".claude/skills/demo-skill")
        .is_symlink());

    // Drop the plugin from the config and resync — the stale link should be
    // removed automatically.
    write_config_with_plugins(project.path(), marketplace.path(), "");
    agentenv()
        .args(["--project", project_arg, "sync"])
        .assert()
        .success();

    assert!(!project.path().join(".claude/skills/demo-skill").exists());
}
