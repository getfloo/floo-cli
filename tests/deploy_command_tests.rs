//! `floo deploy`: local readiness checks, the API plan, create or connect, push and watch.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use mockito::{Matcher, Server};
use serde_json::{json, Value};
use tempfile::TempDir;

const APP_ID: &str = "01234567-89ab-cdef-0123-456789abcdef";
const REPO: &str =
    "floo-managed/floo-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-0123456789abcdef0123456789abcdef";

fn app(name: &str, source: Value) -> Value {
    json!({
        "id": APP_ID, "name": name, "org_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "status": "live", "url": format!("https://{name}.example.test"),
        "created_at": "2024-01-01T00:00:00Z", "source": source,
    })
}

fn plan(fields: Value) -> String {
    let mut plan = json!({"app": "client-portal", "org": "acme", "message": "planned"});
    plan.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    plan.to_string()
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

/// A git that runs the real git for everything except `push`, which it logs, and
/// `ls-remote`, which answers `$LS_REMOTE` so no test reaches github.com.
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
            "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = push ]; then\n    printf '%s\\n' \"$@\" > \"$PUSH_ARGS\"\n    exit \"${{PUSH_EXIT:-0}}\"\n  fi\n  if [ \"$arg\" = ls-remote ]; then\n    printf '%s' \"$LS_REMOTE\"\n    exit 0\n  fi\ndone\nexec {} \"$@\"\n",
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
        "[app]\nname = \"client-portal\"\naccess_mode = \"accounts\"\n\n\
         [services.web]\ntype = \"web\"\npath = \".\"\nport = 3000\ningress = \"public\"\n",
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

fn json_lines(output: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(output)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn fails(env: &Env, args: &[&str]) -> Value {
    let out = floo(env)
        .args(args)
        .arg("--json")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    error(&out)
}

fn pushed(env: &Env) -> Option<Vec<String>> {
    std::fs::read_to_string(env.home.path().join("push-args"))
        .ok()
        .map(|args| args.lines().map(str::to_string).collect())
}

#[test]
fn stops_with_the_next_command_when_the_folder_is_not_ready() {
    let server = Server::new();
    let env = setup(&server);

    let err = fails(&env, &[]);
    assert_eq!(err["code"], "NO_CONFIG_FOUND");
    assert!(err["suggestion"]
        .as_str()
        .unwrap()
        .starts_with("floo init <app-name> --path "));

    write_config(env.repo.path());
    let err = fails(&env, &[]);
    assert_eq!(err["code"], "NOT_A_GIT_REPOSITORY");
    assert!(err["suggestion"]
        .as_str()
        .unwrap()
        .contains(" init && git -C "));

    git(env.repo.path(), &["init", "-q", "-b", "main"]);
    assert_eq!(fails(&env, &[])["code"], "NO_COMMITS");

    git(env.repo.path(), &["add", "-A"]);
    git(env.repo.path(), &["commit", "-q", "-m", "init"]);
    std::fs::write(env.repo.path().join("app.js"), "x").unwrap();
    let err = fails(&env, &[]);
    assert_eq!(err["code"], "UNCOMMITTED_CHANGES");
    assert!(err["suggestion"]
        .as_str()
        .unwrap()
        .contains("add -A && git -C "));

    // A secret-looking file is named, and the fix never says `git add -A`.
    std::fs::create_dir(env.repo.path().join("api")).unwrap();
    std::fs::write(env.repo.path().join("api/.env.local"), "TOKEN=x").unwrap();
    let err = fails(&env, &[]);
    assert_eq!(err["code"], "SECRET_FILES_NOT_IGNORED");
    assert!(err["message"].as_str().unwrap().contains("api/.env.local"));
    let fix = err["suggestion"].as_str().unwrap();
    assert!(
        fix.contains(".gitignore") && !fix.contains("add -A"),
        "{fix}"
    );

    let out = floo(&env)
        .args(["--github", "--hosted", "--json"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    assert_eq!(error(&out)["code"], "INVALID_ARGUMENTS");
    assert!(pushed(&env).is_none());
}

#[test]
fn preflight_without_sign_in_reports_the_folder_with_credentials_stripped() {
    let server = Server::new();
    let env = setup(&server);
    let sha = committed_repo(&env);
    git(
        env.repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://x:ghp_secret@github.com/acme/client-portal.git",
        ],
    );
    std::fs::remove_file(env.home.path().join("config.json")).unwrap();
    let out = floo(&env)
        .args(["--preflight", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!String::from_utf8_lossy(&out).contains("ghp_secret"));
    let value: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["data"]["action"], "plan");
    assert_eq!(value["data"]["commit"], sha);
    assert_eq!(
        value["data"]["remotes"],
        json!([{"name": "origin", "url": "https://github.com/acme/client-portal.git"}])
    );
    assert!(pushed(&env).is_none());
}

fn mock_watch(server: &mut Server, name: &str, sha: &str) -> Vec<mockito::Mock> {
    vec![
        server
            .mock("GET", "/v1/apps")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({"apps": [app(name, Value::Null)]}).to_string())
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
fn a_github_folder_creates_the_app_on_its_repo_pushes_and_watches() {
    let mut server = Server::new();
    let env = setup(&server);
    let sha = committed_repo(&env);
    git(
        env.repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://x:ghp_secret@github.com/acme/client-portal.git",
        ],
    );
    let source = json!({"kind": "github", "repo": "acme/client-portal", "branch": "main"});
    let planned = server
        .mock("POST", "/v1/deploy/plan")
        .match_body(Matcher::Json(json!({
            "app": "client-portal",
            "remotes": {"origin": "https://github.com/acme/client-portal.git"},
            "remote": null, "branch": "main", "head": sha, "source": null,
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(
            json!({"ok": true, "decision": "connect_github", "source": source}),
        ))
        .create();
    let created = server
        .mock("POST", "/v1/apps")
        .match_body(Matcher::Json(
            json!({"name": "client-portal", "source": source}),
        ))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(app("client-portal", source.clone()).to_string())
        .create();
    let _watch = mock_watch(&mut server, "client-portal", &sha);

    let out = floo(&env)
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    planned.assert();
    created.assert();
    let events = json_lines(&out);
    assert_eq!(events[0]["event"], "plan");
    assert_eq!(events[0]["decision"], "connect_github");
    assert_eq!(events.last().unwrap()["status"], "live");
    let args = pushed(&env).unwrap();
    assert_eq!(args[2..], ["push", "origin", "HEAD:refs/heads/main"]);
}

#[test]
fn a_commit_already_on_github_is_not_pushed_again() {
    let mut server = Server::new();
    let env = setup(&server);
    let sha = committed_repo(&env);
    git(
        env.repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:acme/client-portal.git",
        ],
    );
    let source = json!({"kind": "github", "repo": "acme/client-portal", "branch": "main"});
    let _plan = server
        .mock("POST", "/v1/deploy/plan")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(
            json!({"ok": true, "decision": "push", "source": source}),
        ))
        .create();
    let create = server.mock("POST", "/v1/apps").expect(0).create();
    let _watch = mock_watch(&mut server, "client-portal", &sha);

    floo(&env)
        .arg("--json")
        .env("LS_REMOTE", format!("{sha}\trefs/heads/main\n"))
        .assert()
        .success();
    assert!(pushed(&env).is_none());
    create.assert();
}

#[test]
fn a_stop_carries_the_plan_and_creates_nothing() {
    let mut server = Server::new();
    let env = setup(&server);
    committed_repo(&env);
    git(
        env.repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/client-portal",
        ],
    );
    let url = "https://github.com/apps/getfloo/installations/new";
    let _plan = server
        .mock("POST", "/v1/deploy/plan")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(json!({
            "ok": false, "code": "GITHUB_APP_NOT_INSTALLED", "decision": "connect_github",
            "message": "floo can't read acme/client-portal yet.",
            "next": [{"open": url, "cli": "floo apps github setup"}, {"cli": "floo deploy --hosted"}],
        })))
        .create();
    // A bare install page wouldn't link the installation to floo; setup's link does.
    let link = "https://api.getfloo.com/v1/github/setup/install?setup_token=t";
    let setup = server
        .mock("POST", "/v1/github/setup/begin")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "install_url": link }).to_string())
        .create();
    let create = server.mock("POST", "/v1/apps").expect(0).create();

    let err = fails(&env, &[]);
    assert_eq!(err["code"], "GITHUB_APP_NOT_INSTALLED");
    assert!(err["suggestion"]
        .as_str()
        .unwrap()
        .starts_with(&format!("Open {link}, then floo deploy ")));
    assert_eq!(err["next"][0]["open"], link);
    assert!(err["next"][0]["cli"]
        .as_str()
        .unwrap()
        .starts_with("floo deploy "));
    assert_ne!(url, link);
    assert_eq!(err["next"][1]["cli"], "floo deploy --hosted");
    setup.assert();
    assert_eq!(err["decision"], "connect_github");
    create.assert();
    assert!(pushed(&env).is_none());
}

#[test]
fn a_taken_name_suggests_a_free_one_and_leaves_the_config_alone() {
    let mut server = Server::new();
    let env = setup(&server);
    committed_repo(&env);
    let _plan = server
        .mock("POST", "/v1/deploy/plan")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(json!({
            "ok": false, "code": "APP_NAME_TAKEN", "message": "taken",
            "suggested_names": ["acme-client-portal"],
        })))
        .create();

    let err = fails(&env, &["--name", "shop"]);
    assert_eq!(err["code"], "APP_NAME_TAKEN");
    assert!(err["suggestion"]
        .as_str()
        .unwrap()
        .ends_with(" --name acme-client-portal"));
    let config = std::fs::read_to_string(env.repo.path().join("floo.app.toml")).unwrap();
    assert!(config.contains("name = \"client-portal\""));
}

#[test]
fn hosted_creates_an_empty_repo_adds_origin_and_pushes_without_force() {
    let mut server = Server::new();
    let env = setup(&server);
    let sha = committed_repo(&env);
    let source = json!({"kind": "floo", "repo": REPO, "branch": "main"});
    let planned = server
        .mock("POST", "/v1/deploy/plan")
        .match_body(Matcher::PartialJson(
            json!({"source": "floo", "remotes": {}}),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(
            json!({"ok": true, "decision": "create_hosted", "source": {"kind": "floo"}}),
        ))
        .create();
    let created = server
        .mock("POST", "/v1/apps")
        .match_body(Matcher::Json(
            json!({"name": "client-portal", "source": {"kind": "floo", "branch": "main"}}),
        ))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(app("client-portal", source).to_string())
        .create();
    let _watch = mock_watch(&mut server, "client-portal", &sha);

    floo(&env).args(["--hosted", "--json"]).assert().success();
    planned.assert();
    created.assert();
    let clone_url = format!("https://github.com/{REPO}.git");
    assert_eq!(
        git(env.repo.path(), &["remote", "get-url", "origin"]),
        clone_url
    );
    let args = pushed(&env).unwrap();
    assert_eq!(args[2..4], ["-c", "credential.helper="]);
    assert!(args[5].ends_with(" git-credential") && !args[5].contains("projects"));
    assert_eq!(
        args[6..],
        [
            "-c",
            "credential.useHttpPath=true",
            "push",
            clone_url.as_str(),
            "HEAD:refs/heads/main"
        ]
    );
}

#[test]
fn an_accepted_name_is_written_and_committed_before_the_push() {
    let mut server = Server::new();
    let env = setup(&server);
    committed_repo(&env);
    // A root service manifest names the app too, and must keep matching.
    std::fs::write(
        env.repo.path().join("floo.service.toml"),
        "[app]\nname = \"client-portal\"\n\n[service]\nname = \"web\"\ntype = \"web\"\nport = 3000\ningress = \"public\"\n",
    )
    .unwrap();
    git(env.repo.path(), &["add", "-A"]);
    git(env.repo.path(), &["commit", "-q", "-m", "service"]);
    let _plan = server
        .mock("POST", "/v1/deploy/plan")
        .match_body(Matcher::PartialJson(json!({"app": "shop-2"})))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(
            json!({"ok": true, "decision": "create_hosted", "source": {"kind": "floo"}}),
        ))
        .create();
    let _create = server
        .mock("POST", "/v1/apps")
        .match_body(Matcher::PartialJson(json!({"name": "shop-2"})))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            app(
                "shop-2",
                json!({"kind": "floo", "repo": REPO, "branch": "main"}),
            )
            .to_string(),
        )
        .create();

    let err = fails_with_push_error(&env, &["--hosted", "--name", "shop-2"]);
    assert_eq!(err["code"], "GIT_PUSH_FAILED");
    let config = std::fs::read_to_string(env.repo.path().join("floo.app.toml")).unwrap();
    assert!(config.contains("name = \"shop-2\""), "{config}");
    let service = std::fs::read_to_string(env.repo.path().join("floo.service.toml")).unwrap();
    assert!(service.contains("[app]\nname = \"shop-2\""), "{service}");
    assert!(service.contains("name = \"web\""), "{service}");
    assert_eq!(
        git(env.repo.path(), &["log", "-1", "--format=%s"]),
        "Name the floo app shop-2"
    );
    assert_eq!(git(env.repo.path(), &["status", "--porcelain"]), "");
}

fn fails_with_push_error(env: &Env, args: &[&str]) -> Value {
    let out = floo(env)
        .args(args)
        .arg("--json")
        .env("PUSH_EXIT", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.test")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.test")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let lines = json_lines(&out);
    lines.last().unwrap()["error"].clone()
}

#[test]
fn another_branch_gets_a_preview_and_leaves_dev_alone() {
    let mut server = Server::new();
    let env = setup(&server);
    committed_repo(&env);
    git(env.repo.path(), &["switch", "-q", "-c", "feat/cart"]);
    let sha = git(env.repo.path(), &["rev-parse", "HEAD"]);
    git(
        env.repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/client-portal.git",
        ],
    );
    let source = json!({"kind": "github", "repo": "acme/client-portal", "branch": "main"});
    let _plan = server
        .mock("POST", "/v1/deploy/plan")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(
            json!({"ok": true, "decision": "push", "source": source}),
        ))
        .create();
    let _apps = mock_watch(&mut server, "client-portal", &sha);
    let preview = server
        .mock("POST", format!("/v1/apps/{APP_ID}/deploys").as_str())
        .match_body(Matcher::PartialJson(
            json!({"env": "preview", "branch": "feat/cart", "commit_sha": sha}),
        ))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            json!({"id": "deploy-p", "status": "live", "url": "https://p.example.test"})
                .to_string(),
        )
        .create();

    floo(&env).arg("--json").assert().success();
    preview.assert();
    let args = pushed(&env).unwrap();
    assert_eq!(args[2..], ["push", "origin", "HEAD:refs/heads/feat/cart"]);
}
