use assert_cmd::Command;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use mockito::{Matcher, Mock, Server};
use predicates::prelude::*;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const TEST_APP_ID: &str = "app-uuid-1234";
const TEST_APP_NAME: &str = "my-app";
const TEST_ORG_ID: &str = "org-uuid-5678";
const UPDATE_SUCCESS_SIGNATURE_B64: &str = "J6XvKzQVWSMxsnLl4sAkqMhCXAtDI7AZ/ckGf7BeD+KSiM2YtCjQlsV7cwpNypzRyCd9HU87U5sGeuG4NiJ4TqMHpLdoljLuhR9zXuyDyGgqasRSvqawmbpkrs+YsDaD7scj80uA/eUOKH0XmWu6yJA15gUXq97H+XJSXI1aciDN5jeOdqaMAgZfYhIvINzxi2O59iTnv4+EdFeBT4MHWdO5WknsAhk13kQYMLoUbbXBQmWjGTrLlJhLiNcfsub8yJvJG347It2To4/Bz6oUrfNyS8jAmxUhYoJjn+8dOOd8x8rmzHyCa7sCuUgk3UThTEzkKhHjhAieMVatm/+L8W3bE3LKIKYwQlclNmAdXcHlUmwXLxJnVBbJdf1juqqsPh8Nz0/0BqSu5WmxA4/w/WoRCXfegsiM8fUeacbpx44DjLDhye8zkc/LnXrqkL5u+x5TOwlb+GRu5x+BvyocXp4xBF8Yw/kExWk54fSPSNUkGZSFACqSSHY9CkBtqQy1";

fn floo() -> Command {
    Command::cargo_bin("floo-local").unwrap()
}

/// Create temp HOME with config pointing at mock server.
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

fn update_success_signature() -> Vec<u8> {
    BASE64_STANDARD
        .decode(UPDATE_SUCCESS_SIGNATURE_B64)
        .expect("test update signature decodes")
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
fn test_apps_status_json_surfaces_runtime_url() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let runtime_url = "https://floo-my-app-dev-web-l3txcgkazq-uc.a.run.app";

    let _m = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"apps":[{{"id":"{TEST_APP_ID}","name":"{TEST_APP_NAME}","org_id":"{TEST_ORG_ID}","status":"live","url":"https://test.floo.app","runtime":"rails","runtime_url":"{runtime_url}","created_at":"2024-01-01T00:00:00Z"}}]}}"#
        ))
        .create();

    floo()
        .args(["--json", "apps", "status", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""runtime_url""#))
        .stdout(predicate::str::contains(runtime_url));
}

#[test]
fn test_apps_status_human_includes_runtime_url() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let runtime_url = "https://floo-my-app-dev-web-l3txcgkazq-uc.a.run.app";

    let _m = server
        .mock("GET", "/v1/apps")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"apps":[{{"id":"{TEST_APP_ID}","name":"{TEST_APP_NAME}","org_id":"{TEST_ORG_ID}","status":"live","url":"https://test.floo.app","runtime":"rails","runtime_url":"{runtime_url}","created_at":"2024-01-01T00:00:00Z"}}]}}"#
        ))
        .create();
    let _m_org = mock_org_me(&mut server);

    floo()
        .args(["apps", "status", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Runtime URL:"))
        .stderr(predicate::str::contains(runtime_url))
        .stderr(predicate::str::contains("debug only"));
}

#[test]
fn test_apps_delete_requires_explicit_confirmation_in_json_mode() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    // No DELETE mock — the command must refuse before reaching that endpoint.
    floo()
        .args(["--json", "apps", "delete", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("CONFIRMATION_REQUIRED"))
        .stdout(predicate::str::contains("yes-i-know-this-destroys-data"));
}

#[test]
fn test_apps_delete_json_with_explicit_flag() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_delete = server
        .mock("DELETE", format!("/v1/apps/{TEST_APP_ID}").as_str())
        .with_status(204)
        .create();

    floo()
        .args([
            "--json",
            "apps",
            "delete",
            TEST_APP_NAME,
            "--yes-i-know-this-destroys-data",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(TEST_APP_ID))
        .stdout(predicate::str::contains(r#""destructive":true"#))
        .stdout(predicate::str::contains(r#""data_loss":true"#))
        .stdout(predicate::str::contains(r#""tier":3"#));
}

#[test]
fn test_apps_delete_force_alias_still_works() {
    // `--force` was the legacy flag. Keep as alias so existing scripts don't
    // break. New code should reach for --yes-i-know-this-destroys-data.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_delete = server
        .mock("DELETE", format!("/v1/apps/{TEST_APP_ID}").as_str())
        .with_status(204)
        .create();

    floo()
        .args(["--json", "apps", "delete", TEST_APP_NAME, "--force"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#));
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

    // The CLI sends `POST /v1/apps/{id}/env?env=dev`. Mockito's default
    // when `.match_query` is omitted is to require an empty query string
    // (not "match any query"), so we must explicitly match the `env`
    // param or the mock silently fails to register and the server
    // returns its fallback 501 — which the CLI surfaces as API_ERROR
    // with empty message. That's how these four tests drifted into
    // green-by-orphan: nothing runs ci.yml on push so nobody noticed.
    let _m_set = server
        .mock("POST", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .match_query(Matcher::UrlEncoded("env".into(), "dev".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "id":"00000000-0000-0000-0000-000000000001",
                "app_id":"00000000-0000-0000-0000-0000000000aa",
                "environment_id":"00000000-0000-0000-0000-0000000000ee",
                "service_id":null,
                "key":"MY_KEY",
                "masked_value":"my_v****",
                "created_at":"2026-04-24T00:00:00Z",
                "updated_at":"2026-04-24T00:00:00Z"
            }"#,
        )
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

    // CLI sends `POST /v1/apps/{id}/env/import?env=dev` — must match
    // the env query param explicitly (see test_env_set_json comment).
    let _m_import = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/env/import").as_str(),
        )
        .match_query(Matcher::UrlEncoded("env".into(), "dev".into()))
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
fn test_domains_remove_refuses_without_yes_flag_in_json_mode() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_services_single(&mut server);

    // No DELETE mock — command must refuse before reaching that endpoint.
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
        .failure()
        .stdout(predicate::str::contains("CONFIRMATION_REQUIRED"));
}

#[test]
fn test_domains_remove_json_with_yes_flag() {
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
            "--yes",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("app.example.com"))
        .stdout(predicate::str::contains(r#""destructive":true"#))
        .stdout(predicate::str::contains(r#""data_loss":false"#))
        .stdout(predicate::str::contains(r#""tier":2"#));
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

#[test]
fn test_domains_status_json() {
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
            r#"{"domains":[{"hostname":"app.example.com","status":"pending","dns_instructions":"CNAME app.example.com -> test.getfloo.com","service_name":"web","ssl_status":"pending","verified":false,"created_at":"2024-01-01T00:00:00Z"}]}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "domains",
            "status",
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
fn test_domains_status_not_found() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/domains").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"domains":[]}"#)
        .create();

    floo()
        .args([
            "--json",
            "domains",
            "status",
            "missing.example.com",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("DOMAIN_NOT_FOUND"));
}

#[test]
fn test_domains_watch_becomes_active() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_verify = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/domains/app.example.com/verify").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"hostname":"app.example.com","status":"active","dns_instructions":null}"#)
        .create();

    floo()
        .args([
            "--json",
            "domains",
            "watch",
            "app.example.com",
            "--app",
            TEST_APP_NAME,
            "--timeout",
            "60",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("active"));
}

#[test]
fn test_domains_watch_fails_on_failed_status() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_verify = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/domains/app.example.com/verify").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"hostname":"app.example.com","status":"failed","dns_instructions":null}"#)
        .create();

    floo()
        .args([
            "--json",
            "domains",
            "watch",
            "app.example.com",
            "--app",
            TEST_APP_NAME,
            "--timeout",
            "60",
        ])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("DOMAIN_VERIFICATION_FAILED"));
}

#[test]
fn test_domains_watch_timeout() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    // Always returns pending — watch should time out.
    let _m_verify = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/domains/app.example.com/verify").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"hostname":"app.example.com","status":"pending","dns_instructions":"CNAME app.example.com -> test.getfloo.com"}"#)
        .create();

    // --timeout 0 means the deadline is already expired after the first verify call.
    floo()
        .args([
            "--json",
            "domains",
            "watch",
            "app.example.com",
            "--app",
            TEST_APP_NAME,
            "--timeout",
            "0",
        ])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("DOMAIN_WATCH_TIMEOUT"));
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
        .args(["--json", "deploys", "list", "--app", TEST_APP_NAME])
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

    floo()
        .args([
            "--json",
            "deploys",
            "rollback",
            TEST_APP_NAME,
            "deploy-456",
            "--yes",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("deploy"))
        .stdout(predicate::str::contains(r#""destructive":true"#))
        .stdout(predicate::str::contains(r#""data_loss":false"#))
        .stdout(predicate::str::contains(r#""tier":2"#));
}

#[test]
fn test_deploy_rollback_refuses_without_yes_in_json_mode() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    // No rollback mock — the command must refuse before reaching that endpoint.

    floo()
        .args(["--json", "deploys", "rollback", TEST_APP_NAME, "deploy-456"])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("CONFIRMATION_REQUIRED"));
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
    let server = Server::new();
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
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "100".into()),
            Matcher::UrlEncoded("search".into(), "error".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:01Z","severity":"ERROR","message":"Connection error occurred","service_name":"web"}],"app_name":"my-app"}"#,
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
fn test_services_list_json_returns_both_app_and_managed_services() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_services_single(&mut server);

    let _m_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"managed_services":[{"id":"ms-1","app_id":"app-uuid-1234","type":"postgres","name":"default","status":"ready","env_var_keys":["DATABASE_URL"],"created_at":null,"updated_at":null}],"total":1}"#,
        )
        .create();

    floo()
        .args(["--json", "services", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("app_services"))
        .stdout(predicate::str::contains("managed_services"))
        .stdout(predicate::str::contains("web"))
        .stdout(predicate::str::contains("postgres"));
}

#[test]
fn test_services_list_partial_view_when_managed_services_fetch_fails() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_services_single(&mut server);

    // Managed services endpoint is broken. `list` must degrade gracefully:
    // app services render, managed_services is empty, a warning fires.
    let _m_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"INTERNAL_ERROR","message":"boom"}}"#)
        .create();

    floo()
        .args(["--json", "services", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("web"))
        .stdout(predicate::str::contains(r#""managed_services":[]"#));
}

#[test]
fn test_services_list_empty_when_no_services_of_either_kind() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_app = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/services").as_str())
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
            Matcher::UrlEncoded("environment".into(), "dev".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"services":[]}"#)
        .create();

    let _m_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"managed_services":[],"total":0}"#)
        .create();

    floo()
        .args(["--json", "services", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""app_services":[]"#))
        .stdout(predicate::str::contains(r#""managed_services":[]"#));
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
fn test_services_info_routes_to_managed_service_by_type() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_app_services = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/services").as_str())
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"services":[]}"#)
        .create();

    let _m_list_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"managed_services":[{"id":"ms-1","app_id":"1","type":"postgres","name":"default","status":"ready","env_var_keys":["DATABASE_URL"],"created_at":"2026-04-24T00:00:00Z","updated_at":"2026-04-24T00:00:00Z"}],"total":1}"#,
        )
        .create();

    let _m_get_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services/ms-1").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"ms-1","app_id":"1","type":"postgres","name":"default","status":"ready","env_var_keys":["DATABASE_URL"],"credentials":{"DATABASE_URL":"postgresql://redacted"},"created_at":"2026-04-24T00:00:00Z","updated_at":"2026-04-24T00:00:00Z"}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "services",
            "info",
            "postgres",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""type":"postgres""#))
        // Credentials must NEVER appear in CLI output — the CLI struct deliberately
        // skips the credentials field so plaintext secrets can't leak into logs.
        .stdout(predicate::str::contains("redacted").not());
}

#[test]
fn test_services_info_nothing_matches_lists_available() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_app_services = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/services").as_str())
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"services":[{"id":"svc-1","name":"web","type":"web","status":"ready","ingress":"public","cloud_run_url":"https://web.example","port":8080}]}"#)
        .create();

    let _m_list_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"managed_services":[{"id":"ms-1","app_id":"1","type":"redis","name":"default","status":"ready","env_var_keys":["REDIS_URL"],"created_at":null,"updated_at":null}],"total":1}"#)
        .create();

    floo()
        .args(["--json", "services", "info", "nope", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("SERVICE_NOT_FOUND"))
        .stdout(predicate::str::contains("web"))
        .stdout(predicate::str::contains("redis"));
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
        .args([
            "--json",
            "services",
            "info",
            "postgres",
            "--app",
            TEST_APP_NAME,
        ])
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
        .args(["--json", "deploys", "list"])
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
        .args(["--json", "redeploy", project.path().to_str().unwrap()])
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

    // `floo redeploy --app X` (without --rebuild) goes through
    // deploy_restart → POST /v1/apps/{id}/restart, NOT the old /deploys
    // endpoint. The rebuild path (--rebuild flag) is what POSTs to
    // /deploys via rebuild_app — see test_deploy_rebuild_json below
    // if/when we add one.
    let _m_restart = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/restart").as_str(),
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
            "redeploy",
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
        .args(["--json", "redeploy", project.path().to_str().unwrap()])
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

    // Same /restart routing as test_deploy_existing_app_by_name_json —
    // `floo redeploy --app X` (without --rebuild) calls deploy_restart,
    // which on status=="failed" emits RESTART_FAILED (not DEPLOY_FAILED)
    // via output::error_with_data. The data payload contains
    // {app, deploy}, and deploy carries through the build_logs field
    // from the API response — so the existing build_logs assertion
    // still works because the whole deploy object gets serialized.
    let _m_restart = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/restart").as_str(),
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
            "redeploy",
            project.path().to_str().unwrap(),
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"RESTART_FAILED""#))
        .stdout(predicate::str::contains(
            r#""suggestion":"Run `floo logs` for details.""#,
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
        .args(["redeploy", project.path().to_str().unwrap()])
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
        .args(["redeploy", project.path().to_str().unwrap()])
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
        .args(["--json", "redeploy", project.path().to_str().unwrap()])
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
                    },
                    {
                        "name": format!("{asset_name}.sig"),
                        "browser_download_url": format!("{}/downloads/{}.sig", server.url(), asset_name),
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

    let signature = update_success_signature();
    let _m_signature = server
        .mock("GET", format!("/downloads/{asset_name}.sig").as_str())
        .with_status(200)
        .with_body(signature.as_slice())
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

#[test]
fn test_preflight_surfaces_orphaned_managed_service() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_preflight = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/preflight").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "managed_services":{
                    "to_provision":[],
                    "to_retain":[],
                    "to_orphan":[{"type":"postgres","name":"default","tier":"basic","managed_service_id":"ms-1","data_impact":"Postgres schema app_xyz and role floo_app_xyz"}],
                    "in_flight_deprovisioning":[]
                },
                "summary":{"action_count":1,"destructive_count":1,"estimated_duration_seconds":null},
                "destructive":true,
                "data_loss":false
            }"#,
        )
        .create();

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        format!("[app]\nname = \"{TEST_APP_NAME}\"\n"),
    )
    .unwrap();
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

    floo()
        .args(["--json", "preflight", project.path().to_str().unwrap()])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""valid":true"#))
        .stdout(predicate::str::contains(r#""to_orphan""#))
        .stdout(predicate::str::contains("app_xyz"))
        .stdout(predicate::str::contains(r#""destructive":true"#));
}

#[test]
fn test_preflight_degrades_gracefully_when_app_not_resolvable() {
    // Unauthenticated-style: local-only config, no mock endpoints wired.
    // The plan fetch is best-effort; local validation still ships.
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        "[app]\nname = \"some-app\"\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("floo.service.toml"),
        r#"[app]
name = "some-app"

[service]
name = "web"
type = "web"
port = 3000
ingress = "public"
"#,
    )
    .unwrap();

    floo()
        .args(["--json", "preflight", project.path().to_str().unwrap()])
        .env("HOME", "/tmp/floo-test-nonexistent-preflight")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""valid":true"#))
        .stdout(predicate::str::contains(r#""plan":null"#));
}

#[test]
fn test_services_add_provisions_and_writes_lock_file() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_create = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "id":"ms-new",
                "app_id":"app-uuid-1234",
                "type":"postgres",
                "name":"default",
                "status":"ready",
                "env_var_keys":["DATABASE_URL"],
                "credentials":{"DATABASE_URL":"postgresql://redacted"},
                "created_at":"2026-04-24T00:00:00Z",
                "updated_at":"2026-04-24T00:00:00Z"
            }"#,
        )
        .create();

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        format!("[app]\nname = \"{TEST_APP_NAME}\"\n"),
    )
    .unwrap();

    floo()
        .args([
            "--json",
            "services",
            "add",
            "postgres",
            "--app",
            TEST_APP_NAME,
            "--tier",
            "basic",
        ])
        .env("HOME", home.path())
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""type":"postgres""#))
        // Credentials must never leak into stdout even though the API returned them.
        .stdout(predicate::str::contains("redacted").not());

    let lock = std::fs::read_to_string(project.path().join(".floo").join("services.lock")).unwrap();
    assert!(lock.contains(r#""type": "postgres""#));
    assert!(lock.contains(r#""status": "ready""#));
}

#[test]
fn test_services_remove_refuses_without_confirmation_flag_in_json_mode() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"managed_services":[{"id":"ms-1","app_id":"app-uuid-1234","type":"postgres","name":"default","status":"ready","env_var_keys":["DATABASE_URL"],"created_at":null,"updated_at":null}],"total":1}"#,
        )
        .create();

    // No mock for DELETE — the command must refuse before reaching that endpoint.
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        format!("[app]\nname = \"{TEST_APP_NAME}\"\n"),
    )
    .unwrap();

    floo()
        .args([
            "--json",
            "services",
            "remove",
            "postgres",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .current_dir(project.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("CONFIRMATION_REQUIRED"))
        .stdout(predicate::str::contains("yes-i-know-this-destroys-data"));
}

#[test]
fn test_services_remove_with_explicit_flag_destroys_and_updates_lock() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"managed_services":[{"id":"ms-1","app_id":"app-uuid-1234","type":"postgres","name":"default","status":"ready","env_var_keys":["DATABASE_URL"],"created_at":null,"updated_at":null}],"total":1}"#,
        )
        .create();

    let _m_delete = server
        .mock(
            "DELETE",
            format!("/v1/apps/{TEST_APP_ID}/managed-services/ms-1").as_str(),
        )
        .with_status(204)
        .create();

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        format!("[app]\nname = \"{TEST_APP_NAME}\"\n"),
    )
    .unwrap();
    // Pre-existing lock file with the row we're about to remove.
    std::fs::create_dir_all(project.path().join(".floo")).unwrap();
    std::fs::write(
        project.path().join(".floo").join("services.lock"),
        r#"{"version":1,"managed_services":[{"type":"postgres","name":"default","status":"ready","created_at":null}]}
"#,
    )
    .unwrap();

    floo()
        .args([
            "--json",
            "services",
            "remove",
            "postgres",
            "--app",
            TEST_APP_NAME,
            "--yes-i-know-this-destroys-data",
        ])
        .env("HOME", home.path())
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""destructive":true"#))
        .stdout(predicate::str::contains(r#""data_loss":true"#))
        .stdout(predicate::str::contains(r#""tier":3"#));

    let lock = std::fs::read_to_string(project.path().join(".floo").join("services.lock")).unwrap();
    assert!(
        !lock.contains(r#""postgres""#),
        "lock file should no longer have the postgres entry, got: {lock}"
    );
}

#[test]
fn test_services_remove_not_found_surfaces_clear_error() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"managed_services":[],"total":0}"#)
        .create();

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        format!("[app]\nname = \"{TEST_APP_NAME}\"\n"),
    )
    .unwrap();

    floo()
        .args([
            "--json",
            "services",
            "remove",
            "postgres",
            "--app",
            TEST_APP_NAME,
            "--yes-i-know-this-destroys-data",
        ])
        .env("HOME", home.path())
        .current_dir(project.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("MANAGED_SERVICE_NOT_FOUND"));
}

#[test]
fn test_services_migrate_upserts_each_declared_section_and_writes_lock() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    // POST is idempotent on (app_id, type, name) — the API returns the existing
    // row if one exists, so migrate is a "read-and-record" operation, not a
    // "create-new" operation. Both of our declared sections get a 201.
    let _m_postgres = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .match_body(mockito::Matcher::JsonString(
            r#"{"type":"postgres","name":"default","tier":"basic"}"#.to_string(),
        ))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "id":"ms-pg",
                "app_id":"app-uuid-1234",
                "type":"postgres",
                "name":"default",
                "status":"ready",
                "env_var_keys":["DATABASE_URL"],
                "created_at":"2026-04-24T00:00:00Z",
                "updated_at":"2026-04-24T00:00:00Z"
            }"#,
        )
        .create();

    let _m_redis = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .match_body(mockito::Matcher::JsonString(
            r#"{"type":"redis","name":"default","tier":"basic"}"#.to_string(),
        ))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "id":"ms-rd",
                "app_id":"app-uuid-1234",
                "type":"redis",
                "name":"default",
                "status":"ready",
                "env_var_keys":["REDIS_URL"],
                "created_at":"2026-04-24T00:00:00Z",
                "updated_at":"2026-04-24T00:00:00Z"
            }"#,
        )
        .create();

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        format!(
            r#"[app]
name = "{TEST_APP_NAME}"

[postgres]
tier = "basic"

[redis]
tier = "basic"
"#
        ),
    )
    .unwrap();

    floo()
        .args([
            "--json",
            "services",
            "migrate",
            "--app",
            TEST_APP_NAME,
            project.path().to_str().unwrap(),
        ])
        .env("HOME", home.path())
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""type":"postgres""#))
        .stdout(predicate::str::contains(r#""type":"redis""#))
        .stdout(predicate::str::contains("next_steps"));

    // Lock file now reflects both migrated services.
    let lock = std::fs::read_to_string(project.path().join(".floo").join("services.lock")).unwrap();
    assert!(
        lock.contains(r#""type": "postgres""#),
        "lock missing postgres: {lock}"
    );
    assert!(
        lock.contains(r#""type": "redis""#),
        "lock missing redis: {lock}"
    );
}

#[test]
fn test_services_migrate_no_op_when_no_legacy_sections() {
    let server = Server::new();
    let home = setup_config(&server);

    // No mocks — migrate must short-circuit before any API call when there
    // are no legacy sections to migrate.
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        format!("[app]\nname = \"{TEST_APP_NAME}\"\n"),
    )
    .unwrap();

    floo()
        .args([
            "--json",
            "services",
            "migrate",
            "--app",
            TEST_APP_NAME,
            project.path().to_str().unwrap(),
        ])
        .env("HOME", home.path())
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("migrated"));
}
