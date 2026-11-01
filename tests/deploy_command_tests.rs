//! `floo deploy`: local readiness checks, project creation, push and watch.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use mockito::{Matcher, Server};
use predicates::prelude::*;
use serde_json::{json, Value};
use tempfile::TempDir;

const APP_ID: &str = "01234567-89ab-cdef-0123-456789abcdef";
const REPO: &str =
    "floo-managed/floo-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-0123456789abcdef0123456789abcdef";

fn project() -> Value {
    json!({
        "app_id": APP_ID,
        "name": "client-portal",
        "app_url": "https://client-portal.example.test",
        "repo_full_name": REPO,
        "clone_url": format!("https://github.com/{REPO}.git"),
        "default_branch": "main",
        "first_deploy_id": "deploy-starter",
    })
}

struct Env {
    home: TempDir,
    repo: TempDir,
    bin: PathBuf,
}

fn real_git() -> String {
    let output = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// A git that runs the real git for everything except `push`, which it logs.
fn setup(server: &Server) -> Env {
    let home = TempDir::new().unwrap();
    std::fs::write(
        home.path().join("config.json"),
        json!({
            "api_key": "floo_test123",
            "api_url": server.url(),
            "default_org": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        })
        .to_string(),
    )
    .unwrap();
    let bin = home.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let shim = bin.join("git");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = push ]; then\n    printf '%s\\n' \"$@\" > \"$PUSH_ARGS\"\n    exit \"${{PUSH_EXIT:-0}}\"\n  fi\ndone\nexec {} \"$@\"\n",
            real_git()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    Env {
        home,
        repo: TempDir::new().unwrap(),
        bin,
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new(real_git())
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.test")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.test")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn write_config(repo: &Path) {
    std::fs::write(
        repo.join("floo.app.toml"),
        "[app]\nname = \"client-portal\"\naccess_mode = \"accounts\"\n",
    )
    .unwrap();
}

fn committed_repo(env: &Env) -> String {
    git(env.repo.path(), &["init", "-q", "-b", "main"]);
    write_config(env.repo.path());
    git(env.repo.path(), &["add", "-A"]);
    git(env.repo.path(), &["commit", "-q", "-m", "init"]);
    git(env.repo.path(), &["rev-parse", "HEAD"])
}

#[allow(deprecated)]
fn floo(env: &Env) -> Command {
    let mut command = Command::cargo_bin("floo-local").unwrap();
    let path = format!(
        "{}:{}",
        env.bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    command
        .env("HOME", env.home.path())
        .env("FLOO_CONFIG_DIR", env.home.path())
        .env_remove("FLOO_API_URL")
        .env("FLOO_NO_UPDATE_CHECK", "1")
        .env("PATH", path)
        .env("PUSH_ARGS", env.home.path().join("push-args"))
        .arg("deploy")
        .arg(env.repo.path());
    command
}

fn error(output: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(output).unwrap();
    assert_eq!(value["success"], false);
    value["error"].clone()
}

#[test]
fn stops_with_the_next_command_when_the_folder_is_not_ready() {
    let server = Server::new();
    let env = setup(&server);

    // No floo.app.toml.
    let out = floo(&env)
        .arg("--json")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let err = error(&out);
    assert_eq!(err["code"], "NO_CONFIG_FOUND");
    assert!(err["suggestion"]
        .as_str()
        .unwrap()
        .starts_with("floo init <app-name> --path "));

    // Not a git repository.
    write_config(env.repo.path());
    let out = floo(&env)
        .arg("--json")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let err = error(&out);
    assert_eq!(err["code"], "NOT_A_GIT_REPOSITORY");
    assert!(err["suggestion"]
        .as_str()
        .unwrap()
        .contains(" init && git -C "));

    // No commits.
    git(env.repo.path(), &["init", "-q", "-b", "main"]);
    let out = floo(&env)
        .arg("--json")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    assert_eq!(error(&out)["code"], "NO_COMMITS");

    // Uncommitted changes.
    git(env.repo.path(), &["add", "-A"]);
    git(env.repo.path(), &["commit", "-q", "-m", "init"]);
    std::fs::write(env.repo.path().join("app.js"), "x").unwrap();
    let out = floo(&env)
        .arg("--json")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let err = error(&out);
    assert_eq!(err["code"], "UNCOMMITTED_CHANGES");
    assert!(err["suggestion"]
        .as_str()
        .unwrap()
        .contains("add -A && git -C "));

    // A 'floo' remote that is not a floo-hosted repository.
    std::fs::remove_file(env.repo.path().join("app.js")).unwrap();
    git(
        env.repo.path(),
        &["remote", "add", "floo", "https://github.com/owner/repo.git"],
    );
    let out = floo(&env)
        .arg("--json")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    assert_eq!(error(&out)["code"], "INVALID_REMOTE");
    assert!(!env.home.path().join("push-args").exists());
}

#[test]
fn preflight_reports_the_plan_without_auth_or_push() {
    let server = Server::new();
    let env = setup(&server);
    let sha = committed_repo(&env);
    std::fs::remove_file(env.home.path().join("config.json")).unwrap();
    let out = floo(&env)
        .args(["--preflight", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["data"]["action"], "create-or-push");
    assert_eq!(value["data"]["app"], "client-portal");
    assert_eq!(value["data"]["commit"], sha);
    assert!(!env.home.path().join("push-args").exists());
}

fn mock_watch(server: &mut Server, sha: &str) -> Vec<mockito::Mock> {
    vec![
        server
            .mock("GET", "/v1/apps")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({"apps": [{
                    "id": APP_ID, "name": "client-portal", "org_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                    "status": "live", "url": "https://client-portal.example.test",
                    "runtime": "nodejs", "created_at": "2024-01-01T00:00:00Z"
                }]})
                .to_string(),
            )
            .create(),
        server
            .mock("GET", format!("/v1/apps/{APP_ID}/deploys").as_str())
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({"deploys": [{"id": "deploy-1", "status": "live", "commit_sha": sha}], "has_more": false})
                    .to_string(),
            )
            .create(),
    ]
}

#[test]
fn first_run_creates_the_project_adds_the_remote_force_pushes_and_watches() {
    let mut server = Server::new();
    let env = setup(&server);
    let sha = committed_repo(&env);
    let list = server
        .mock("GET", "/v1/projects")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"projects": []}).to_string())
        .create();
    let create = server
        .mock("POST", "/v1/projects")
        .match_body(Matcher::Json(json!({"name": "client-portal"})))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(project().to_string())
        .create();
    let _watch = mock_watch(&mut server, &sha);

    floo(&env)
        .arg("--json")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""status":"live""#));

    list.assert();
    create.assert();
    assert_eq!(
        git(env.repo.path(), &["remote", "get-url", "floo"]),
        project()["clone_url"]
    );
    let push = std::fs::read_to_string(env.home.path().join("push-args")).unwrap();
    let args: Vec<&str> = push.lines().collect();
    assert_eq!(args[2..4], ["-c", "credential.helper="]);
    assert!(args[5].ends_with(" projects git-credential"));
    assert_eq!(
        args[6..],
        [
            "-c",
            "credential.useHttpPath=true",
            "push",
            "--force",
            project()["clone_url"].as_str().unwrap(),
            "HEAD:refs/heads/main"
        ]
    );
    assert!(git_config(env.repo.path(), "floo.pendingFirstPush").is_none());
}

fn git_config(repo: &Path, key: &str) -> Option<String> {
    let output = std::process::Command::new(real_git())
        .arg("-C")
        .arg(repo)
        .args(["config", "--local", "--get", key])
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).unwrap().trim().to_string())
}

#[test]
fn a_retry_after_a_failed_first_push_still_replaces_the_starter() {
    let mut server = Server::new();
    let env = setup(&server);
    let sha = committed_repo(&env);
    let create = server
        .mock("POST", "/v1/projects")
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(project().to_string())
        .expect(1)
        .create();
    let _missing = server
        .mock("GET", "/v1/projects")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"projects": []}).to_string())
        .expect(1)
        .create();

    let out = floo(&env)
        .arg("--json")
        .env("PUSH_EXIT", "1")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    assert_eq!(error(&out)["code"], "GIT_PUSH_FAILED");
    assert_eq!(
        git_config(env.repo.path(), "floo.pendingFirstPush").as_deref(),
        project()["clone_url"].as_str()
    );

    // The rerun finds the remote it added and force-pushes again.
    let _found = server
        .mock("GET", "/v1/projects")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"projects": [project()]}).to_string())
        .create();
    let _watch = mock_watch(&mut server, &sha);
    floo(&env).arg("--json").assert().success();
    let push = std::fs::read_to_string(env.home.path().join("push-args")).unwrap();
    assert!(push.contains("push\n--force\n"), "{push}");
    assert!(git_config(env.repo.path(), "floo.pendingFirstPush").is_none());
    create.assert();
}

#[test]
fn pushes_to_the_project_url_even_when_the_remote_has_a_separate_push_url() {
    let mut server = Server::new();
    let env = setup(&server);
    let sha = committed_repo(&env);
    let clone_url = project()["clone_url"].as_str().unwrap().to_string();
    git(env.repo.path(), &["remote", "add", "floo", &clone_url]);
    git(
        env.repo.path(),
        &[
            "remote",
            "set-url",
            "--push",
            "floo",
            "https://github.com/owner/other.git",
        ],
    );
    let _list = server
        .mock("GET", "/v1/projects")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"projects": [project()]}).to_string())
        .create();
    let _watch = mock_watch(&mut server, &sha);
    floo(&env).arg("--json").assert().success();
    let push = std::fs::read_to_string(env.home.path().join("push-args")).unwrap();
    assert!(
        push.ends_with(&format!("push\n{clone_url}\nHEAD:refs/heads/main\n")),
        "{push}"
    );
}

#[test]
fn linked_repo_pushes_without_force_and_reports_a_rejected_push() {
    let mut server = Server::new();
    let env = setup(&server);
    let sha = committed_repo(&env);
    git(
        env.repo.path(),
        &[
            "remote",
            "add",
            "floo",
            project()["clone_url"].as_str().unwrap(),
        ],
    );
    let _list = server
        .mock("GET", "/v1/projects")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"projects": [project()]}).to_string())
        .expect_at_least(1)
        .create();
    let create = server.mock("POST", "/v1/projects").expect(0).create();
    let _watch = mock_watch(&mut server, &sha);

    floo(&env).arg("--json").assert().success();
    let push = std::fs::read_to_string(env.home.path().join("push-args")).unwrap();
    assert!(push.ends_with(&format!(
        "push\n{}\nHEAD:refs/heads/main\n",
        project()["clone_url"].as_str().unwrap()
    )));

    let out = floo(&env)
        .arg("--json")
        .env("PUSH_EXIT", "1")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let err = error(&out);
    assert_eq!(err["code"], "GIT_PUSH_FAILED");
    assert!(err["suggestion"]
        .as_str()
        .unwrap()
        .contains("pull --no-rebase floo main && floo deploy "));
    create.assert();
}
