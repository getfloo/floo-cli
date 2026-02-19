use assert_cmd::Command;
use mockito::{Matcher, Mock, Server};
use predicates::prelude::*;
use sha2::{Digest, Sha256};
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

fn update_asset_name() -> Option<String> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc.exe",
        _ => return None,
    };
    Some(format!("floo-{target}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Mock resolve_app (name-based): list -> found.
fn mock_resolve_app(server: &mut Server) -> Vec<Mock> {
    let m = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"apps":[{app}]}}"#, app = app_json()))
        .create();
    vec![m]
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

// ───────────────────────── Databases ─────────────────────────

#[test]
fn test_db_info_json_uses_db_endpoint() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_db = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/db").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"host":"db.example.internal","port":5432,"database":"floo_apps","status":"READY","username":"floo_user","schema_name":"app_1234"}"#,
        )
        .create();

    floo()
        .args(["--json", "db", "info", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("db.example.internal"))
        .stdout(predicate::str::contains(r#""database":"floo_apps""#));
}

#[test]
fn test_db_info_json_falls_back_to_databases_endpoint() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_db_not_found = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/db").as_str())
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"NOT_FOUND","message":"Not found"}}"#)
        .create();

    let _m_databases = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/databases").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"databases":[{"id":"db-1","name":"default","host":"fallback-db.internal","port":5432,"database":"floo_apps","status":"READY","username":"floo_user","schema_name":"app_5678"}],"total":1}"#,
        )
        .create();

    floo()
        .args(["--json", "db", "info", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("fallback-db.internal"))
        .stdout(predicate::str::contains(r#""name":"default""#));
}

#[test]
fn test_db_info_json_app_not_found() {
    let mut server = Server::new();
    let home = setup_config(&server);

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
        .args(["--json", "db", "info", "missing-app"])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("APP_NOT_FOUND"));
}

#[test]
fn test_db_info_json_surfaces_api_errors() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_db = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/db").as_str())
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"INTERNAL_ERROR","message":"Server error"}}"#)
        .create();

    floo()
        .args(["--json", "db", "info", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("INTERNAL_ERROR"))
        .stdout(predicate::str::contains("APP_NOT_FOUND").not());
}

#[test]
fn test_db_info_json_returns_parse_error_for_malformed_payload() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_db = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/db").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"host":"db.example.internal","port":"bad","database":"floo_apps","status":"READY"}"#)
        .create();

    floo()
        .args(["--json", "db", "info", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("PARSE_ERROR"));
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

#[test]
fn test_deploy_existing_app_by_name_json() {
    let mut server = Server::new();
    let home = setup_config(&server);

    // Create a temp project with package.json for detection
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();

    let _m_list = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"apps":[{app}]}}"#, app = app_json()))
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
            r#"{{"id":"deploy-001","status":"live","url":"https://{TEST_APP_NAME}.floo.app","build_logs":""}}"#
        ))
        .create();

    floo()
        .args([
            "--json",
            "deploy",
            project.path().to_str().unwrap(),
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""id":"app-uuid-1234""#))
        .stdout(predicate::str::contains(r#""deploy""#));
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

#[test]
fn test_apps_status_uuid_surfaces_get_app_api_error() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let app_id = "11111111-1111-1111-1111-111111111111";

    let _m_get = server
        .mock("GET", format!("/v1/apps/{app_id}").as_str())
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"INTERNAL_ERROR","message":"Server error"}}"#)
        .create();

    floo()
        .args(["--json", "apps", "status", app_id])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("INTERNAL_ERROR"))
        .stdout(predicate::str::contains("APP_NOT_FOUND").not());
}

#[test]
fn test_apps_status_name_surfaces_list_apps_api_error() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let _m_list = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"INTERNAL_ERROR","message":"Server error"}}"#)
        .create();

    floo()
        .args(["--json", "apps", "status", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("INTERNAL_ERROR"))
        .stdout(predicate::str::contains("APP_NOT_FOUND").not());
}

// ───────────────────────── Update ─────────────────────────

#[test]
fn test_update_json_release_lookup_failure() {
    let mut server = Server::new();
    let _m_release = server
        .mock("GET", "/releases/latest")
        .with_status(404)
        .with_body("not found")
        .create();

    let install_dir = TempDir::new().unwrap();
    let install_path = install_dir.path().join("floo");

    floo()
        .args(["--json", "update"])
        .env("FLOO_UPDATE_API_BASE", format!("{}/releases", server.url()))
        .env("FLOO_UPDATE_TARGET_PATH", install_path.as_os_str())
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            r#""code":"RELEASE_LOOKUP_FAILED""#,
        ));
}

#[test]
fn test_update_json_success() {
    let Some(asset_name) = update_asset_name() else {
        return;
    };

    let binary_bytes = b"#!/usr/bin/env bash\necho floo v0.2.0\n";
    let checksum = sha256_hex(binary_bytes);

    let mut server = Server::new();
    let _m_release = server
        .mock("GET", "/releases/latest")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "tag_name": "v0.2.0",
                "assets": [
                    {
                        "name": asset_name,
                        "browser_download_url": format!("{}/downloads/{}", server.url(), asset_name),
                    },
                    {
                        "name": format!("{asset_name}.sha256"),
                        "browser_download_url": format!("{}/downloads/{}.sha256", server.url(), asset_name),
                    }
                ],
            })
            .to_string(),
        )
        .create();

    let _m_binary = server
        .mock("GET", format!("/downloads/{asset_name}").as_str())
        .with_status(200)
        .with_body(binary_bytes.as_slice())
        .create();

    let _m_checksum = server
        .mock("GET", format!("/downloads/{asset_name}.sha256").as_str())
        .with_status(200)
        .with_body(format!("{checksum}  {asset_name}"))
        .create();

    let install_dir = TempDir::new().unwrap();
    let install_path = install_dir.path().join("floo");

    floo()
        .args(["--json", "update"])
        .env("FLOO_UPDATE_API_BASE", format!("{}/releases", server.url()))
        .env("FLOO_UPDATE_TARGET_PATH", install_path.as_os_str())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""version":"v0.2.0""#));

    assert_eq!(
        std::fs::read(install_path).unwrap(),
        binary_bytes.as_slice()
    );
}
