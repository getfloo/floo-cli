//! `floo connect owner/repo`: move a hosted app to the customer's GitHub repo, or
//! connect or create an app on it. git runs for real against local bare repos
//! that stand in for github.com, so the history that moves is checked.

#![cfg(unix)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use mockito::{Matcher, Mock, Server};
use serde_json::{json, Value};
use tempfile::TempDir;

const APP_ID: &str = "01234567-89ab-cdef-0123-456789abcdef";
const HOSTED: &str =
    "floo-managed/floo-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-0123456789abcdef0123456789abcdef";
const TARGET: &str = "acme/storefront";

struct Env {
    home: TempDir,
    folder: TempDir,
    github: TempDir,
}

impl Env {
    fn bare(&self, repo: &str) -> PathBuf {
        self.github.path().join(format!("{repo}.git"))
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
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

/// A hosted repo with `main`, a `feature` branch and a tag, an empty target repo,
/// and a folder whose `origin` is the hosted repo.
fn setup(server: &Server) -> Env {
    let env = Env {
        home: TempDir::new().unwrap(),
        folder: TempDir::new().unwrap(),
        github: TempDir::new().unwrap(),
    };
    std::fs::write(
        env.home.path().join("config.json"),
        json!({
            "api_key": "floo_test123",
            "api_url": server.url(),
            "default_org": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        })
        .to_string(),
    )
    .unwrap();
    for repo in [HOSTED, TARGET] {
        let bare = env.bare(repo);
        std::fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    }
    let folder = env.folder.path();
    git(folder, &["init", "-q", "-b", "main"]);
    std::fs::write(
        folder.join("floo.app.toml"),
        "[app]\nname = \"storefront\"\n",
    )
    .unwrap();
    git(folder, &["add", "-A"]);
    git(folder, &["commit", "-q", "-m", "init"]);
    git(folder, &["tag", "v1"]);
    git(folder, &["branch", "feature"]);
    let hosted_path = env.bare(HOSTED);
    let hosted_path = hosted_path.to_str().unwrap();
    git(
        folder,
        &["push", "-q", hosted_path, "main", "feature", "v1"],
    );
    let hosted_url = format!("https://github.com/{HOSTED}.git");
    git(folder, &["remote", "add", "origin", &hosted_url]);
    env
}

#[allow(deprecated)]
fn floo(env: &Env, args: &[&str]) -> Command {
    let mut command = Command::cargo_bin("floo-local").unwrap();
    let github = format!("file://{}/", env.github.path().display());
    command
        .current_dir(env.folder.path())
        .env("HOME", env.home.path())
        .env("FLOO_CONFIG_DIR", env.home.path())
        .env_remove("FLOO_API_URL")
        .env("FLOO_NO_UPDATE_CHECK", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", env.home.path().join(".gitconfig"))
        // github.com is the local bare repos.
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", format!("url.{github}.insteadOf"))
        .env("GIT_CONFIG_VALUE_0", "https://github.com/")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .arg("connect")
        .args(args)
        .arg("--json");
    command
}

fn app_with(source: Value) -> Value {
    json!({
        "id": APP_ID, "name": "storefront", "org_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "status": "live", "created_at": "2024-01-01T00:00:00Z", "source": source,
    })
}

fn mock_apps(server: &mut Server, apps: Value) -> Mock {
    server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"apps": apps, "total": 1}).to_string())
        .create()
}

fn mock_access(server: &mut Server, access: Value) -> Mock {
    server
        .mock("GET", "/v1/github/check-repo-access")
        .match_query(Matcher::UrlEncoded("repo".into(), TARGET.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(access.to_string())
        .create()
}

fn mock_move(server: &mut Server) -> Mock {
    server
        .mock("PUT", &*format!("/v1/apps/{APP_ID}/source"))
        .match_body(Matcher::Json(
            json!({"kind": "github", "repo": TARGET, "branch": "main"}),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "app": "storefront",
                "source": {"kind": "github", "repo": TARGET, "branch": "main"},
                "previous": {"kind": "floo", "repo": HOSTED, "branch": "main"},
                "archived_repo": HOSTED,
                "message": "storefront moved from the repo floo hosted to acme/storefront@main.",
            })
            .to_string(),
        )
        .create()
}

fn hosted_app(server: &mut Server) -> Mock {
    mock_apps(
        server,
        json!([app_with(
            json!({"kind": "floo", "repo": HOSTED, "branch": "main"})
        )]),
    )
}

fn refs(bare: &Path) -> String {
    git(bare, &["for-each-ref", "--format=%(refname)"])
}

#[test]
fn a_hosted_app_moves_with_every_branch_and_tag_and_origin_follows() {
    let mut server = Server::new();
    let env = setup(&server);
    let _apps = hosted_app(&mut server);
    let _access = mock_access(&mut server, json!({"accessible": true}));
    let moved = mock_move(&mut server);
    let ssh = format!("git@github.com:{HOSTED}.git");
    git(
        env.folder.path(),
        &["remote", "set-url", "--push", "origin", &ssh],
    );

    let output = floo(&env, &[TARGET])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    moved.assert();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(reply["data"]["archived_repo"], HOSTED);
    assert_eq!(reply["data"]["remotes_repointed"], json!(["origin"]));
    assert_eq!(
        refs(&env.bare(TARGET)),
        "refs/heads/feature\nrefs/heads/main\nrefs/tags/v1"
    );
    assert_eq!(
        git(env.folder.path(), &["remote", "get-url", "origin"]),
        format!("https://github.com/{TARGET}.git")
    );
    assert_eq!(
        git(env.folder.path(), &["config", "remote.origin.pushurl"]),
        format!("git@github.com:{TARGET}.git")
    );
}

#[test]
fn the_hosted_remote_is_removed_when_another_already_fetches_from_the_repo() {
    let mut server = Server::new();
    let env = setup(&server);
    let target_url = format!("https://github.com/{TARGET}.git");
    git(env.folder.path(), &["remote", "add", "github", &target_url]);
    let _apps = hosted_app(&mut server);
    let _access = mock_access(&mut server, json!({"accessible": true}));
    let _moved = mock_move(&mut server);

    let output = floo(&env, &[TARGET])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(reply["data"]["remotes_removed"], json!(["origin"]));
    assert_eq!(git(env.folder.path(), &["remote"]), "github");
}

#[test]
fn a_rerun_after_part_of_the_history_landed_pushes_the_rest() {
    let mut server = Server::new();
    let env = setup(&server);
    let target = env.bare(TARGET);
    git(
        env.folder.path(),
        &["push", "-q", target.to_str().unwrap(), "main"],
    );
    let _apps = hosted_app(&mut server);
    let _access = mock_access(&mut server, json!({"accessible": true}));
    let moved = mock_move(&mut server);

    floo(&env, &[TARGET]).assert().success();

    moved.assert();
    assert_eq!(
        refs(&target),
        "refs/heads/feature\nrefs/heads/main\nrefs/tags/v1"
    );
}

#[test]
fn a_rerun_after_the_switch_landed_still_repoints_the_folder() {
    let mut server = Server::new();
    let env = setup(&server);
    let _apps = mock_apps(
        &mut server,
        json!([app_with(
            json!({"kind": "github", "repo": TARGET, "branch": "main"})
        )]),
    );
    let _access = mock_access(&mut server, json!({"accessible": true}));
    let unchanged = server
        .mock("PUT", &*format!("/v1/apps/{APP_ID}/source"))
        .match_body(Matcher::Json(json!({"kind": "github", "repo": TARGET})))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "app": "storefront",
                "source": {"kind": "github", "repo": TARGET, "branch": "main"},
                "previous": {"kind": "github", "repo": TARGET, "branch": "main"},
                "archived_repo": null,
                "message": "storefront already deploys dev from acme/storefront@main.",
            })
            .to_string(),
        )
        .create();

    let output = floo(&env, &[TARGET])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    unchanged.assert();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(reply["data"]["remotes_repointed"], json!(["origin"]));
    assert_eq!(
        git(env.folder.path(), &["remote", "get-url", "origin"]),
        format!("https://github.com/{TARGET}.git")
    );
}

#[test]
fn a_broken_manifest_stops_before_any_app_is_chosen() {
    let mut server = Server::new();
    let env = setup(&server);
    std::fs::write(env.folder.path().join("floo.app.toml"), "[app\nname = ").unwrap();
    let apps = mock_apps(&mut server, json!([])).expect(0);

    let output = floo(&env, &[TARGET])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    apps.assert();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert_ne!(reply["error"]["code"], "NO_CONFIG_FOUND");
    assert!(reply["error"]["code"].is_string());
}

#[test]
fn a_repo_with_other_commits_is_refused_and_nothing_moves() {
    let mut server = Server::new();
    let env = setup(&server);
    let other = TempDir::new().unwrap();
    git(other.path(), &["init", "-q", "-b", "main"]);
    git(
        other.path(),
        &["commit", "-q", "--allow-empty", "-m", "theirs"],
    );
    let target = env.bare(TARGET);
    git(
        other.path(),
        &["push", "-q", target.to_str().unwrap(), "main"],
    );
    let _apps = hosted_app(&mut server);
    let _access = mock_access(&mut server, json!({"accessible": true}));
    let moved = mock_move(&mut server).expect(0);

    let output = floo(&env, &[TARGET])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    moved.assert();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(reply["error"]["code"], "REPO_NOT_EMPTY");
    assert_eq!(refs(&target), "refs/heads/main");
    assert_eq!(
        git(env.folder.path(), &["remote", "get-url", "origin"]),
        format!("https://github.com/{HOSTED}.git")
    );
}

#[test]
fn a_repo_floo_cannot_read_stops_before_anything_is_pushed() {
    let mut server = Server::new();
    let env = setup(&server);
    let _apps = hosted_app(&mut server);
    let _access = mock_access(
        &mut server,
        json!({"accessible": false, "reason": "APP_NOT_INSTALLED"}),
    );
    let link = "https://api.getfloo.com/v1/github/setup/install?setup_token=t";
    let _setup = server
        .mock("POST", "/v1/github/setup/begin?owner=acme")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "install_url": link }).to_string())
        .create();
    let moved = mock_move(&mut server).expect(0);

    let output = floo(&env, &[TARGET])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    moved.assert();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(reply["error"]["code"], "GITHUB_REPO_NOT_ACCESSIBLE");
    assert!(reply["error"]["suggestion"]
        .as_str()
        .unwrap()
        .starts_with(&format!(
            "Open {link}, install the App on acme, then floo connect acme/storefront"
        )));
    assert_eq!(refs(&env.bare(TARGET)), "");
}

#[test]
fn an_app_that_does_not_exist_yet_is_created_on_the_repo() {
    let mut server = Server::new();
    let env = setup(&server);
    let _apps = mock_apps(&mut server, json!([]));
    let _access = mock_access(&mut server, json!({"accessible": true}));
    let created = server
        .mock("POST", "/v1/apps")
        .match_body(Matcher::Json(
            json!({"name": "storefront", "source": {"kind": "github", "repo": TARGET}}),
        ))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            app_with(json!({"kind": "github", "repo": TARGET, "branch": "main"})).to_string(),
        )
        .create();

    let output = floo(&env, &[TARGET])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    created.assert();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(reply["data"]["source"]["repo"], TARGET);
}
