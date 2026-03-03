use assert_cmd::Command;
use mockito::{Matcher, Mock, Server};
use predicates::prelude::*;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const TEST_APP_ID: &str = "app-uuid-1234";
const TEST_APP_NAME: &str = "my-app";
const TEST_ORG_ID: &str = "org-uuid-5678";

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
        r#"{{"id":"{TEST_APP_ID}","name":"{TEST_APP_NAME}","org_id":"{TEST_ORG_ID}","status":"live","url":"https://test.floo.app","runtime":"nodejs","created_at":"2024-01-01T00:00:00Z"}}"#
    )
}

/// Org JSON fixture for org lookup mocks.
fn org_json() -> String {
    format!(
        r#"{{"id":"{TEST_ORG_ID}","name":"Test Org","slug":"test-org","spend_cap":null,"current_period_spend_cents":0,"spend_cap_exceeded":false}}"#
    )
}

/// Mock GET /v1/orgs/me for human-mode list.
fn mock_org_me(server: &mut Server) -> Mock {
    server
        .mock("GET", "/v1/orgs/me")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(org_json())
        .create()
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

/// Write a floo.service.toml to the project dir (required for --json deploy).
fn write_service_config(project: &TempDir, app_name: &str) {
    std::fs::write(
        project.path().join("floo.service.toml"),
        format!(
            r#"[app]
name = "{app_name}"

[service]
name = "web"
type = "web"
port = 3000
ingress = "public"
"#
        ),
    )
    .unwrap();
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

/// Mock a single user-managed service (web).
fn mock_services_single(server: &mut Server) -> Mock {
    server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/services").as_str(),
        )
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"services":[{"id":"svc-web-1","name":"web","type":"web","status":"live","cloud_run_url":"https://web.floo.app","port":3000}]}"#)
        .create()
}

/// Mock two user-managed services (api + web).
fn mock_services_multi(server: &mut Server) -> Mock {
    server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/services").as_str(),
        )
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"services":[{"id":"svc-api-1","name":"api","type":"api","status":"live","cloud_run_url":"https://api.floo.app","port":8000},{"id":"svc-web-1","name":"web","type":"web","status":"live","cloud_run_url":"https://web.floo.app","port":3000}]}"#)
        .create()
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
            Matcher::UrlEncoded("per_page".into(), "50".into()),
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
        .stdout(predicate::str::contains(TEST_APP_NAME))
        .stdout(predicate::str::contains(TEST_ORG_ID));
}

#[test]
fn test_apps_list_human() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let _m = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "50".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"apps":[{app}]}}"#, app = app_json()))
        .create();

    let _m_org = mock_org_me(&mut server);

    floo()
        .args(["apps", "list"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains(TEST_APP_NAME))
        .stderr(predicate::str::contains("Name"))
        .stderr(predicate::str::contains("Status"))
        .stderr(predicate::str::contains("Org"));
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
            Matcher::UrlEncoded("per_page".into(), "50".into()),
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

const TEST_SERVICE_ID: &str = "svc-uuid-1234";
const TEST_SERVICE_NAME: &str = "web";

fn mock_list_services_one(server: &mut Server) -> Mock {
    server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/services").as_str())
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"services":[{{"id":"{TEST_SERVICE_ID}","name":"{TEST_SERVICE_NAME}"}}]}}"#
        ))
        .create()
}

fn mock_list_services_two(server: &mut Server) -> Mock {
    server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/services").as_str(),
        )
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"services":[{{"id":"{TEST_SERVICE_ID}","name":"web"}},{{"id":"svc-uuid-5678","name":"api"}}]}}"#
        ))
        .create()
}

#[test]
fn test_env_set_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

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
    let _services = mock_list_services_one(&mut server);

    let _m_list = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .match_query(Matcher::UrlEncoded(
            "service_id".into(),
            TEST_SERVICE_ID.into(),
        ))
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
    let _services = mock_list_services_one(&mut server);

    let _m_del = server
        .mock(
            "DELETE",
            format!("/v1/apps/{TEST_APP_ID}/env/MY_KEY").as_str(),
        )
        .match_query(Matcher::UrlEncoded(
            "service_id".into(),
            TEST_SERVICE_ID.into(),
        ))
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

#[test]
fn test_env_get_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

    let _m_get = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/env/MY_KEY").as_str())
        .match_query(Matcher::UrlEncoded(
            "service_id".into(),
            TEST_SERVICE_ID.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"key":"MY_KEY","value":"secret123"}"#)
        .create();

    floo()
        .args(["--json", "env", "get", "MY_KEY", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("secret123"));
}

#[test]
fn test_env_get_human_raw_stdout() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

    let _m_get = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/env/MY_KEY").as_str())
        .match_query(Matcher::UrlEncoded(
            "service_id".into(),
            TEST_SERVICE_ID.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"key":"MY_KEY","value":"secret123"}"#)
        .create();

    floo()
        .args(["env", "get", "MY_KEY", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("secret123"));
}

#[test]
fn test_env_import_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

    let _m_import = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/env/import").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"imported":2}"#)
        .create();

    // Write a temp .env file
    let env_dir = TempDir::new().unwrap();
    let env_path = env_dir.path().join(".env");
    std::fs::write(&env_path, "KEY1=val1\nKEY2=val2\n").unwrap();

    floo()
        .args([
            "--json",
            "env",
            "import",
            env_path.to_str().unwrap(),
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("imported"));
}

#[test]
fn test_env_multiple_services_error() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_two(&mut server);

    floo()
        .args(["--json", "env", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("MULTIPLE_SERVICES"))
        .stdout(predicate::str::contains("web"))
        .stdout(predicate::str::contains("api"));
}

// ───────────────────────── Domains ─────────────────────────

#[test]
fn test_domains_add_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_services_single(&mut server);

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
    let _services = mock_services_single(&mut server);

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
    let _services = mock_services_single(&mut server);

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

#[test]
fn test_domains_list_multi_service_requires_services_flag() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_services_multi(&mut server);

    floo()
        .args(["--json", "domains", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("MULTIPLE_SERVICES_NO_TARGET"));
}

#[test]
fn test_domains_list_multi_service_with_services_flag() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_services_multi(&mut server);

    let _m_list = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/domains").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"domains":[{"hostname":"api.example.com","status":"ACTIVE","dns_instructions":""}]}"#)
        .create();

    floo()
        .args([
            "--json",
            "domains",
            "list",
            "--app",
            TEST_APP_NAME,
            "--services",
            "api",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("api.example.com"));
}

// ───────────────────────── Rollbacks ─────────────────────────

#[test]
fn test_deploy_list_json() {
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
        .args(["--json", "deploy", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("deploys"))
        .stdout(predicate::str::contains("deploy-123"));
}

#[test]
fn test_deploy_rollback_json() {
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
        .args(["--json", "deploy", "rollback", TEST_APP_NAME, "deploy-456"])
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
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"INFO","message":"Server started","service_name":"web"}],"app_name":"my-app"}"#,
        )
        .create();

    floo()
        .args(["--json", "logs", "--app", TEST_APP_NAME])
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
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"INFO","message":"Server started","service_name":"web"}],"app_name":"my-app"}"#,
        )
        .create();

    floo()
        .args(["logs", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("App:"))
        .stderr(predicate::str::contains(TEST_APP_NAME))
        .stderr(predicate::str::contains("Server started"));
}

#[test]
fn test_logs_from_config_file() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.service.toml"),
        format!(
            r#"[app]
name = "{TEST_APP_NAME}"

[service]
name = "web"
type = "web"
port = 3000
ingress = "public"
"#
        ),
    )
    .unwrap();

    let _m_logs = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/logs").as_str(),
        )
        .match_query(Matcher::UrlEncoded("limit".into(), "100".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"INFO","message":"Config resolved","service_name":"web"}],"app_name":"my-app"}"#,
        )
        .create();

    floo()
        .args(["--json", "logs"])
        .env("HOME", home.path())
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("Config resolved"));
}

#[test]
fn test_logs_no_config_no_app_errors() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let project = TempDir::new().unwrap();

    floo()
        .args(["--json", "logs"])
        .env("HOME", home.path())
        .current_dir(project.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NO_CONFIG_FOUND"#));
}

#[test]
fn test_logs_multi_services() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_logs_api = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/logs").as_str(),
        )
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "100".into()),
            Matcher::UrlEncoded("service".into(), "api".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:01Z","severity":"INFO","message":"API log","service_name":"api"}],"app_name":"my-app"}"#,
        )
        .create();

    let _m_logs_web = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/logs").as_str(),
        )
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "100".into()),
            Matcher::UrlEncoded("service".into(), "web".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"INFO","message":"Web log","service_name":"web"}],"app_name":"my-app"}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "logs",
            "--app",
            TEST_APP_NAME,
            "--services",
            "api",
            "--services",
            "web",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("API log"))
        .stdout(predicate::str::contains("Web log"));
}

#[test]
fn test_logs_search_filter() {
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
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"INFO","message":"Server started","service_name":"web"},{"timestamp":"2024-01-01T00:00:01Z","severity":"ERROR","message":"Connection error occurred","service_name":"web"}],"app_name":"my-app"}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "logs",
            "--app",
            TEST_APP_NAME,
            "--search",
            "error",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("Connection error occurred"))
        .stdout(predicate::str::contains("Server started").not());
}

#[test]
fn test_logs_single_service_no_prefix() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_logs = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/logs").as_str(),
        )
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "100".into()),
            Matcher::UrlEncoded("service".into(), "api".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"INFO","message":"Single service log","service_name":"api"}],"app_name":"my-app"}"#,
        )
        .create();

    floo()
        .args(["logs", "--app", TEST_APP_NAME, "--services", "api"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Single service log"))
        .stderr(predicate::str::contains("[api]").not());
}

// ───────────────────────── Services ─────────────────────────

#[test]
fn test_services_list_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_services_single(&mut server);

    floo()
        .args(["--json", "services", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("services"))
        .stdout(predicate::str::contains("web"));
}

#[test]
fn test_services_info_user_managed() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_services_single(&mut server);

    floo()
        .args(["--json", "services", "info", "web", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("web"))
        .stdout(predicate::str::contains("https://web.floo.app"));
}

#[test]
fn test_services_info_db_uses_db_endpoint() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    // No user-managed services
    let _m_services = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/services").as_str())
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"services":[]}"#)
        .create();

    let _m_db = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/db").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"host":"db.example.internal","port":5432,"database":"floo_apps","status":"READY","username":"floo_user","schema_name":"app_1234"}"#,
        )
        .create();

    floo()
        .args(["--json", "services", "info", "db", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("db.example.internal"))
        .stdout(predicate::str::contains(r#""database":"floo_apps""#));
}

#[test]
fn test_services_info_db_falls_back_to_databases_endpoint() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_services = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/services").as_str())
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"services":[]}"#)
        .create();

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
        .args(["--json", "services", "info", "db", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("fallback-db.internal"))
        .stdout(predicate::str::contains(r#""name":"default""#));
}

#[test]
fn test_services_info_app_not_found() {
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
        .args(["--json", "services", "info", "db", "--app", "missing-app"])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("APP_NOT_FOUND"));
}

#[test]
fn test_services_info_surfaces_api_errors() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_services = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/services").as_str())
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"INTERNAL_ERROR","message":"Server error"}}"#)
        .create();

    floo()
        .args(["--json", "services", "info", "db", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("INTERNAL_ERROR"))
        .stdout(predicate::str::contains("APP_NOT_FOUND").not());
}

#[test]
fn test_releases_list_from_config() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let project = TempDir::new().unwrap();
    write_service_config(&project, TEST_APP_NAME);

    let _m_releases = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/releases").as_str(),
        )
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "20".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"releases":[{"release_number":1,"tag":"v0.1.0","commit_sha":"abc1234","promoted_by":"user@example.com","created_at":"2024-01-01T00:00:00Z"}]}"#)
        .create();

    floo()
        .args(["--json", "releases", "list"])
        .current_dir(project.path())
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("v0.1.0"));
}

#[test]
fn test_deploy_list_from_config() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let project = TempDir::new().unwrap();
    write_service_config(&project, TEST_APP_NAME);

    let _m_deploys = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/deploys").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"deploys":[{"id":"deploy-123","status":"live","runtime":"nodejs","created_at":"2024-01-01T00:00:00Z"}]}"#)
        .create();

    floo()
        .args(["--json", "deploy", "list"])
        .current_dir(project.path())
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("deploy-123"));
}

// ───────────────────────── Deploy ─────────────────────────

#[test]
fn test_deploy_new_app_json() {
    let mut server = Server::new();
    let home = setup_config(&server);

    // Create a temp project with package.json for detection and service config
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();
    write_service_config(&project, "test-deploy");

    // Config-resolved deploy: resolve_app (name lookup) returns 404, then creates app
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
        .args(["--json", "deploy", project.path().to_str().unwrap()])
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

    // Create a temp project with package.json for detection + service config
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();
    write_service_config(&project, TEST_APP_NAME);

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

#[test]
fn test_deploy_fails_with_invalid_response_when_app_id_missing() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();
    write_service_config(&project, "test-deploy");

    // resolve_app returns 404 (name not found), so deploy creates app
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

    // create_app returns response without "id" field
    let _m_create_app = server
        .mock("POST", "/v1/apps")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"name":"test-deploy","status":"created","runtime":"nodejs"}"#)
        .create();

    floo()
        .args(["--json", "deploy", project.path().to_str().unwrap()])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"PARSE_ERROR""#))
        .stdout(predicate::str::contains("Failed to parse response"));
}

#[test]
fn test_deploy_failed_json_includes_logs_and_suggestion() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();
    write_service_config(&project, TEST_APP_NAME);

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

    let _m_deploy = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/deploys").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"deploy-001","status":"failed","url":"https://my-app.floo.app","build_logs":"build failed: npm ci exited 1"}"#,
        )
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
        .failure()
        .stdout(predicate::str::contains(r#""code":"DEPLOY_FAILED""#))
        .stdout(predicate::str::contains(
            r#""suggestion":"Check build output above, or run `floo logs` for details.""#,
        ))
        .stdout(predicate::str::contains(
            r#""build_logs":"build failed: npm ci exited 1""#,
        ));
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

// ───────────────────────── Deploy SSE streaming ─────────────────────────

#[test]
fn test_deploy_with_sse_streaming() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();
    write_service_config(&project, "test-stream");

    // resolve_app: name lookup returns empty, so app gets created
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

    let _m_create_app = server
        .mock("POST", "/v1/apps")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"id":"{TEST_APP_ID}","name":"test-stream","status":"created","runtime":"nodejs"}}"#
        ))
        .create();

    // Deploy returns pending (Phase 2 — not terminal)
    let _m_deploy = server
        .mock("POST", format!("/v1/apps/{TEST_APP_ID}/deploys").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id":"deploy-001","status":"pending","url":null,"build_logs":""}"#)
        .create();

    // SSE stream endpoint
    let sse_body = concat!(
        "event: status\ndata: {\"status\": \"building\"}\n\n",
        "event: log\ndata: {\"text\": \"Building image...\\nPushing to registry...\"}\n\n",
        "event: status\ndata: {\"status\": \"deploying\"}\n\n",
        "event: done\ndata: {\"status\": \"live\", \"url\": \"https://test-stream.floo.app\"}\n\n",
    );

    let _m_stream = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/deploys/deploy-001/logs/stream").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create();

    // Final deploy state fetched after stream ends
    let _m_get_deploy = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/deploys/deploy-001").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"deploy-001","status":"live","url":"https://test-stream.floo.app","build_logs":"Building image...\nPushing to registry..."}"#,
        )
        .create();

    floo()
        .args(["deploy", project.path().to_str().unwrap()])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Building"))
        .stderr(predicate::str::contains("https://test-stream.floo.app"));
}

#[test]
fn test_deploy_sse_fallback_to_polling() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();
    write_service_config(&project, "test-fallback");

    // resolve_app: name lookup returns empty, so app gets created
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

    let _m_create_app = server
        .mock("POST", "/v1/apps")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"id":"{TEST_APP_ID}","name":"test-fallback","status":"created","runtime":"nodejs"}}"#
        ))
        .create();

    // Deploy returns pending (Phase 2)
    let _m_deploy = server
        .mock("POST", format!("/v1/apps/{TEST_APP_ID}/deploys").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id":"deploy-002","status":"pending","url":null,"build_logs":""}"#)
        .create();

    // SSE endpoint returns 404 — forces fallback to polling
    let _m_stream = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/deploys/deploy-002/logs/stream").as_str(),
        )
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"NOT_FOUND","message":"Not found"}}"#)
        .create();

    // Polling endpoint returns live immediately
    let _m_get_deploy = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/deploys/deploy-002").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"deploy-002","status":"live","url":"https://test-fallback.floo.app","build_logs":"done"}"#,
        )
        .create();

    floo()
        .args(["deploy", project.path().to_str().unwrap()])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("https://test-fallback.floo.app"));
}

#[test]
fn test_deploy_json_mode_uses_polling() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();
    write_service_config(&project, "test-poll");

    // resolve_app returns 404, then creates app
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

    let _m_create_app = server
        .mock("POST", "/v1/apps")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"id":"{TEST_APP_ID}","name":"test-poll","status":"created","runtime":"nodejs"}}"#
        ))
        .create();

    // Deploy returns pending
    let _m_deploy = server
        .mock("POST", format!("/v1/apps/{TEST_APP_ID}/deploys").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id":"deploy-003","status":"pending","url":null,"build_logs":""}"#)
        .create();

    // SSE stream endpoint for JSON mode
    let sse_body = concat!(
        "event: status\ndata: {\"status\": \"building\"}\n\n",
        "event: done\ndata: {\"status\": \"live\", \"url\": \"https://test-poll.floo.app\"}\n\n",
    );

    let _m_stream = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/deploys/deploy-003/logs/stream").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create();

    // Final deploy state fetched after stream ends
    let _m_get_deploy = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/deploys/deploy-003").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"deploy-003","status":"live","url":"https://test-poll.floo.app","build_logs":"done"}"#,
        )
        .create();

    floo()
        .args(["--json", "deploy", project.path().to_str().unwrap()])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("https://test-poll.floo.app"));
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
