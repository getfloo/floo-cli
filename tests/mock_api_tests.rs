use assert_cmd::Command;
use mockito::{Matcher, Mock, Server};
use predicates::prelude::*;
use tempfile::TempDir;

const TEST_APP_ID: &str = "app-uuid-1234";
const TEST_APP_NAME: &str = "my-app";

fn floo() -> Command {
    Command::cargo_bin("floo").unwrap()
}

/// Create temp HOME with config pointing at mock server.
fn setup_config(server: &Server) -> TempDir {
    let home = TempDir::new().unwrap();
    let config_dir = home.path().join(".floo");
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

/// App JSON fixture used across resolve_app and list mocks.
fn app_json() -> String {
    format!(
        r#"{{"id":"{TEST_APP_ID}","name":"{TEST_APP_NAME}","status":"live","url":"https://test.floo.app","runtime":"nodejs","created_at":"2024-01-01T00:00:00Z"}}"#
    )
}

/// Mock resolve_app (name-based): UUID lookup -> 404, list -> found.
fn mock_resolve_app(server: &mut Server) -> Vec<Mock> {
    let m1 = server
        .mock("GET", format!("/v1/apps/{TEST_APP_NAME}").as_str())
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"NOT_FOUND","message":"Not found"}}"#)
        .create();
    let m2 = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"apps":[{app}]}}"#, app = app_json()))
        .create();
    vec![m1, m2]
}

// ───────────────────────── Apps ─────────────────────────

#[test]
fn test_apps_list_json() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let _m = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "20".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"apps":[{app}]}}"#, app = app_json()))
        .create();

    floo()
        .args(["--json", "apps", "list"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""apps":[{"#))
        .stdout(predicate::str::contains(TEST_APP_NAME));
}

#[test]
fn test_apps_list_human() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let _m = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "20".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"apps":[{app}]}}"#, app = app_json()))
        .create();

    floo()
        .args(["apps", "list"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains(TEST_APP_NAME))
        .stderr(predicate::str::contains("Name"))
        .stderr(predicate::str::contains("Status"));
}

#[test]
fn test_apps_status_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    floo()
        .args(["--json", "apps", "status", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(TEST_APP_ID));
}

#[test]
fn test_apps_delete_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_delete = server
        .mock("DELETE", format!("/v1/apps/{TEST_APP_ID}").as_str())
        .with_status(204)
        .create();

    // --json auto-confirms via output::confirm()
    floo()
        .args(["--json", "apps", "delete", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(TEST_APP_ID));
}

#[test]
fn test_apps_list_api_error() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let _m = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "20".into()),
        ]))
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"INTERNAL_ERROR","message":"Server error"}}"#)
        .create();

    floo()
        .args(["--json", "apps", "list"])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""success":false"#))
        .stdout(predicate::str::contains("INTERNAL_ERROR"));
}

// ───────────────────────── Env ─────────────────────────

#[test]
fn test_env_set_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_set = server
        .mock("POST", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"key":"MY_KEY","masked_value":"my_v****"}"#)
        .create();

    floo()
        .args([
            "--json",
            "env",
            "set",
            "my_key=my_value",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("MY_KEY"));
}

#[test]
fn test_env_list_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"env_vars":[{"key":"DATABASE_URL","masked_value":"post****"}]}"#)
        .create();

    floo()
        .args(["--json", "env", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("env_vars"))
        .stdout(predicate::str::contains("DATABASE_URL"));
}

#[test]
fn test_env_remove_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_del = server
        .mock(
            "DELETE",
            format!("/v1/apps/{TEST_APP_ID}/env/MY_KEY").as_str(),
        )
        .with_status(204)
        .create();

    floo()
        .args(["--json", "env", "remove", "my_key", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("MY_KEY"));
}

// ───────────────────────── Domains ─────────────────────────

#[test]
fn test_domains_add_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_add = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/domains").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"hostname":"app.example.com","status":"PENDING","dns_instructions":"Add a CNAME record pointing to test.floo.app"}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "domains",
            "add",
            "app.example.com",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("app.example.com"));
}

#[test]
fn test_domains_list_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/domains").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"domains":[{"hostname":"app.example.com","status":"PENDING","dns_instructions":"Add CNAME"}]}"#,
        )
        .create();

    floo()
        .args(["--json", "domains", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("domains"))
        .stdout(predicate::str::contains("app.example.com"));
}

#[test]
fn test_domains_remove_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_del = server
        .mock(
            "DELETE",
            format!("/v1/apps/{TEST_APP_ID}/domains/app.example.com").as_str(),
        )
        .with_status(204)
        .create();

    floo()
        .args([
            "--json",
            "domains",
            "remove",
            "app.example.com",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("app.example.com"));
}

// ───────────────────────── Rollbacks ─────────────────────────

#[test]
fn test_rollbacks_list_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/deploys").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"deploys":[{"id":"deploy-123","status":"live","runtime":"nodejs","created_at":"2024-01-01T00:00:00Z"}]}"#,
        )
        .create();

    floo()
        .args(["--json", "rollbacks", "list", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("deploys"))
        .stdout(predicate::str::contains("deploy-123"));
}

#[test]
fn test_rollback_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_rollback = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/rollback").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"deploy-456","status":"live","runtime":"nodejs","created_at":"2024-01-02T00:00:00Z"}"#,
        )
        .create();

    // --json auto-confirms via output::confirm()
    floo()
        .args(["--json", "rollback", TEST_APP_NAME, "deploy-456"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("deploy"));
}

// ───────────────────────── Logs ─────────────────────────

#[test]
fn test_logs_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_logs = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/logs").as_str(),
        )
        .match_query(Matcher::UrlEncoded("limit".into(), "100".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"INFO","message":"Server started"}],"app_name":"my-app"}"#,
        )
        .create();

    floo()
        .args(["--json", "logs", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("logs"))
        .stdout(predicate::str::contains("app_name"));
}

#[test]
fn test_logs_human() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_logs = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/logs").as_str(),
        )
        .match_query(Matcher::UrlEncoded("limit".into(), "100".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"INFO","message":"Server started"}],"app_name":"my-app"}"#,
        )
        .create();

    floo()
        .args(["logs", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Logs for"))
        .stderr(predicate::str::contains("Server started"));
}

// ───────────────────────── Deploy ─────────────────────────

#[test]
fn test_deploy_new_app_json() {
    let mut server = Server::new();
    let home = setup_config(&server);

    // Create a temp project with package.json for detection
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();

    let _m_create_app = server
        .mock("POST", "/v1/apps")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"id":"{TEST_APP_ID}","name":"test-deploy","status":"created","runtime":"nodejs"}}"#
        ))
        .create();

    // Return status "live" immediately to skip the polling loop
    let _m_deploy = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/deploys").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"id":"deploy-001","status":"live","url":"https://test-deploy.floo.app","build_logs":""}}"#
        ))
        .create();

    floo()
        .args([
            "--json",
            "deploy",
            project.path().to_str().unwrap(),
            "--name",
            "test-deploy",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""app""#))
        .stdout(predicate::str::contains(r#""deploy""#))
        .stdout(predicate::str::contains(r#""detection""#));
}

// ───────────────────────── App Not Found ─────────────────────────

#[test]
fn test_app_not_found_json() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let _m_get = server
        .mock("GET", "/v1/apps/nonexistent")
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"NOT_FOUND","message":"Not found"}}"#)
        .create();

    let _m_list = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"apps":[]}"#)
        .create();

    floo()
        .args(["--json", "apps", "status", "nonexistent"])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("APP_NOT_FOUND"));
}
