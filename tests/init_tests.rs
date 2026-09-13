#[cfg(unix)]
mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;

// Each command starts a fresh process with output modes reset and no credentials.
fn floo(project: &Path) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("floo-local"));
    command
        .current_dir(project)
        .env("FLOO_CONFIG_DIR", project.join("isolated-config"))
        .env("FLOO_NO_UPDATE_CHECK", "1");
    command
}

#[test]
fn init_refuses_existing_service_config_without_writing_files() {
    let project = tempfile::tempdir().unwrap();
    let service = "[app]\nname = 'existing'\n[service]\nname = 'web'\ntype = 'web'\nport = 8080\n";
    std::fs::write(project.path().join("floo.service.toml"), service).unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"dependencies":{"next":"14"}}"#,
    )
    .unwrap();

    floo(project.path())
        .args(["init", "myapp", "--json"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("floo.service.toml already exists"))
        .stdout(predicate::str::contains("cannot combine"));

    assert_eq!(
        std::fs::read_to_string(project.path().join("floo.service.toml")).unwrap(),
        service
    );
    assert!(!project.path().join("floo.app.toml").exists());
    assert!(!project.path().join("Dockerfile").exists());
    assert!(!project.path().join("AGENTS.md").exists());
}

#[test]
fn init_json_manifest_round_trips_through_preflight() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"dependencies":{"next":"14"}}"#,
    )
    .unwrap();

    std::fs::write(
        project.path().join("package-lock.json"),
        r#"{"lockfileVersion":3}"#,
    )
    .unwrap();

    let init = floo(project.path())
        .args(["init", "myapp", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let init: serde_json::Value = serde_json::from_slice(&init).unwrap();
    let preflight = floo(project.path())
        .args(["preflight", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preflight: serde_json::Value = serde_json::from_slice(&preflight).unwrap();

    assert_eq!(init["success"], true);
    assert_eq!(preflight["data"]["valid"], true);
    assert_eq!(preflight["data"]["app"], init["data"]["app_name"]);
    assert_eq!(
        preflight["data"]["services"][0]["name"],
        init["data"]["service"]["name"]
    );
    assert_eq!(
        preflight["data"]["services"][0]["port"],
        init["data"]["service"]["port"]
    );
}

#[cfg(unix)]
fn interactive_init(project: &Path, answers: &str) -> assert_cmd::assert::Assert {
    use assert_cmd::assert::OutputAssertExt;
    use std::io::Write;
    let (mut master, slave) = support::stdout_terminal();
    master.write_all(answers.as_bytes()).unwrap();
    master.write_all(b"\x04").unwrap();
    let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin!("floo-local"));
    command
        .args(["init", "myapp"])
        .current_dir(project)
        .env("FLOO_CONFIG_DIR", project.join("isolated-config"))
        .env("FLOO_NO_UPDATE_CHECK", "1")
        .stdin(slave);
    command.output().unwrap().assert()
}

#[test]
#[cfg(unix)]
fn shared_path_worker_prompts_for_and_persists_production_command() {
    let project = tempfile::tempdir().unwrap();
    interactive_init(
        project.path(),
        "\ny\nweb\n.\n8080\nweb\ny\nqueue\n./\n8080\nworker\nn\npython -m worker\nn\n",
    )
    .success()
    .stderr(predicate::str::contains(
        "Production command for worker 'queue'",
    ));

    let content = std::fs::read_to_string(project.path().join("floo.app.toml")).unwrap();
    let manifest: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(
        manifest["services"]["queue"]["command"].as_str(),
        Some("python -m worker")
    );
    floo(project.path())
        .args(["preflight", "--json"])
        .assert()
        .success();
}

#[test]
#[cfg(unix)]
fn worker_entered_before_web_still_prompts_for_command() {
    let project = tempfile::tempdir().unwrap();
    interactive_init(
        project.path(),
        "\ny\nqueue\n.\n8080\nworker\ny\nweb\n.\n8080\nweb\nn\npython -m worker\nn\n",
    )
    .success()
    .stderr(predicate::str::contains(
        "Production command for worker 'queue'",
    ));
    floo(project.path())
        .args(["preflight", "--json"])
        .assert()
        .success();
}

#[test]
#[cfg(unix)]
fn duplicate_service_name_fails_without_writing_scaffold() {
    let project = tempfile::tempdir().unwrap();
    interactive_init(project.path(), "\ny\nweb\n.\n8080\nweb\ny\nweb\n")
        .failure()
        .stderr(predicate::str::contains("Multiple services named 'web'"));
    assert!(!project.path().join("floo.app.toml").exists());
    assert!(!project.path().join("AGENTS.md").exists());
}

#[test]
#[cfg(unix)]
fn blank_shared_worker_command_fails_validation_before_writing() {
    let project = tempfile::tempdir().unwrap();
    interactive_init(
        project.path(),
        "\ny\nweb\n.\n8080\nweb\ny\nqueue\n.\n8080\nworker\nn\n  \n",
    )
    .failure()
    .stderr(predicate::str::contains("declares no production 'command'"));
    assert!(!project.path().join("floo.app.toml").exists());
    assert!(!project.path().join("AGENTS.md").exists());
}

#[test]
#[cfg(unix)]
fn candidate_discovery_rejects_service_file_in_inline_subdirectory() {
    let project = tempfile::tempdir().unwrap();
    let subdir = project.path().join("web");
    std::fs::create_dir(&subdir).unwrap();
    std::fs::write(subdir.join("floo.service.toml"), "existing service config").unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"dependencies":{"next":"14"}}"#,
    )
    .unwrap();

    interactive_init(project.path(), "\ny\nweb\nweb\n8080\nweb\nn\n")
        .failure()
        .stderr(predicate::str::contains("defined inline"))
        .stderr(predicate::str::contains("floo.service.toml' also exists"));
    assert!(!project.path().join("floo.app.toml").exists());
    assert!(!project.path().join("Dockerfile").exists());
    assert!(!project.path().join("AGENTS.md").exists());
}

#[test]
#[cfg(unix)]
fn worker_with_its_own_build_does_not_prompt_for_command() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("queue")).unwrap();
    interactive_init(
        project.path(),
        "\ny\nweb\n.\n8080\nweb\ny\nqueue\nqueue\n8080\nworker\nn\nn\n",
    )
    .success()
    .stderr(predicate::str::contains("Production command").not());
    floo(project.path())
        .args(["preflight", "--json"])
        .assert()
        .success();
}
