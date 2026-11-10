//! `floo deploy`: local readiness checks, the API plan, create or connect, push and watch.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use mockito::{Matcher, Server};
use predicates::prelude::PredicateBooleanExt;
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
            "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = push ]; then\n    printf '%s\\n' \"$@\" > \"$PUSH_ARGS\"\n    if [ -n \"$PUSH_403_ONCE\" ] && [ ! -e \"$PUSH_403_ONCE\" ]; then\n      : > \"$PUSH_403_ONCE\"\n      echo 'fatal: unable to access: The requested URL returned error: 403' >&2\n      exit 128\n    fi\n    exit \"${{PUSH_EXIT:-0}}\"\n  fi\n  if [ \"$arg\" = ls-remote ]; then\n    printf '%s' \"$LS_REMOTE\"\n    exit 0\n  fi\ndone\nexec {} \"$@\"\n",
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

/// `--json` stdout parsed as exactly one document; a second line fails the parse.
fn json_document(output: &[u8]) -> Value {
    serde_json::from_slice(output).unwrap()
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
    assert_eq!(value["data"]["valid"], true);
    let plan = &value["data"]["deploy_plan"];
    assert_eq!(plan["code"], "NOT_AUTHENTICATED");
    assert_eq!(plan["commit"], sha);
    assert_eq!(
        plan["remotes"],
        json!([{"name": "origin", "url": "https://github.com/acme/client-portal.git"}])
    );
    assert!(pushed(&env).is_none());
}

/// `floo deploy --preflight` is the one check before a deploy: the config report,
/// then the plan, with nothing created or pushed. An invalid config never plans.
#[test]
fn preflight_checks_the_config_then_shows_the_plan_and_changes_nothing() {
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
    let source = json!({"kind": "github", "repo": "acme/client-portal", "branch": "main"});
    let planned = server
        .mock("POST", "/v1/deploy/plan")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(json!({
            "ok": true, "decision": "connect_github", "source": source,
            "message": "origin is acme/client-portal and floo can read it.",
        })))
        .expect(2)
        .create();
    let lookup = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"apps":[]}"#)
        .expect(2)
        .create();
    let create = server.mock("POST", "/v1/apps").expect(0).create();

    let out = floo(&env)
        .args(["--preflight", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["data"]["valid"], true);
    assert_eq!(value["data"]["app"], "client-portal");
    assert_eq!(value["data"]["services"][0]["name"], "web");
    assert_eq!(value["data"]["plan_skipped_reason"], "APP_NOT_CREATED");
    assert_eq!(value["data"]["deploy_plan"]["ok"], true);
    assert_eq!(value["data"]["deploy_plan"]["decision"], "connect_github");

    floo(&env)
        .arg("--preflight")
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "Deploy plan: origin is acme/client-portal and floo can read it.",
        ));
    planned.assert();

    std::fs::write(
        env.repo.path().join("floo.app.toml"),
        "[app]\nname = \"client-portal\"\n\n[services.web]\ntype = \"web\"\npath = \"missing\"\nport = 3000\ningress = \"public\"\n",
    )
    .unwrap();
    let out = floo(&env)
        .args(["--preflight", "--json"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["data"]["valid"], false);
    assert!(value["data"].get("deploy_plan").is_none());
    planned.assert();
    lookup.assert();
    create.assert();
    assert!(pushed(&env).is_none());
}

/// The checks run before git is set up, and the plan names the git step that's missing.
#[test]
fn preflight_runs_the_config_checks_before_the_folder_is_a_repository() {
    let mut server = Server::new();
    let env = setup(&server);
    let _lookup = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"apps":[]}"#)
        .create();
    let planned = server.mock("POST", "/v1/deploy/plan").expect(0).create();
    write_config(env.repo.path());
    let out = floo(&env)
        .args(["--preflight", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["data"]["valid"], true);
    let plan = &value["data"]["deploy_plan"];
    assert_eq!(plan["ok"], false);
    assert_eq!(plan["code"], "NOT_A_GIT_REPOSITORY");
    assert!(plan["next"][0]["cli"]
        .as_str()
        .unwrap()
        .contains(" init && git -C "));

    // --env only means something with --preflight.
    floo(&env).args(["--env", "prod"]).assert().code(2);
    floo(&env)
        .args(["--preflight", "--env", "prod", "--json"])
        .assert()
        .success();
    planned.assert();
}

/// A User accounts app is public by design: preflight explains User accounts
/// instead of suggesting team access (feedback ae6aec6c).
#[test]
fn preflight_explains_user_accounts_instead_of_suggesting_team_access() {
    let mut server = Server::new();
    let env = setup(&server);
    let _lookup = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"apps":[]}"#)
        .create();
    std::fs::write(
        env.repo.path().join("floo.app.toml"),
        "[app]\nname = \"client-portal\"\n\n\
         [services.web]\ntype = \"web\"\npath = \".\"\nport = 3000\ningress = \"public\"\n\n\
         [managed.default]\ntype = \"postgres\"\n\n[managed.default.users]\n\
         allowed_email_domains = [\"duke.edu\"]\n",
    )
    .unwrap();
    let out = floo(&env)
        .args(["--preflight", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&out).unwrap();
    let codes: Vec<&str> = value["data"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|finding| finding["code"].as_str())
        .collect();
    assert!(codes.contains(&"USER_ACCOUNTS_ON"), "{codes:?}");
    assert!(!codes.contains(&"ACCESS_MODE_PUBLIC"), "{codes:?}");

    // Team sign-in in front of user accounts locks the app's own users out.
    let config = std::fs::read_to_string(env.repo.path().join("floo.app.toml")).unwrap();
    std::fs::write(
        env.repo.path().join("floo.app.toml"),
        config.replacen("[app]\n", "[app]\naccess_mode = \"accounts\"\n", 1),
    )
    .unwrap();
    let out = floo(&env)
        .args(["--preflight", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&out).unwrap();
    let findings = value["data"]["findings"].as_array().unwrap();
    let blocked = findings
        .iter()
        .find(|finding| finding["code"] == "USER_ACCOUNTS_BEHIND_TEAM_SIGN_IN")
        .expect("warning");
    assert_eq!(blocked["severity"], "warning");
    assert!(blocked["message"]
        .as_str()
        .unwrap()
        .contains("`[app] access_mode = \"public\"`"));
    assert!(!findings.iter().any(|f| f["code"] == "USER_ACCOUNTS_ON"));

    // A dev override wins over [app], so the fix names the override.
    std::fs::write(
        env.repo.path().join("floo.app.toml"),
        format!("{config}\n[environments.dev]\naccess_mode = \"accounts\"\n"),
    )
    .unwrap();
    let out = floo(&env)
        .args(["--preflight", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&out).unwrap();
    let blocked = value["data"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == "USER_ACCOUNTS_BEHIND_TEAM_SIGN_IN")
        .expect("warning")
        .clone();
    assert!(blocked["message"]
        .as_str()
        .unwrap()
        .contains("`[environments.dev] access_mode = \"public\"`"));
}

/// A new hosted app starts behind team sign-in, so preflight must not call it public.
#[test]
fn preflight_says_when_only_the_org_can_open_the_app() {
    let mut server = Server::new();
    let env = setup(&server);
    committed_repo(&env);
    let config = std::fs::read_to_string(env.repo.path().join("floo.app.toml")).unwrap();
    std::fs::write(
        env.repo.path().join("floo.app.toml"),
        config.replace("access_mode = \"accounts\"\n", ""),
    )
    .unwrap();
    git(
        env.repo.path(),
        &["commit", "-qam", "floo picks the access mode"],
    );
    let planned = server
        .mock("POST", "/v1/deploy/plan")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(json!({
            "ok": true, "decision": "create_hosted", "source": {"kind": "floo"},
            "access_mode": "accounts",
        })))
        .expect(2)
        .create();
    let _lookup = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"apps":[]}"#)
        .create();

    let out = floo(&env)
        .args(["--preflight", "--hosted", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&out).unwrap();
    let codes: Vec<&str> = value["data"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|finding| finding["code"].as_str())
        .collect();
    assert!(codes.contains(&"ACCESS_MODE_UNSET"), "{codes:?}");
    assert!(!codes.contains(&"ACCESS_MODE_PUBLIC"), "{codes:?}");
    assert_eq!(value["data"]["deploy_plan"]["access_mode"], "accounts");

    floo(&env)
        .args(["--preflight", "--hosted"])
        .assert()
        .success()
        .stderr(
            predicates::str::contains("Only signed-in members of acme can open it")
                .and(predicates::str::contains("For a public site")),
        );
    planned.assert();
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
    let doc = json_document(&out);
    assert_eq!(doc["data"]["plan"]["decision"], "connect_github");
    assert_eq!(doc["data"]["deploy"]["status"], "live");
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
fn a_push_deploy_does_not_need_the_local_env_file() {
    let mut server = Server::new();
    let env = setup(&server);
    git(env.repo.path(), &["init", "-q", "-b", "main"]);
    write_config(env.repo.path());
    let config = env.repo.path().join("floo.app.toml");
    let declared = std::fs::read_to_string(&config).unwrap() + "env_file = \".env\"\n";
    std::fs::write(&config, declared).unwrap();
    git(env.repo.path(), &["add", "-A"]);
    git(env.repo.path(), &["commit", "-q", "-m", "init"]);
    let sha = git(env.repo.path(), &["rev-parse", "HEAD"]);
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
    let planned = server
        .mock("POST", "/v1/deploy/plan")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(
            json!({"ok": true, "decision": "push", "source": source}),
        ))
        .create();
    let _watch = mock_watch(&mut server, "client-portal", &sha);

    floo(&env)
        .arg("--json")
        .env("LS_REMOTE", format!("{sha}\trefs/heads/main\n"))
        .assert()
        .success();
    planned.assert();
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
            "source": {"kind": "github", "repo": "acme/client-portal", "branch": "main"},
            "next": [{"open": url, "cli": "floo apps github setup"}, {"cli": "floo deploy --hosted"}],
        })))
        .create();
    // A bare install page wouldn't link the installation to floo; setup's link does,
    // and it is begun for the repo's owner so setup never stops to ask which account.
    let link = "https://api.getfloo.com/v1/github/setup/install?setup_token=t";
    let setup = server
        .mock("POST", "/v1/github/setup/begin?owner=acme")
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
        .with_body(plan(json!({
            "ok": true, "decision": "create_hosted", "source": {"kind": "floo"},
            "access_mode": "accounts",
        })))
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

    let out = floo(&env).args(["--hosted", "--json"]).assert().success();
    let doc = json_document(&out.get_output().stdout);
    assert_eq!(doc["data"]["plan"]["access_mode"], "accounts");
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
fn a_new_hosted_repo_pushes_again_while_github_still_refuses_with_403() {
    let mut server = Server::new();
    let env = setup(&server);
    let sha = committed_repo(&env);
    let source = json!({"kind": "floo", "repo": REPO, "branch": "main"});
    let _plan = server
        .mock("POST", "/v1/deploy/plan")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(
            json!({"ok": true, "decision": "create_hosted", "source": {"kind": "floo"}}),
        ))
        .create();
    let _create = server
        .mock("POST", "/v1/apps")
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(app("client-portal", source).to_string())
        .create();
    let _watch = mock_watch(&mut server, "client-portal", &sha);
    let refused = env.home.path().join("refused-once");

    floo(&env)
        .args(["--hosted", "--json"])
        .env("PUSH_403_ONCE", &refused)
        .assert()
        .success();
    // GitHub refused the first push, and the second one went through.
    assert!(refused.exists());
    assert_eq!(
        pushed(&env).unwrap().last().unwrap(),
        "HEAD:refs/heads/main"
    );
}

#[test]
fn an_existing_hosted_repo_does_not_retry_a_refused_push() {
    let mut server = Server::new();
    let env = setup(&server);
    committed_repo(&env);
    let _plan = server
        .mock("POST", "/v1/deploy/plan")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(plan(json!({
            "ok": true,
            "decision": "push",
            "source": {"kind": "floo", "repo": REPO, "branch": "main"},
        })))
        .create();
    let refused = env.home.path().join("refused-once");

    let out = floo(&env)
        .arg("--json")
        .env("PUSH_403_ONCE", &refused)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_document(&out)["error"]["code"], "GIT_PUSH_FAILED");
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
    json_document(&out)["error"].clone()
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
