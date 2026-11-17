//! `floo events`: each event's repair run in the list, and starting one by hand.

use assert_cmd::Command;
use mockito::{Matcher, Mock, Server};
use predicates::prelude::*;
use tempfile::TempDir;

const APP_ID: &str = "app-uuid-1234";
const EVENT_ID: &str = "evt-uuid-1";

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
            r#"{{"apps":[{{"id":"{APP_ID}","name":"my-app","org_id":"org-1","status":"live","url":"https://my-app.getfloo.app","runtime":"nodejs","created_at":"2026-11-01T00:00:00Z"}}]}}"#
        ))
        .create()
}

#[test]
fn list_shows_each_repair_run_and_its_pull_request() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _apps = mock_resolve_app(&mut server);
    let _events = server
        .mock("GET", format!("/v1/apps/{APP_ID}/reparo/events").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"events":[
              {"id":"e1","status":"triggered","repair_phase":"verifying","repair_failure_reason":null,
               "repair_pr_url":"https://github.com/floo-managed/my-app/pull/7","created_at":"2026-11-09T00:00:00Z"},
              {"id":"e2","status":"triggered","repair_phase":"failed","repair_failure_reason":"no_fix",
               "repair_pr_url":null,"created_at":"2026-11-09T00:00:00Z"},
              {"id":"e3","status":"succeeded","repair_phase":null,"repair_failure_reason":null,
               "repair_pr_url":null,"created_at":"2026-11-09T00:00:00Z"}
            ],"total":3,"page":1,"per_page":20}"#,
        )
        .create();

    floo()
        .args(["events", "list", "--app", "my-app"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Repair"))
        .stderr(predicate::str::contains("verifying"))
        .stderr(predicate::str::contains(
            "https://github.com/floo-managed/my-app/pull/7",
        ))
        .stderr(predicate::str::contains("failed: no_fix"));
}

#[test]
fn repair_starts_a_run_for_the_event() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _apps = mock_resolve_app(&mut server);
    let start = server
        .mock(
            "POST",
            format!("/v1/apps/{APP_ID}/reparo/events/{EVENT_ID}/repair").as_str(),
        )
        .with_status(202)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"id":"{EVENT_ID}","repair_phase":"pending"}}"#))
        .create();

    floo()
        .args(["--json", "events", "repair", EVENT_ID, "--app", "my-app"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""repair_phase":"pending""#));
    start.assert();
}

#[test]
fn repair_reports_the_api_refusal() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _apps = mock_resolve_app(&mut server);
    let _refused = server
        .mock(
            "POST",
            format!("/v1/apps/{APP_ID}/reparo/events/{EVENT_ID}/repair").as_str(),
        )
        .with_status(409)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"detail":{"code":"REPAIR_MODE_OFF","message":"Turn on repair mode for this app first."}}"#,
        )
        .create();

    floo()
        .args(["events", "repair", EVENT_ID, "--app", "my-app"])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Turn on repair mode"));
}

#[test]
fn diff_prints_the_commit_to_approve_and_each_patch() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _apps = mock_resolve_app(&mut server);
    let head = "c".repeat(40);
    let _diff = server
        .mock(
            "GET",
            format!("/v1/apps/{APP_ID}/reparo/events/{EVENT_ID}/diff").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"pr_url":"https://github.com/floo-managed/my-app/pull/7","head_sha":"{head}","changed_files":3,
               "files":[{{"filename":"app.py","status":"modified","additions":1,"deletions":1,"patch":"-a\n+b"}},
                        {{"filename":"logo.png","status":"added","additions":0,"deletions":0,"patch":null}}]}}"#
        ))
        .create();

    floo()
        .args(["events", "diff", EVENT_ID, "--app", "my-app"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "floo events approve {EVENT_ID} --commit {head}"
        )))
        .stderr(predicate::str::contains("+b"))
        .stderr(predicate::str::contains("No line-by-line view"))
        .stderr(predicate::str::contains("GitHub lists 2 of the 3 files"));
}

#[test]
fn diff_of_a_recommendation_shows_the_agents_evidence_and_no_commit() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _apps = mock_resolve_app(&mut server);
    let _diff = server
        .mock(
            "GET",
            format!("/v1/apps/{APP_ID}/reparo/events/{EVENT_ID}/diff").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"pr_url":null,"head_sha":null,"changed_files":2,
               "files":[{"filename":"app/pricing.py","status":"modified","additions":1,"deletions":1,"patch":"-a\n+b"},
                        {"filename":"lib/b.py","status":"renamed","additions":0,"deletions":0,"patch":null,"previous_filename":"lib/a.py"}],
               "diagnosis":"Order 1002 has no discount.","reproduction_service":"web",
               "reproduction":[{"method":"GET","path":"/orders/1002","body":null},
                               {"method":"POST","path":"/orders","body":"{\"id\": \"9\"}"}],"reproduction_blocked":null}"#,
        )
        .create();

    floo()
        .args(["events", "diff", EVENT_ID, "--app", "my-app"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("floo didn't apply or test it"))
        .stderr(predicate::str::contains("floo events approve").not())
        .stderr(predicate::str::contains("Order 1002 has no discount."))
        .stderr(predicate::str::contains("to the web service"))
        .stderr(predicate::str::contains("GET /orders/1002"))
        .stderr(predicate::str::contains("POST /orders"))
        .stderr(predicate::str::contains(r#"{"id": "9"}"#))
        .stderr(predicate::str::contains("lib/a.py → lib/b.py"));
}

#[test]
fn approve_sends_the_reviewed_commit() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _apps = mock_resolve_app(&mut server);
    let head = "c".repeat(40);
    let approve = server
        .mock(
            "POST",
            format!("/v1/apps/{APP_ID}/reparo/events/{EVENT_ID}/approve").as_str(),
        )
        .match_body(Matcher::Json(serde_json::json!({ "commit_sha": head })))
        .with_status(202)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"id":"{EVENT_ID}","repair_phase":"verifying"}}"#
        ))
        .create();

    floo()
        .args([
            "events", "approve", EVENT_ID, "--commit", &head, "--app", "my-app",
        ])
        .env("HOME", home.path())
        .assert()
        .success();
    approve.assert();
}

#[test]
fn merge_reports_a_head_moved_since_verification() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _apps = mock_resolve_app(&mut server);
    let _merge = server
        .mock(
            "POST",
            format!("/v1/apps/{APP_ID}/reparo/events/{EVENT_ID}/merge").as_str(),
        )
        .with_status(409)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"detail":{"code":"REPAIR_HEAD_MOVED","message":"The pull request changed after floo verified it; it needs a new approval first."}}"#,
        )
        .create();

    floo()
        .args(["events", "merge", EVENT_ID, "--app", "my-app"])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("changed after floo verified it"));
}
