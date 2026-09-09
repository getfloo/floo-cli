use assert_cmd::Command;
use mockito::{Matcher, Server};
use predicates::prelude::*;
use serde_json::{json, Value};
use tempfile::TempDir;

const APP_ID: &str = "01234567-89ab-cdef-0123-456789abcdef";
const REPO: &str =
    "floo-managed/floo-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-0123456789abcdef0123456789abcdef";
const TOKEN: &str = "ghs_mock_ephemeral_project_token";

fn project() -> Value {
    json!({
        "app_id": APP_ID,
        "name": "client-portal",
        "app_url": "https://client-portal.example.test",
        "repo_full_name": REPO,
        "clone_url": format!("https://github.com/{REPO}.git"),
        "default_branch": "main",
        "first_deploy_id": null,
    })
}

fn setup(server: &Server) -> TempDir {
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
    home
}

fn isolated(command: &mut Command, home: &TempDir) {
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("FLOO_CONFIG_DIR", home.path())
        .env_remove("FLOO_API_URL")
        .env("FLOO_NO_UPDATE_CHECK", "1")
        .env_remove("FORCE_COLOR");
}

#[allow(deprecated)]
fn floo(home: &TempDir) -> Command {
    let mut command = Command::cargo_bin("floo-local").unwrap();
    isolated(&mut command, home);
    command
}

fn list_mock(server: &mut Server, projects: Vec<Value>) -> mockito::Mock {
    server
        .mock("GET", "/v1/projects")
        .match_header("authorization", "Bearer floo_test123")
        .match_header("x-floo-org-id", "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
        .with_status(200)
        .with_body(json!({"projects": projects}).to_string())
        .create()
}

fn token_mock(server: &mut Server, token: &str) -> mockito::Mock {
    server
        .mock("POST", format!("/v1/projects/{APP_ID}/git-token").as_str())
        .match_header("authorization", "Bearer floo_test123")
        .match_header("x-floo-org-id", "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
        .with_status(200)
        .with_body(
            json!({
                "token": token,
                "expires_at": "2026-09-09T13:00:00Z",
                "clone_url": project()["clone_url"],
            })
            .to_string(),
        )
        .create()
}

#[test]
fn create_prints_project_block_and_followup_without_waiting() {
    let mut server = Server::new();
    let home = setup(&server);
    let mut created = project();
    created["first_deploy_id"] = json!("first-deploy-id");
    let mock = server
        .mock("POST", "/v1/projects")
        .match_header("authorization", "Bearer floo_test123")
        .match_header("x-floo-org-id", "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
        .match_body(Matcher::Json(json!({"name": "client-portal"})))
        .with_status(201)
        .with_body(created.to_string())
        .create();
    floo(&home)
        .args(["projects", "create", "client-portal"])
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains("app: client-portal"))
        .stderr(predicate::str::contains(
            "url: https://client-portal.example.test",
        ))
        .stderr(predicate::str::contains("login: hosted, invite-only"))
        .stderr(predicate::str::contains("database: postgres"))
        .stderr(predicate::str::contains(format!(
            "repo: https://github.com/{REPO}.git"
        )))
        .stderr(predicate::str::contains(
            "floo deploys watch --app client-portal",
        ))
        .stderr(predicate::str::contains(
            "next: floo projects clone client-portal",
        ));
    mock.assert();
}

#[test]
fn create_json_is_one_object_and_watch_is_optional() {
    for first_deploy in [None, Some("deploy-id")] {
        let mut server = Server::new();
        let home = setup(&server);
        let mut created = project();
        created["first_deploy_id"] = json!(first_deploy);
        let mock = server
            .mock("POST", "/v1/projects")
            .with_status(201)
            .with_body(created.to_string())
            .create();
        let result = floo(&home)
            .args(["projects", "create", "client-portal", "--json"])
            .assert()
            .success()
            .stderr("")
            .get_output()
            .stdout
            .clone();
        let result: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["data"]["app_id"], APP_ID);
        assert_eq!(result["data"]["login"], "hosted, invite-only");
        assert_eq!(result["data"]["database"], "postgres");
        assert_eq!(result["data"]["next"], "floo projects clone client-portal");
        assert_eq!(
            result["data"].get("watch").is_some(),
            first_deploy.is_some()
        );
        mock.assert();
    }
}

#[test]
fn list_renders_name_url_repo_and_empty_json_list() {
    let mut server = Server::new();
    let home = setup(&server);
    let mock = list_mock(&mut server, vec![project()]);
    floo(&home)
        .args(["projects", "list"])
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains("NAME"))
        .stderr(predicate::str::contains("client-portal"))
        .stderr(predicate::str::contains(
            "https://client-portal.example.test",
        ))
        .stderr(predicate::str::contains(REPO));
    mock.assert();
    mock.remove();
    let mock = list_mock(&mut server, vec![]);
    let output = floo(&home)
        .args(["projects", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<Value>(&output).unwrap()["data"]["projects"],
        json!([])
    );
    mock.assert();
}

#[test]
fn credential_get_only_emits_password_protocol_and_fetches_each_time() {
    let mut server = Server::new();
    let home = setup(&server);
    let config_before = std::fs::read(home.path().join("config.json")).unwrap();
    for suffix in ["", ".git"] {
        let mock = token_mock(&mut server, TOKEN);
        floo(&home)
            .args(["projects", "git-credential", "get", "--json"])
            .write_stdin(format!("protocol=https\nhost=github.com\npath={REPO}{suffix}\nusername=ignored\n\npath=ignored-after-blank\n"))
            .assert()
            .success()
            .stdout(format!("username=x-access-token\npassword={TOKEN}\n\n"))
            .stderr("");
        mock.assert();
        mock.remove();
    }
    assert_eq!(
        std::fs::read(home.path().join("config.json")).unwrap(),
        config_before
    );
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 1);
}

#[test]
fn credential_ignores_nonmatching_requests_without_authentication() {
    let home = TempDir::new().unwrap();
    let matching = format!("protocol=https\nhost=github.com\npath={REPO}.git\n\n");
    let requests = [
        String::new(),
        matching.replace("https", "http"),
        matching.replace("github.com", "github.com.evil.test"),
        matching.replace("github.com", "github.com:443"),
        matching.replace(REPO, "customer/ordinary-repo"),
        matching.replace(REPO, &format!("{REPO}/extra")),
        matching.replace(REPO, &format!("/{REPO}")),
        matching.replace("0123456789abcdef0123456789abcdef", "not-hex"),
        matching.replace(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "gggggggggggggggggggggggggggggggg",
        ),
        matching.replace(".git", ".git?query=1"),
        matching.replace(".git", ".git.git"),
        "protocol=https\nhost=github.com\n\n".to_string(),
        matching.replace("host=github.com", "host=github.com\nhost=github.com"),
        matching.replace("protocol=https", "malformed"),
    ];
    for request in requests {
        floo(&home)
            .args(["projects", "git-credential", "get"])
            .write_stdin(request)
            .assert()
            .success()
            .stdout("")
            .stderr("");
    }
}

#[test]
fn credential_store_and_erase_are_noops() {
    let home = TempDir::new().unwrap();
    for operation in ["store", "erase"] {
        floo(&home)
            .args(["projects", "git-credential", operation, "--json"])
            .write_stdin(format!(
                "protocol=https\nhost=github.com\npath={REPO}.git\npassword={TOKEN}\n\n"
            ))
            .assert()
            .success()
            .stdout("")
            .stderr("");
    }
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn credential_rejects_protocol_injection_without_printing_token() {
    for token in [
        "secret\npassword=injected",
        "secret\rfield=injected",
        "secret\0",
        "",
    ] {
        let mut server = Server::new();
        let home = setup(&server);
        let mock = token_mock(&mut server, token);
        floo(&home)
            .args(["projects", "git-credential", "get"])
            .write_stdin(format!(
                "protocol=https\r\nhost=github.com\r\npath={REPO}\r\n\r\n"
            ))
            .assert()
            .failure()
            .stdout("")
            .stderr(predicate::str::contains("secret").not())
            .stderr(predicate::str::contains("next: floo projects list"));
        mock.assert();
    }
}

#[test]
fn project_api_errors_preserve_codes_messages_and_hints() {
    for (status, code) in [
        (503, "MANAGED_PROJECTS_DISABLED"),
        (403, "MANAGED_REPO_NOT_OWNED"),
        (404, "PROJECT_NOT_FOUND"),
    ] {
        let mut server = Server::new();
        let home = setup(&server);
        let mock = server.mock("POST", "/v1/projects")
            .with_status(status)
            .with_body(json!({"detail": {"code": code, "message": "Project unavailable.", "hint": "Contact your organization admin."}}).to_string())
            .expect(2)
            .create();
        floo(&home)
            .args(["projects", "create", "client-portal"])
            .assert()
            .failure()
            .stdout("")
            .stderr(predicate::str::contains("Project unavailable."))
            .stderr(predicate::str::contains("Contact your organization admin."))
            .stderr(predicate::str::contains("next: floo projects list\n"));
        let output = floo(&home)
            .args(["projects", "create", "client-portal", "--json"])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let output: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(output["error"]["code"], code);
        assert_eq!(output["error"]["hint"], "Contact your organization admin.");
        assert!(output["error"]["suggestion"]
            .as_str()
            .unwrap()
            .ends_with("next: floo projects list"));
        mock.assert();
    }
}

#[test]
fn credential_api_error_keeps_stdout_empty_even_in_json_mode() {
    let mut server = Server::new();
    let home = setup(&server);
    let mock = server.mock("POST", format!("/v1/projects/{APP_ID}/git-token").as_str())
        .with_status(403)
        .with_body(json!({"detail": {"code": "MANAGED_REPO_NOT_OWNED", "message": "Repository is not owned.", "hint": "Check your organization."}}).to_string())
        .create();
    floo(&home)
        .args(["projects", "git-credential", "get", "--json"])
        .write_stdin(format!("protocol=https\nhost=github.com\npath={REPO}\n\n"))
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("Repository is not owned."))
        .stderr(predicate::str::contains("Check your organization."))
        .stderr(predicate::str::contains("next: floo projects list"));
    mock.assert();
}

#[test]
fn clone_missing_project_and_missing_git_have_next_commands() {
    let mut server = Server::new();
    let home = setup(&server);
    let mock = list_mock(&mut server, vec![]);
    floo(&home)
        .args(["projects", "clone", "missing", "--json"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("PROJECT_NOT_FOUND"))
        .stdout(predicate::str::contains("next: floo projects list"));
    mock.assert();
    mock.remove();
    let mock = list_mock(&mut server, vec![project()]);
    floo(&home)
        .env("PATH", home.path())
        .args(["projects", "clone", APP_ID])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains(
            "Git is not installed or is not on PATH",
        ))
        .stderr(predicate::str::contains(format!(
            "next: floo projects clone {APP_ID}"
        )));
    mock.assert();
}

#[cfg(unix)]
mod git_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn git_shim(home: &TempDir) -> std::path::PathBuf {
        let bin = home.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let git = bin.join("git");
        std::fs::write(&git, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$GIT_ARGS\"\necho 'mock git progress'\nexit \"${GIT_EXIT:-0}\"\n").unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    fn clone_args(home: &TempDir) -> Vec<String> {
        std::fs::read_to_string(home.path().join("git-args"))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn clone_uses_repo_local_config_for_name_and_id_and_preserves_json_stdout() {
        let mut server = Server::new();
        let home = setup(&server);
        let bin = git_shim(&home);
        for (name_or_id, dir) in [
            ("client-portal", None),
            (APP_ID, Some("destination with spaces")),
        ] {
            let mock = list_mock(&mut server, vec![project()]);
            let mut command = floo(&home);
            let executable = command.get_program().to_string_lossy().to_string();
            command
                .env("PATH", &bin)
                .env("GIT_ARGS", home.path().join("git-args"))
                .args(["projects", "clone", name_or_id, "--json"]);
            if let Some(dir) = dir {
                command.arg(dir);
            }
            let output = command
                .assert()
                .success()
                .stderr(predicate::str::contains("mock git progress"))
                .get_output()
                .stdout
                .clone();
            let output: Value = serde_json::from_slice(&output).unwrap();
            let expected_dir = dir.unwrap_or("client-portal");
            assert_eq!(output["data"]["directory"], expected_dir);
            assert_eq!(output["data"]["next"], "read AGENTS.md");
            let args = clone_args(&home);
            assert_eq!(args[0..4], ["clone", "-c", "credential.helper=", "-c"]);
            assert_eq!(
                args[4],
                format!("credential.helper=!{executable} projects git-credential")
            );
            assert_eq!(args[5..8], ["-c", "credential.useHttpPath=true", "--"]);
            assert_eq!(args[8], project()["clone_url"]);
            assert_eq!(args[9], expected_dir);
            mock.assert();
            mock.remove();
        }
    }

    #[test]
    fn clone_git_failure_is_reported_with_retry_command() {
        let mut server = Server::new();
        let home = setup(&server);
        let bin = git_shim(&home);
        let mock = list_mock(&mut server, vec![project()]);
        floo(&home)
            .env("PATH", bin)
            .env("GIT_ARGS", home.path().join("git-args"))
            .env("GIT_EXIT", "128")
            .args(["projects", "clone", "client-portal"])
            .assert()
            .failure()
            .stdout("")
            .stderr(predicate::str::contains("git clone failed"))
            .stderr(predicate::str::contains(
                "next: floo projects clone client-portal client-portal",
            ));
        mock.assert();
    }

    #[test]
    fn clone_rejects_credential_urls_and_non_managed_transports_before_git() {
        for url in [
            "https://token@github.com/owner/repo",
            "ext::arbitrary-command",
            "ssh://github.com/owner/repo",
            "https://github.com/owner/ordinary-repo",
        ] {
            let mut server = Server::new();
            let home = setup(&server);
            let bin = git_shim(&home);
            let mut project = project();
            project["clone_url"] = json!(url);
            let mock = list_mock(&mut server, vec![project]);
            floo(&home)
                .env("PATH", bin)
                .env("GIT_ARGS", home.path().join("git-args"))
                .args(["projects", "clone", "client-portal"])
                .assert()
                .failure()
                .stdout("")
                .stderr(predicate::str::contains("Project clone URL must be"))
                .stderr(predicate::str::contains(url).not());
            assert!(!home.path().join("git-args").exists());
            mock.assert();
        }
    }

    // Real git only operates on an empty local repository here. All HTTP calls
    // go to mockito; no fetch, push, or external host is contacted.
    #[test]
    #[allow(deprecated)]
    fn git_executes_quoted_workspace_helper_and_never_approves_to_inherited_helpers() {
        let mut server = Server::new();
        let home = setup(&server);
        let bin = git_shim(&home);
        let workspace = home.path().join("agent's workspace $literal;dir");
        std::fs::create_dir(&workspace).unwrap();
        let executable = workspace.join("floo-local");
        std::fs::copy(assert_cmd::cargo::cargo_bin("floo-local"), &executable).unwrap();
        let mock = list_mock(&mut server, vec![project()]);
        let mut command = Command::new(&executable);
        isolated(&mut command, &home);
        command
            .env("PATH", bin)
            .env("GIT_ARGS", home.path().join("git-args"))
            .args(["projects", "clone", "client-portal"])
            .assert()
            .success()
            .stdout("")
            .stderr(predicate::str::contains(
                "directory: client-portal\nnext: read AGENTS.md",
            ));
        mock.assert();
        let args = clone_args(&home);
        let repo = home.path().join("local-repo");
        std::fs::create_dir(&repo).unwrap();
        let inherited = home.path().join("inherited-helper-called");
        std::fs::write(
            home.path().join(".gitconfig"),
            format!(
                "[credential]\n\thelper = !echo called > {}\n",
                inherited.display()
            ),
        )
        .unwrap();
        let global_before = std::fs::read(home.path().join(".gitconfig")).unwrap();
        let git = || {
            let mut command = Command::new("git");
            isolated(&mut command, &home);
            command
                .current_dir(&repo)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", home.path().join(".gitconfig"))
                .env("GIT_TERMINAL_PROMPT", "0")
                .env_remove("GIT_CONFIG_COUNT")
                .env_remove("GIT_CONFIG_PARAMETERS")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE");
            command
        };
        git().arg("init").assert().success();
        for config in [&args[2], &args[4], &args[6]] {
            let (key, value) = config.split_once('=').unwrap();
            git()
                .args(["config", "--local", "--add", key, value])
                .assert()
                .success();
        }
        let token = token_mock(&mut server, TOKEN);
        let input = format!("protocol=https\nhost=github.com\npath={REPO}.git\n\n");
        let result = git()
            .args(["credential", "fill"])
            .write_stdin(input)
            .assert()
            .success()
            .stderr("")
            .get_output()
            .stdout
            .clone();
        let result_text = String::from_utf8(result.clone()).unwrap();
        assert_eq!(result_text.matches(TOKEN).count(), 1);
        assert!(result_text
            .lines()
            .any(|line| line == format!("password={TOKEN}")));
        token.assert();
        git()
            .args(["credential", "approve"])
            .write_stdin(result.clone())
            .assert()
            .success()
            .stdout("")
            .stderr("");
        git()
            .args(["credential", "reject"])
            .write_stdin(result)
            .assert()
            .success()
            .stdout("")
            .stderr("");
        assert!(
            !inherited.exists(),
            "inherited helpers must never receive tokens"
        );
        assert_eq!(
            std::fs::read(home.path().join(".gitconfig")).unwrap(),
            global_before
        );
        assert!(!std::fs::read_to_string(repo.join(".git/config"))
            .unwrap()
            .contains(TOKEN));
        assert!(!home.path().join(".git-credentials").exists());
    }
}
