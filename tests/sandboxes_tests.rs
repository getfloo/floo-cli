//! `floo sandboxes`: list by app and tag, read logs, tag, and terminate.

use assert_cmd::Command;
use mockito::{Matcher, Mock, Server};
use predicates::prelude::*;
use tempfile::TempDir;

const APP_ID: &str = "app-uuid-1234";
const SANDBOX: &str = "sb-abc123";

#[allow(deprecated)]
fn floo() -> Command {
    Command::cargo_bin("floo-local").unwrap()
}

fn setup_config(server: &Server) -> TempDir {
    let home = TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        format!(
            r#"{{"api_key":"floo_test123","api_url":"{}"}}"#,
            server.url()
        ),
    )
    .unwrap();
    home
}

fn mock_resolve_app(server: &mut Server) -> Mock {
    server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"apps":[{{"id":"{APP_ID}","name":"agents","org_id":"org-1","status":"live","url":"https://agents.getfloo.app","runtime":"nodejs","created_at":"2026-11-01T00:00:00Z"}}]}}"#
        ))
        .create()
}

const LISTED: &str = r#"{"sandboxes":[{"id":"sb-abc123","app_id":"app-uuid-1234","app_name":"agents","name":null,
  "state":"running","size":"small","vcpus":1,"memory_mib":512,"created_at":"2026-11-24T21:00:00Z",
  "agent_worker":false,"parent_id":null,"tags":{"user":"ada","task":"build"}}],
  "vcpus_in_use":1,"vcpus_limit":16}"#;

#[test]
fn list_asks_for_the_app_and_every_tag_and_shows_each_sandbox_with_its_tags() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _apps = mock_resolve_app(&mut server);
    let listed = server
        .mock("GET", "/v1/orgs/sandboxes")
        // Each tag is its own `tag` parameter, its `=` encoded.
        .match_query(Matcher::Regex(format!(
            "^tag=task%3Dbuild&tag=user%3Dada&app_id={APP_ID}$"
        )))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(LISTED)
        .create();

    floo()
        .args(["sandboxes", "list", "--app", "agents"])
        .args(["--tag", "task=build", "--tag", "user=ada"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains(SANDBOX))
        .stderr(predicate::str::contains("task=build,user=ada"))
        .stderr(predicate::str::contains("1 of 16 vCPUs in use."));
    listed.assert();
}

#[test]
fn list_without_an_app_is_the_whole_orgs_in_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _listed = server
        .mock("GET", "/v1/orgs/sandboxes")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(LISTED)
        .create();

    floo()
        .args(["--json", "sandboxes", "list"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""vcpus_limit":16"#));
}

#[test]
fn a_tag_that_is_not_name_equals_value_is_refused_before_any_call() {
    floo()
        .args(["--json", "sandboxes", "list", "--tag", "task"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("INVALID_FORMAT"));
}

#[test]
fn logs_print_each_stream_where_the_command_wrote_it() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _logs = server
        .mock("GET", format!("/v1/orgs/sandboxes/{SANDBOX}/logs").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"output":[{"stream":"stdout","text":"ready\n"},{"stream":"stderr","text":"warn\n"}],"running":true}"#,
        )
        .create();

    floo()
        .args(["sandboxes", "logs", SANDBOX])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout("ready\n")
        .stderr("warn\n");
}

#[test]
fn logs_of_a_sandbox_with_no_command_say_the_apis_reason() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _logs = server
        .mock("GET", format!("/v1/orgs/sandboxes/{SANDBOX}/logs").as_str())
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"SANDBOX_NOT_FOUND","message":"Sandbox 'sb-abc123' runs no command of its own"}}"#)
        .create();

    floo()
        .args(["--json", "sandboxes", "logs", SANDBOX])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("runs no command of its own"));
}

#[test]
fn tag_replaces_the_tags_and_clear_removes_them() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let path = format!("/v1/orgs/sandboxes/{SANDBOX}/tags");
    let tagged = server
        .mock("PUT", path.as_str())
        .match_body(Matcher::Json(
            serde_json::json!({"tags": {"task": "build", "user": "ada"}}),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"tags":{"task":"build","user":"ada"}}"#)
        .create();
    let cleared = server
        .mock("PUT", path.as_str())
        .match_body(Matcher::Json(serde_json::json!({"tags": {}})))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"tags":{}}"#)
        .create();

    floo()
        .args([
            "--json",
            "sandboxes",
            "tag",
            SANDBOX,
            "task=build",
            "user=ada",
        ])
        .env("HOME", home.path())
        .assert()
        .success();
    floo()
        .args(["--json", "sandboxes", "tag", SANDBOX, "--clear"])
        .env("HOME", home.path())
        .assert()
        .success();
    tagged.assert();
    cleared.assert();
}

#[test]
fn terminate_with_the_data_loss_flag_ends_it_and_says_what_was_lost() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let ended = server
        .mock(
            "POST",
            format!("/v1/orgs/sandboxes/{SANDBOX}/terminate").as_str(),
        )
        .with_status(204)
        .create();

    floo()
        .args(["--json", "sandboxes", "terminate", SANDBOX])
        .arg("--yes-i-know-this-destroys-data")
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""data_loss":true"#));
    ended.assert();
}

#[test]
fn terminate_without_confirmation_or_in_preflight_calls_nothing() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let ended = server.mock("POST", Matcher::Any).expect(0).create();

    floo()
        .args(["--json", "sandboxes", "terminate", SANDBOX])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("CONFIRMATION_REQUIRED"));
    floo()
        .args(["--json", "sandboxes", "terminate", SANDBOX, "--preflight"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Would terminate sandbox"));
    ended.assert();
}

#[test]
fn create_sends_what_was_asked_and_prints_only_the_id_on_stdout() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let created = server
        .mock("POST", "/v1/orgs/sandboxes")
        .match_body(Matcher::PartialJsonString(
            r#"{"app":"agents","size":"medium","timeout":900,"env":{"DEBUG":"1"},"tags":{"task":"build"}}"#
                .to_string(),
        ))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"id":"{SANDBOX}","app_id":"00000000-0000-0000-0000-000000000001","app_name":"agents"}}"#
        ))
        .create();

    floo()
        .args(["sandboxes", "create", "--app", "agents", "--size", "medium"])
        .args([
            "--timeout",
            "900",
            "--env",
            "DEBUG=1",
            "--tag",
            "task=build",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(format!("{SANDBOX}\n"));
    created.assert();
}

#[test]
fn exec_prints_each_stream_and_exits_with_the_commands_code() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let ran = server
        .mock("POST", format!("/v1/orgs/sandboxes/{SANDBOX}/exec").as_str())
        .match_body(Matcher::PartialJsonString(
            r#"{"command":["sh","-c","exit 3"],"workdir":"/work","timeout":300}"#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"exit_code":3,"stdout":"out\n","stderr":"err\n","timed_out":false,"truncated":false}"#,
        )
        .create();

    floo()
        .args(["sandboxes", "exec", SANDBOX, "--workdir", "/work"])
        .args(["--", "sh", "-c", "exit 3"])
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stdout("out\n")
        .stderr(predicate::str::contains("err\n"));
    ran.assert();
}

#[test]
fn cp_uploads_text_and_downloads_bytes_as_they_were() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let local = home.path().join("app.py");
    std::fs::write(&local, "print(1)\n").unwrap();
    let wrote = server
        .mock(
            "PUT",
            format!("/v1/orgs/sandboxes/{SANDBOX}/files/content").as_str(),
        )
        .match_query(Matcher::UrlEncoded("path".into(), "/work/app.py".into()))
        .match_body(Matcher::Json(
            serde_json::json!({"content": "print(1)\n", "encoding": "utf-8"}),
        ))
        .with_status(204)
        .create();
    let read = server
        .mock(
            "GET",
            format!("/v1/orgs/sandboxes/{SANDBOX}/files/content").as_str(),
        )
        .match_query(Matcher::UrlEncoded("path".into(), "/work/logo.bin".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"content":"/wA=","encoding":"base64"}"#)
        .create();
    let copied = home.path().join("logo.bin");

    floo()
        .args(["sandboxes", "cp", local.to_str().unwrap()])
        .arg(format!("{SANDBOX}:/work/app.py"))
        .env("HOME", home.path())
        .assert()
        .success();
    floo()
        .args(["sandboxes", "cp", &format!("{SANDBOX}:/work/logo.bin")])
        .arg(copied.to_str().unwrap())
        .env("HOME", home.path())
        .assert()
        .success();

    wrote.assert();
    read.assert();
    assert_eq!(std::fs::read(&copied).unwrap(), vec![0xff, 0x00]);
}

#[test]
fn cp_between_two_local_paths_is_refused_before_any_call() {
    floo()
        .args(["sandboxes", "cp", "./a", "./b"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("One side of `floo sandboxes cp`"));
}

#[test]
fn ls_shows_each_entry_with_directories_marked() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _listed = server
        .mock("GET", format!("/v1/orgs/sandboxes/{SANDBOX}/files").as_str())
        .match_query(Matcher::UrlEncoded("path".into(), "/work".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"files":[{"name":"src","type":"dir","size":0},{"name":"app.py","type":"file","size":9}]}"#)
        .create();

    floo()
        .args(["sandboxes", "ls", SANDBOX, "/work"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("src/"))
        .stderr(predicate::str::contains("app.py"));
}
