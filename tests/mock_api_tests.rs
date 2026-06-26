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

#[allow(deprecated)]
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

fn preview_branch_json(name: &str, status: &str, reset_eligible: bool) -> String {
    let blocked = if reset_eligible {
        "null".to_string()
    } else {
        r#""latest deploy is building""#.to_string()
    };
    format!(
        r#"{{
            "id":"resource-{name}",
            "managed_service_id":"ms-{name}",
            "name":"{name}",
            "source_environment":"dev",
            "preview_slug":"feat-db-abcde",
            "resource_status":"{status}",
            "hydration_mode":"clone-dev",
            "schema_name":"app_default_preview_feat_db_abcde",
            "role_name":"floo_app_default_preview_feat_db_abcde",
            "base_schema_name":"app_default_dev",
            "base_role_name":"floo_app_default_dev",
            "created_at":"2026-06-24T10:00:00Z",
            "updated_at":"2026-06-24T10:01:00Z",
            "expires_at":"2026-06-27T10:00:00Z",
            "reset_eligible":{reset_eligible},
            "reset_blocked_reason":{blocked}
        }}"#
    )
}

fn preview_detail_json(branches: &str) -> String {
    format!(
        r#"{{
            "id":"env-preview-1",
            "app_id":"{TEST_APP_ID}",
            "slug":"feat-db-abcde",
            "source_branch":"feat/db-branch",
            "pr_number":42,
            "url":"https://my-app-preview-feat-db-abcde.on.getfloo.com",
            "latest_deploy_id":"deploy-preview-1",
            "latest_deploy_status":"live",
            "latest_commit_sha":"abc123",
            "ttl_hours":72,
            "expires_at":"2026-06-27T10:00:00Z",
            "resources":[],
            "database_branches":[{branches}]
        }}"#
    )
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
fn test_billing_spend_cap_get_json_uses_cents_key() {
    // #1161: `spend-cap get` emits the cap as `spend_cap_cents`, consistent
    // with `billing usage` and every other *_cents field. The bare `spend_cap`
    // key that drifted between the two commands is gone.
    let mut server = Server::new();
    let home = setup_config(&server);

    let _m = server
        .mock("GET", "/v1/orgs/me")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"id":"{TEST_ORG_ID}","name":"Test Org","slug":"test-org","spend_cap":5000,"current_period_spend_cents":1200,"spend_cap_exceeded":false}}"#
        ))
        .create();

    floo()
        .args(["--json", "billing", "spend-cap", "get"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""spend_cap_cents":5000"#))
        .stdout(predicate::str::contains(
            r#""current_period_spend_cents":1200"#,
        ))
        // The drifted bare key (`"spend_cap":`) must not reappear.
        .stdout(predicate::str::contains(r#""spend_cap":"#).not());
}

#[test]
fn test_billing_usage_period_scopes_derived_fields() {
    // #1161: `--period` must scope every period-derived field. The org's
    // always-current-month columns (current_period_spend_cents=9000,
    // spend_cap_exceeded=false) must NOT leak into a `--period last_month`
    // view — that view's spend and exceeded flag come from the period-scoped
    // breakdown (total_cost_usd=120.0 -> 12000c, over the 10000c cap).
    let mut server = Server::new();
    let home = setup_config(&server);

    let _org = server
        .mock("GET", "/v1/orgs/me")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"id":"{TEST_ORG_ID}","name":"Test Org","slug":"test-org","plan":"pro","spend_cap":10000,"current_period_spend_cents":9000,"spend_cap_exceeded":false}}"#
        ))
        .create();

    let _limits = server
        .mock("GET", "/v1/billing/limits")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"plan":"pro","max_spend_cap_cents":20000}"#)
        .create();

    let _breakdown = server
        .mock("GET", "/v1/billing/orgs/me/cost-breakdown")
        .match_query(Matcher::UrlEncoded("period".into(), "last_month".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"period":{"start":"2026-05-01T00:00:00Z","end":"2026-06-01T00:00:00Z","label":"Last month"},"total_cost_usd":120.0,"included_cost_usd":5.0,"apps":[]}"#,
        )
        .create();

    floo()
        .args(["--json", "billing", "usage", "--period", "last_month"])
        .env("HOME", home.path())
        .assert()
        .success()
        // Period-scoped spend from the breakdown (12000c), not the org's 9000c.
        .stdout(predicate::str::contains(r#""period_spend_cents":12000"#))
        // Period-scoped exceeded (12000 >= 10000 cap), not the org's current false.
        .stdout(predicate::str::contains(r#""spend_cap_exceeded":true"#))
        // The stale current-month key is gone from `usage`.
        .stdout(predicate::str::contains("current_period_spend_cents").not());
}

#[test]
fn test_orgs_invite_json_assigns_role_and_redacts_url() {
    // #1161: `orgs invite` resolves the current org, POSTs email + role in one
    // step, and returns a one-time invite_url that is secret-shaped (redacted
    // in JSON, never printed raw).
    let mut server = Server::new();
    let home = setup_config(&server);
    let _org = mock_org_me(&mut server);

    let _invite = server
        .mock("POST", format!("/v1/orgs/{TEST_ORG_ID}/invites").as_str())
        .match_body(Matcher::PartialJson(serde_json::json!({
            "email": "alice@example.com",
            "role": "admin",
        })))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"inv-1","org_id":"org-uuid-5678","email":"alice@example.com","role":"admin","status":"pending","invited_by_id":"u-1","expires_at":"2026-07-01T00:00:00Z","created_at":"2026-06-26T00:00:00Z","invite_url":"https://app.getfloo.com/invite/secret-token-xyz"}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "orgs",
            "invite",
            "alice@example.com",
            "--role",
            "admin",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""role":"admin""#))
        // The one-time invite_url is secret-shaped: redacted, never raw.
        .stdout(predicate::str::contains("secret-token-xyz").not())
        .stdout(predicate::str::contains("contains_secrets"));
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
fn test_apps_show_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    floo()
        .args(["--json", "apps", "show", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(TEST_APP_ID));
}

#[test]
fn test_apps_show_with_app_flag() {
    // Parity with the rest of the CLI's flag-based API: `--app` is
    // accepted in addition to the legacy positional form. Closes
    // feedback 4419e7d3 (2026-05-01) for the `apps show` surface.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    floo()
        .args(["--json", "apps", "show", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(TEST_APP_ID));
}

#[test]
fn test_apps_show_json_surfaces_runtime_url() {
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
        .args(["--json", "apps", "show", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""runtime_url""#))
        .stdout(predicate::str::contains(runtime_url));
}

#[test]
fn test_apps_show_human_includes_runtime_url() {
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
        .args(["apps", "show", TEST_APP_NAME])
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

fn set_env_response_body() -> &'static str {
    r#"{
        "id":"00000000-0000-0000-0000-000000000001",
        "app_id":"00000000-0000-0000-0000-0000000000aa",
        "environment_id":"00000000-0000-0000-0000-0000000000ee",
        "service_id":null,
        "key":"API_KEY",
        "masked_value":"********",
        "created_at":"2026-04-24T00:00:00Z",
        "updated_at":"2026-04-24T00:00:00Z"
    }"#
}

#[test]
fn test_env_set_stdin() {
    // #1152: a secret value piped on stdin keeps it out of argv / shell history.
    // The trailing newline from `echo` is stripped before it reaches the API.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

    let _m_set = server
        .mock("POST", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .match_query(Matcher::UrlEncoded("env".into(), "dev".into()))
        .match_body(Matcher::Regex(r#""value":"supersecret""#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(set_env_response_body())
        .create();

    floo()
        .args([
            "--json",
            "env",
            "set",
            "API_KEY",
            "--stdin",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .write_stdin("supersecret\n")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#));
}

#[test]
fn test_env_set_value_file() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

    let dir = TempDir::new().unwrap();
    let secret_path = dir.path().join("secret.txt");
    std::fs::write(&secret_path, "filesecret\n").unwrap();

    let _m_set = server
        .mock("POST", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .match_query(Matcher::UrlEncoded("env".into(), "dev".into()))
        .match_body(Matcher::Regex(r#""value":"filesecret""#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(set_env_response_body())
        .create();

    floo()
        .args([
            "--json",
            "env",
            "set",
            "API_KEY",
            "--value-file",
            secret_path.to_str().unwrap(),
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#));
}

#[test]
fn test_env_set_stdin_rejects_inline_value() {
    // KEY=VALUE alongside --stdin is ambiguous (value given two ways) — refuse.
    let server = Server::new();
    let home = setup_config(&server);

    floo()
        .args([
            "env",
            "set",
            "API_KEY=inline",
            "--stdin",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .write_stdin("piped")
        .assert()
        .failure()
        .stderr(predicate::str::contains("key only"));
}

#[test]
fn test_env_set_dry_run_stdin_does_not_read() {
    // Dry-run mutates nothing and must not consume stdin; it previews the key
    // and names the value source. No POST mock is registered, so a real call
    // would surface as an error — the test passing proves none was made.
    let server = Server::new();
    let home = setup_config(&server);

    floo()
        .args([
            "env",
            "set",
            "API_KEY",
            "--stdin",
            "--dry-run",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("value from stdin"));
}

#[test]
fn test_env_set_stdin_and_value_file_mutually_exclusive() {
    // The value must come from exactly one source. clap's reciprocal
    // conflicts_with rejects --stdin together with --value-file at parse time;
    // this pins that wiring against accidental removal.
    let server = Server::new();
    let home = setup_config(&server);

    floo()
        .args([
            "env",
            "set",
            "API_KEY",
            "--stdin",
            "--value-file",
            "/tmp/whatever",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .write_stdin("x")
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn test_env_list_json() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

    // With no `--services`, `env list` reads all vars (service_id omitted).
    // For a single-service app the API returns that service's vars plus
    // app-level; the CLI keeps the plain Key/Value table (no Service column).
    let _m_list = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .match_query(Matcher::UrlEncoded("env".into(), "dev".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"env_vars":[{"key":"DATABASE_URL","masked_value":"********","service_id":null}]}"#,
        )
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
fn test_env_unset_json() {
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
        .args(["--json", "env", "unset", "my_key", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("MY_KEY"));
}

#[test]
fn test_env_get_json() {
    // PR #138 made `--json` redact secret-shaped values by default and
    // stamp the payload with `contains_secrets: true`. `floo env get` is
    // the canonical secret-fetch surface, so the redacted shape is what
    // agents will see; revealing the plaintext now requires the explicit
    // global flag (covered by `test_env_get_json_reveal_secrets` below).
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
        .stdout(predicate::str::contains(r#""contains_secrets":true"#))
        .stdout(predicate::str::contains(r#""value":"***REDACTED***""#))
        .stdout(predicate::str::contains("secret123").not());
}

#[test]
fn test_env_get_json_reveal_secrets() {
    // `--reveal-secrets` opts back in to plaintext for callers that
    // control where the JSON goes. The `contains_secrets` marker still
    // fires so harnesses can detect-and-refuse even under reveal.
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
        .args([
            "--json",
            "--reveal-secrets",
            "env",
            "get",
            "MY_KEY",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""contains_secrets":true"#))
        .stdout(predicate::str::contains("secret123"));
}

#[test]
fn test_env_get_human_nonsecret_raw_stdout() {
    // A non-secret key (LOG_LEVEL is on the redactor allowlist; "debug" is not
    // credential-shaped) prints its plaintext to stdout in human mode, with no
    // --reveal-secrets needed — `env get` stays convenient for plain config.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

    let _m_get = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/env/LOG_LEVEL").as_str(),
        )
        .match_query(Matcher::UrlEncoded(
            "service_id".into(),
            TEST_SERVICE_ID.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"key":"LOG_LEVEL","value":"debug"}"#)
        .create();

    floo()
        .args(["env", "get", "LOG_LEVEL", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("debug"));
}

#[test]
fn test_env_get_human_secret_refused_without_reveal() {
    // Regression for the #1152 inversion: human `env get` on a secret-shaped key
    // must NOT print plaintext. It refuses with SECRET_REVEAL_REQUIRED and emits
    // nothing usable on stdout — matching the --json path, which redacts.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

    let _m_get = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/env/DATABASE_URL").as_str(),
        )
        .match_query(Matcher::UrlEncoded(
            "service_id".into(),
            TEST_SERVICE_ID.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"key":"DATABASE_URL","value":"postgres://u:p@h/db"}"#)
        .create();

    floo()
        .args(["env", "get", "DATABASE_URL", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("postgres://u:p@h/db").not())
        .stderr(predicate::str::contains("reveal-secrets"));
}

#[test]
fn test_env_get_human_secret_printed_with_reveal() {
    // `--reveal-secrets` opts back in to plaintext on stdout for the same key.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

    let _m_get = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/env/DATABASE_URL").as_str(),
        )
        .match_query(Matcher::UrlEncoded(
            "service_id".into(),
            TEST_SERVICE_ID.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"key":"DATABASE_URL","value":"postgres://u:p@h/db"}"#)
        .create();

    floo()
        .args([
            "--reveal-secrets",
            "env",
            "get",
            "DATABASE_URL",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("postgres://u:p@h/db"));
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
fn test_env_list_all_services_no_flag() {
    // #1152: `env list` with no --services on a multi-service app no longer
    // errors MULTIPLE_SERVICES. It reads every service's vars plus app-level in
    // one pass (service_id omitted => the API returns all).
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_two(&mut server);

    let _m_list = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .match_query(Matcher::UrlEncoded("env".into(), "dev".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"env_vars":[
                {"key":"DATABASE_URL","masked_value":"********","service_id":"svc-uuid-1234"},
                {"key":"API_TOKEN","masked_value":"********","service_id":"svc-uuid-5678"},
                {"key":"LOG_LEVEL","masked_value":"********","service_id":null}
            ]}"#,
        )
        .create();

    floo()
        .args(["--json", "env", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("DATABASE_URL"))
        .stdout(predicate::str::contains("API_TOKEN"))
        .stdout(predicate::str::contains("LOG_LEVEL"));
}

#[test]
fn test_env_list_human_shows_scope_when_mixed() {
    // codex #1152: even a single-service app can return app-level rows
    // alongside the service's. The human table must label the scope so the two
    // are distinguishable — the `Service` column appears whenever scopes differ.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_one(&mut server);

    let _m_list = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .match_query(Matcher::UrlEncoded("env".into(), "dev".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"env_vars":[
                {{"key":"APP_WIDE","masked_value":"********","service_id":null}},
                {{"key":"DB_URL","masked_value":"********","service_id":"{TEST_SERVICE_ID}"}}
            ]}}"#
        ))
        .create();

    // Human mode renders the table to stderr.
    floo()
        .args(["env", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Service"))
        .stderr(predicate::str::contains("(app)"))
        .stderr(predicate::str::contains(TEST_SERVICE_NAME));
}

#[test]
fn test_env_list_human_shows_scope_when_multi_service_single_scope() {
    // codex #1152: a multi-service app whose vars currently all sit in one
    // service must still show the Service column — otherwise rows are
    // indistinguishable from app-level or another service's vars.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_list_services_two(&mut server);

    let _m_list = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .match_query(Matcher::UrlEncoded("env".into(), "dev".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"env_vars":[
                {{"key":"DB_URL","masked_value":"********","service_id":"{TEST_SERVICE_ID}"}},
                {{"key":"WORKERS","masked_value":"********","service_id":"{TEST_SERVICE_ID}"}}
            ]}}"#
        ))
        .create();

    floo()
        .args(["env", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Service"))
        .stderr(predicate::str::contains(TEST_SERVICE_NAME));
}

#[test]
fn test_env_list_human_no_scope_column_when_serviceless() {
    // A service-less app has a single possible scope (app-level), so the plain
    // Key/Value table reads cleaner — no Service column.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _services = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/services").as_str())
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"services":[]}"#)
        .create();

    let _m_list = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/env").as_str())
        .match_query(Matcher::UrlEncoded("env".into(), "dev".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"env_vars":[{"key":"LOG_LEVEL","masked_value":"********","service_id":null}]}"#,
        )
        .create();

    floo()
        .args(["env", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("LOG_LEVEL"))
        .stderr(predicate::str::contains("Service").not());
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
fn test_domains_list_is_app_level_on_multi_service() {
    // #1161: custom domains are app/ingress-level, so `domains list` lists
    // every domain on the app and never demands a service target — even on a
    // multi-service app. It does not resolve services at all (no /services
    // call is mocked here, and the command must still succeed).
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
        .with_body(r#"{"domains":[{"hostname":"api.example.com","status":"ACTIVE","dns_instructions":""}]}"#)
        .create();

    floo()
        .args(["--json", "domains", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("api.example.com"));
}

#[test]
fn test_domains_show_json() {
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
            "show",
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
fn test_domains_show_not_found() {
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
            "show",
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
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("include_build_logs".into(), "false".into()),
            Matcher::UrlEncoded("limit".into(), "20".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"deploys":[{"id":"deploy-123","status":"live","runtime":"nodejs","created_at":"2024-01-01T00:00:00Z","started_at":"2024-01-01T00:00:00Z","finished_at":"2024-01-01T00:02:00Z","duration_ms":120000,"failure_reason":null,"failing_stage":null,"build_logs":"massive-build-log"}],"total":1,"page":1,"per_page":20,"limit":20,"next_cursor":"cursor-next","has_more":true}"#,
        )
        .create();

    floo()
        .args(["--json", "deploys", "list", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("deploys"))
        .stdout(predicate::str::contains("deploy-123"))
        .stdout(predicate::str::contains(r#""duration_ms":120000"#))
        .stdout(predicate::str::contains(r#""next_cursor":"cursor-next""#))
        .stdout(predicate::str::contains(r#""has_more":true"#))
        .stdout(predicate::str::contains("build_logs").not())
        .stdout(predicate::str::contains("massive-build-log").not());
}

#[test]
fn test_deploy_list_json_sends_cursor_and_limit() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/deploys").as_str(),
        )
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("include_build_logs".into(), "false".into()),
            Matcher::UrlEncoded("limit".into(), "2".into()),
            Matcher::UrlEncoded("cursor".into(), "cursor-1".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"deploys":[],"total":5,"page":2,"per_page":2,"limit":2,"next_cursor":"cursor-2","has_more":true}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "deploys",
            "list",
            "--app",
            TEST_APP_NAME,
            "--limit",
            "2",
            "--cursor",
            "cursor-1",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""limit":2"#))
        .stdout(predicate::str::contains(r#""next_cursor":"cursor-2""#));
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
fn test_logs_query_json_with_deployment_filter() {
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
            Matcher::UrlEncoded("deployment".into(), "latest".into()),
            Matcher::UrlEncoded("environment".into(), "prod".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"INFO","message":"Prod deploy booted","deployment_id":"deploy-123","service_name":"web","deploy_context":{"deploy_id":"deploy-123"}}],"total":1,"app_name":"my-app","limit":100,"next_cursor":"cursor-next","has_more":true}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "logs",
            "query",
            "--app",
            TEST_APP_NAME,
            "--deployment",
            "latest",
            "--env",
            "prod",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("Prod deploy booted"))
        .stdout(predicate::str::contains(r#""deployment_id":"deploy-123""#))
        .stdout(predicate::str::contains(r#""next_cursor":"cursor-next""#))
        .stdout(predicate::str::contains(r#""has_more":true"#));
}

#[test]
fn test_logs_query_json_sends_cursor_and_limit_alias() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_logs = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/logs").as_str(),
        )
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "25".into()),
            Matcher::UrlEncoded("cursor".into(), "cursor-123".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"INFO","message":"Next page","service_name":"web"}],"total":1,"app_name":"my-app","limit":25,"next_cursor":null,"has_more":false}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "logs",
            "query",
            "--app",
            TEST_APP_NAME,
            "--limit",
            "25",
            "--cursor",
            "cursor-123",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Next page"))
        .stdout(predicate::str::contains(r#""limit":25"#))
        .stdout(predicate::str::contains(r#""has_more":false"#));
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
fn test_logs_human_cron_rows_show_cron_prefix_even_when_filtered() {
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
            Matcher::UrlEncoded("cron".into(), "knowledge-sync".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs":[{"timestamp":"2024-01-01T00:00:00Z","severity":"DEFAULT","message":"sync complete","service_name":null,"cron_job_name":"knowledge-sync"}],"app_name":"my-app"}"#,
        )
        .create();

    floo()
        .args(["logs", "--app", TEST_APP_NAME, "--cron", "knowledge-sync"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[cron:knowledge-sync]"))
        .stderr(predicate::str::contains("sync complete"));
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

// ───────────────────────── Request logs ─────────────────────────

#[test]
fn test_logs_requests_json_default_tail() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_req = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/requests").as_str(),
        )
        .match_query(Matcher::UrlEncoded("limit".into(), "100".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"requests":[{"timestamp":"2026-04-30T15:42:21Z","method":"GET","path":"/","host":"my-app.on.getfloo.com","status_code":401,"latency_ms":12,"access_mode":"public","user_identity":null}],"total":1,"app_name":"my-app"}"#,
        )
        .create();

    let assert = floo()
        .args(["--json", "logs", "--requests", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let envelope: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout must be a single JSON envelope");
    assert_eq!(envelope["success"], serde_json::Value::Bool(true));
    let data = &envelope["data"];
    assert!(data.is_object(), "data must be the request-logs payload");
    assert_eq!(data["total"], serde_json::json!(1));
    assert_eq!(data["app_name"], serde_json::json!("my-app"));
    let requests = data["requests"].as_array().expect("requests array");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["status_code"], serde_json::json!(401));
}

#[test]
fn test_logs_requests_passes_since_to_api() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    // The API mock only matches when the `since` query param is forwarded;
    // forgetting to thread it would 501 against this mock.
    let _m_req = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/requests").as_str())
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "100".into()),
            Matcher::UrlEncoded("since".into(), "5m".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"requests":[],"total":0,"app_name":"my-app"}"#)
        .create();

    floo()
        .args([
            "--json",
            "logs",
            "--requests",
            "--app",
            TEST_APP_NAME,
            "--since",
            "5m",
        ])
        .env("HOME", home.path())
        .assert()
        .success();
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
fn test_db_branches_list_json_returns_preview_context_and_branch_contract() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let branch = preview_branch_json("default", "ready", true);
    let _preview = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/previews/feat-db-abcde").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(preview_detail_json(&branch))
        .create();
    let _branches = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/previews/feat-db-abcde/database-branches").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"database_branches":[{branch}],"total":1}}"#))
        .create();

    floo()
        .args([
            "--json",
            "db",
            "branches",
            "list",
            "feat-db-abcde",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""environment_name":"preview""#))
        .stdout(predicate::str::contains(r#""source_environment":"dev""#))
        .stdout(predicate::str::contains(r#""hydration_mode":"clone-dev""#))
        .stdout(predicate::str::contains(r#""resource_status":"ready""#))
        .stdout(predicate::str::contains(
            "app_default_preview_feat_db_abcde",
        ))
        .stdout(predicate::str::contains("postgresql://").not())
        .stdout(predicate::str::contains("password").not());
}

#[test]
fn test_db_branches_list_empty_state_names_missing_managed_postgres() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _preview = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/previews/feat-db-abcde").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(preview_detail_json(""))
        .create();
    let _branches = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/previews/feat-db-abcde/database-branches").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"database_branches":[],"total":0}"#)
        .create();

    floo()
        .args([
            "db",
            "branches",
            "list",
            "feat-db-abcde",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "This preview has no managed Postgres attachment",
        ));
}

#[test]
fn test_db_branches_show_surfaces_branch_not_found_code() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _branch = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/previews/feat-db-abcde/database-branches/analytics")
                .as_str(),
        )
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"detail":{"code":"PREVIEW_DATABASE_BRANCH_NOT_FOUND","message":"Preview database branch not found."}}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "db",
            "branches",
            "show",
            "feat-db-abcde",
            "--name",
            "analytics",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "PREVIEW_DATABASE_BRANCH_NOT_FOUND",
        ));
}

#[test]
fn test_db_branches_source_branch_identifier_reports_ambiguous_preview() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let branch = preview_branch_json("default", "ready", true);
    let preview_a = preview_detail_json(&branch);
    let preview_b = preview_a.replace("feat-db-abcde", "feat-db-f00ba");
    let _previews = server
        .mock("GET", format!("/v1/apps/{TEST_APP_ID}/previews").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"previews":[{preview_a},{preview_b}],"total":2}}"#
        ))
        .create();

    floo()
        .args([
            "--json",
            "db",
            "branches",
            "list",
            "feat/db-branch",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("AMBIGUOUS_PREVIEW_IDENTIFIER"));
}

#[test]
fn test_db_branches_reset_refuses_without_confirmation_in_json_mode() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    floo()
        .args([
            "--json",
            "db",
            "branches",
            "reset",
            "feat-db-abcde",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("CONFIRMATION_REQUIRED"))
        .stdout(predicate::str::contains("dev/prod untouched"))
        .stdout(predicate::str::contains("--yes"));
}

#[test]
fn test_db_branches_reset_with_yes_calls_preview_scoped_endpoint() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _reset = server
        .mock(
            "POST",
            format!(
                "/v1/apps/{TEST_APP_ID}/previews/feat-db-abcde/database-branches/default/reset"
            )
            .as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(preview_branch_json("default", "ready", true))
        .create();

    floo()
        .args([
            "--json",
            "db",
            "branches",
            "reset",
            "feat-db-abcde",
            "--app",
            TEST_APP_NAME,
            "--yes",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""dev_prod_untouched":true"#))
        .stdout(predicate::str::contains(r#""scope":"preview""#))
        .stdout(predicate::str::contains(r#""database_branch":{"#))
        .stdout(predicate::str::contains("postgresql://").not());
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
fn test_services_show_user_managed() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _services = mock_services_single(&mut server);

    floo()
        .args(["--json", "services", "show", "web", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("web"))
        .stdout(predicate::str::contains("https://web.floo.app"));
}

#[test]
fn test_services_show_routes_to_managed_service_by_type() {
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
            "show",
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
fn test_services_show_nothing_matches_lists_available() {
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
        .args(["--json", "services", "show", "nope", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("SERVICE_NOT_FOUND"))
        .stdout(predicate::str::contains("web"))
        .stdout(predicate::str::contains("redis"));
}

#[test]
fn test_services_show_app_not_found() {
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
        .args(["--json", "services", "show", "db", "--app", "missing-app"])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("APP_NOT_FOUND"));
}

#[test]
fn test_storage_versions_json_lists_object_generations() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"managed_services":[{"id":"ms-storage-1","app_id":"app-uuid-1234","type":"storage","name":"default","status":"ready","env_var_keys":["STORAGE_BUCKET","STORAGE_URL"],"created_at":null,"updated_at":null}],"total":1}"#,
        )
        .create();

    let _m_versions = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services/ms-storage-1/storage/versions")
                .as_str(),
        )
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("object_path".into(), "uploads/report.json".into()),
            Matcher::UrlEncoded("env".into(), "dev".into()),
            Matcher::UrlEncoded("limit".into(), "75".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"bucket_name":"floo-app-bucket","object_path":"uploads/report.json","versions":[{"object_path":"uploads/report.json","generation":"1700000000000001","is_live":true,"size_bytes":256,"size_human":"256 B","updated_at":null,"created_at":null,"content_type":"application/json","etag":"abc"},{"object_path":"uploads/report.json","generation":"1700000000000000","is_live":false,"size_bytes":128,"size_human":"128 B","updated_at":null,"created_at":null,"content_type":"application/json","etag":"def"}],"total_returned":2,"truncated":false}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "storage",
            "versions",
            "uploads/report.json",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(
            r#""generation":"1700000000000001""#,
        ))
        .stdout(predicate::str::contains(r#""is_live":false"#));
}

#[test]
fn test_storage_restore_json_restores_generation() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"managed_services":[{"id":"ms-storage-1","app_id":"app-uuid-1234","type":"storage","name":"default","status":"ready","env_var_keys":["STORAGE_BUCKET","STORAGE_URL"],"created_at":null,"updated_at":null}],"total":1}"#,
        )
        .create();

    let _m_restore = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/managed-services/ms-storage-1/storage/restore")
                .as_str(),
        )
        .match_query(Matcher::UrlEncoded("env".into(), "prod".into()))
        .match_body(Matcher::JsonString(
            r#"{"object_path":"uploads/report.json","generation":"1700000000000000"}"#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"bucket_name":"floo-app-bucket-prod","object_path":"uploads/report.json","restored_generation":"1700000000000000","live_generation":"1700000000000002","size_bytes":128,"size_human":"128 B","content_type":"application/json"}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "storage",
            "restore",
            "uploads/report.json",
            "--generation",
            "1700000000000000",
            "--env",
            "prod",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(
            r#""restored_generation":"1700000000000000""#,
        ))
        .stdout(predicate::str::contains(
            r#""live_generation":"1700000000000002""#,
        ));
}

#[test]
fn test_db_backup_json_creates_managed_postgres_backup() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"managed_services":[{"id":"ms-pg-1","app_id":"app-uuid-1234","type":"postgres","name":"default","status":"ready","env_var_keys":["DATABASE_URL"],"created_at":null,"updated_at":null}],"total":1}"#,
        )
        .create();

    let _m_backup = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/managed-services/ms-pg-1/postgres/backups").as_str(),
        )
        .match_query(Matcher::UrlEncoded("env".into(), "prod".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"backup-1","app_id":"app-uuid-1234","managed_service_id":"ms-pg-1","env":"prod","status":"available","size_bytes":2048,"size_human":"2.0 KB","checksum_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","created_at":"2026-06-19T12:00:00Z","expires_at":"2026-07-19T12:00:00Z","last_restored_at":null}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "db",
            "backup",
            "--env",
            "prod",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""id":"backup-1""#))
        .stdout(predicate::str::contains(r#""env":"prod""#));
}

#[test]
fn test_db_backups_json_lists_managed_postgres_backups() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"managed_services":[{"id":"ms-pg-1","app_id":"app-uuid-1234","type":"postgres","name":"default","status":"ready","env_var_keys":["DATABASE_URL"],"created_at":null,"updated_at":null}],"total":1}"#,
        )
        .create();

    let _m_backups = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services/ms-pg-1/postgres/backups").as_str(),
        )
        .match_query(Matcher::UrlEncoded("env".into(), "dev".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"backups":[{"id":"backup-1","app_id":"app-uuid-1234","managed_service_id":"ms-pg-1","env":"dev","status":"available","size_bytes":2048,"size_human":"2.0 KB","checksum_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","created_at":"2026-06-19T12:00:00Z","expires_at":"2026-07-19T12:00:00Z","last_restored_at":null}],"total":1}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "db",
            "backups",
            "--env",
            "dev",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""total":1"#))
        .stdout(predicate::str::contains(r#""id":"backup-1""#));
}

#[test]
fn test_db_restore_json_restores_managed_postgres_backup() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);

    let _m_list_managed = server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/managed-services").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"managed_services":[{"id":"ms-pg-1","app_id":"app-uuid-1234","type":"postgres","name":"default","status":"ready","env_var_keys":["DATABASE_URL"],"created_at":null,"updated_at":null}],"total":1}"#,
        )
        .create();

    let _m_restore = server
        .mock(
            "POST",
            format!("/v1/apps/{TEST_APP_ID}/managed-services/ms-pg-1/postgres/restore").as_str(),
        )
        .match_body(Matcher::JsonString(
            r#"{"backup_id":"backup-1","env":"dev"}"#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"backup":{"id":"backup-1","app_id":"app-uuid-1234","managed_service_id":"ms-pg-1","env":"dev","status":"available","size_bytes":2048,"size_human":"2.0 KB","checksum_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","created_at":"2026-06-19T12:00:00Z","expires_at":"2026-07-19T12:00:00Z","last_restored_at":"2026-06-19T12:05:00Z"},"restored_at":"2026-06-19T12:05:00Z"}"#,
        )
        .create();

    floo()
        .args([
            "--json",
            "db",
            "restore",
            "backup-1",
            "--env",
            "dev",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(
            r#""restored_at":"2026-06-19T12:05:00Z""#,
        ));
}

#[test]
fn test_services_show_surfaces_api_errors() {
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
            "show",
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
        .match_query(Matcher::UrlEncoded(
            "include_build_logs".into(),
            "false".into(),
        ))
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
        .with_body(
            r#"{"id":"deploy-001","status":"live","url":"https://test-deploy.floo.app","build_logs":""}"#,
        )
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

// ───────── Deploy not-found keyed on HTTP 404 status (#152 follow-up) ─────────
//
// #157 hardened resolve_app_or_exit / logs / releases / github / the deploy()
// --app restart path onto FlooApiError::is_not_found(), but the config-resolved
// create path (deploy.rs ~544) still read the drift-prone `code ==
// "APP_NOT_FOUND"` string. These tests pin not-found detection to the 404
// status: when resolve_app hits GET /v1/apps/{id} (UUID identifiers) the
// server's own 404 + code flow through unchanged, so a code-string gate misses
// a 404 carrying any other code. Keyed on status, the path fires regardless.

// Guards the live `--app` resolution path (deploy.rs ~300, migrated by #157):
// `floo deploy --app <missing>` must surface the friendly app-not-found error +
// suggestion even when the server's 404 carries a non-APP_NOT_FOUND code. The
// `--app` early-return block handles every Some(app) case, so this is the only
// reachable `--app` not-found path (the unreachable AppSource::Flag branch in
// Path 3 was removed in this change).
#[test]
fn test_deploy_app_flag_not_found_keyed_on_status() {
    let mut server = Server::new();
    let home = setup_config(&server);
    // A UUID --app routes resolve_app to GET /v1/apps/{id}, so the server code
    // (here NOT_FOUND, not APP_NOT_FOUND) reaches the gate verbatim.
    let missing = "11111111-1111-1111-1111-111111111111";

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();
    write_service_config(&project, TEST_APP_NAME);

    let _m_get = server
        .mock("GET", format!("/v1/apps/{missing}").as_str())
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"NOT_FOUND","message":"Not found"}}"#)
        .create();

    floo()
        .args([
            "--json",
            "redeploy",
            project.path().to_str().unwrap(),
            "--app",
            missing,
        ])
        .env("HOME", home.path())
        .assert()
        .failure()
        // The friendly app-not-found error + suggestion only fire on the
        // is_not_found() branch; the old string gate fell through to the raw
        // NOT_FOUND error with no suggestion.
        .stdout(predicate::str::contains("APP_NOT_FOUND"))
        .stdout(predicate::str::contains(
            "Check the app name or ID and try again.",
        ));
}

#[test]
fn test_deploy_config_not_found_keyed_on_status_creates() {
    let mut server = Server::new();
    let home = setup_config(&server);
    // Config-resolved (no --app): a UUID app name in floo.service.toml routes
    // resolve_app to GET /v1/apps/{id}, so a 404 carrying a drifted code must
    // still be treated as "app doesn't exist yet" → create it, not surfaced as
    // a hard error.
    let missing = "22222222-2222-2222-2222-222222222222";

    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"test-project","version":"1.0.0"}"#,
    )
    .unwrap();
    write_service_config(&project, missing);

    let _m_get = server
        .mock("GET", format!("/v1/apps/{missing}").as_str())
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"detail":{"code":"NOT_FOUND","message":"Not found"}}"#)
        .create();

    let _m_create = server
        .mock("POST", "/v1/apps")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"id":"{TEST_APP_ID}","name":"web","status":"created","runtime":"nodejs"}}"#
        ))
        .create();

    let _m_deploy = server
        .mock("POST", format!("/v1/apps/{TEST_APP_ID}/deploys").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"deploy-001","status":"live","url":"https://web.floo.app","build_logs":""}"#,
        )
        .create();

    floo()
        .args(["--json", "redeploy", project.path().to_str().unwrap()])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#));
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
        .args(["--json", "apps", "show", "nonexistent"])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("APP_NOT_FOUND"));
}

#[test]
fn test_apps_show_uuid_surfaces_get_app_api_error() {
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
        .args(["--json", "apps", "show", app_id])
        .env("HOME", home.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("INTERNAL_ERROR"))
        .stdout(predicate::str::contains("APP_NOT_FOUND").not());
}

#[test]
fn test_apps_show_name_surfaces_list_apps_api_error() {
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
        .args(["--json", "apps", "show", TEST_APP_NAME])
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

// ───────────────────────── Database ─────────────────────────

/// Mock POST /v1/apps/{id}/db/query with a given response body.
fn mock_db_query(server: &mut Server, body: &str) -> Mock {
    server
        .mock("POST", format!("/v1/apps/{TEST_APP_ID}/db/query").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create()
}

#[test]
fn test_db_query_human_renders_array_rows() {
    // Regression guard for #153: the API returns rows as an array of arrays
    // with column names carried separately in `columns`. The human renderer
    // must align each row to `columns` and display the real rows — not print
    // "0 rows" for a non-empty result.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _query = mock_db_query(
        &mut server,
        r#"{"columns":["id","email"],"rows":[[1,"alice@example.com"],[2,"bob@example.com"]]}"#,
    );

    floo()
        .args([
            "db",
            "query",
            "SELECT id, email FROM users",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        // Header row aligned to `columns`.
        .stderr(predicate::str::contains("id"))
        .stderr(predicate::str::contains("email"))
        // Actual row values render.
        .stderr(predicate::str::contains("alice@example.com"))
        .stderr(predicate::str::contains("bob@example.com"))
        // Non-empty count — never the bogus "0 rows".
        .stderr(predicate::str::contains("2 rows"))
        .stderr(predicate::str::contains("0 rows").not());
}

#[test]
fn test_db_query_human_single_row_singular_count() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _query = mock_db_query(&mut server, r#"{"columns":["count"],"rows":[[42]]}"#);

    floo()
        .args([
            "db",
            "query",
            "SELECT COUNT(*) AS count FROM orders",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("count"))
        .stderr(predicate::str::contains("42"))
        .stderr(predicate::str::contains("1 row"))
        .stderr(predicate::str::contains("1 rows").not());
}

#[test]
fn test_db_query_human_genuine_zero_rows() {
    // A genuinely empty result set must still report "0 rows".
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _query = mock_db_query(&mut server, r#"{"columns":["id","email"],"rows":[]}"#);

    floo()
        .args([
            "db",
            "query",
            "SELECT id, email FROM users WHERE 1=0",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("0 rows"));
}

#[test]
fn test_db_query_json_passthrough_exposes_contract() {
    // The invariant: --json exposes exactly the API's {columns, rows} contract,
    // which the human renderer above consumes identically. Lock the shape so a
    // future renderer change can't silently diverge the two output modes.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _query = mock_db_query(
        &mut server,
        r#"{"columns":["id","email"],"rows":[[1,"alice@example.com"]]}"#,
    );

    floo()
        .args([
            "--json",
            "db",
            "query",
            "SELECT id, email FROM users",
            "--app",
            TEST_APP_NAME,
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""columns":["id","email"]"#))
        .stdout(predicate::str::contains("alice@example.com"));
}

const NOTIF_PREFS_DEFAULTS: &str = r#"{"preferences":[{"category":"deploy_success","label":"Deploy succeeded","description":"When a deploy finishes successfully.","enabled":false,"is_default":true},{"category":"billing","label":"Spend cap warnings","description":"When your org approaches its spend cap.","enabled":true,"is_default":true}]}"#;

#[test]
fn test_notifications_list_json() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let _m = server
        .mock("GET", "/v1/notification-preferences")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(NOTIF_PREFS_DEFAULTS)
        .create();

    floo()
        .args(["--json", "notifications", "list"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains("deploy_success"))
        .stdout(predicate::str::contains(r#""enabled":false"#));
}

#[test]
fn test_notifications_list_human_shows_category_and_label() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let _m = server
        .mock("GET", "/v1/notification-preferences")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(NOTIF_PREFS_DEFAULTS)
        .create();

    floo()
        .args(["notifications", "list"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("deploy_success"))
        .stderr(predicate::str::contains("Deploy succeeded"));
}

#[test]
fn test_notifications_set_json() {
    let mut server = Server::new();
    let home = setup_config(&server);

    let _m = server
        .mock("PATCH", "/v1/notification-preferences")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"preferences":[{"category":"deploy_success","label":"Deploy succeeded","description":"d","enabled":true,"is_default":false}]}"#,
        )
        .create();

    floo()
        .args(["--json", "notifications", "set", "deploy_success", "on"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""enabled":true"#));
}

// ───────────────────────── Doctor (#1156) ─────────────────────────

/// Mock GET /v1/apps/{id}/doctor/accounts with a caller-supplied body so each
/// test controls the drift list and (optional) drift_detected field.
fn mock_doctor_accounts(server: &mut Server, body: &str) -> Mock {
    server
        .mock(
            "GET",
            format!("/v1/apps/{TEST_APP_ID}/doctor/accounts").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create()
}

fn doctor_body(drift_detected_field: &str, drift_items: &str) -> String {
    format!(
        r#"{{"app_id":"{TEST_APP_ID}","app_name":"{TEST_APP_NAME}","requested":{{"access_mode":"accounts","access_policy":"invite","allowed_domains":[]}},"serving":[],"latest_deploy":null{drift_detected_field},"drift":[{drift_items}]}}"#
    )
}

const DRIFT_ITEM: &str =
    r#"{"kind":"access_mode_drift","summary":"gateway serves public","likely_fix":null}"#;

#[test]
fn test_doctor_accounts_json_no_drift_exits_zero() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _doctor = mock_doctor_accounts(&mut server, &doctor_body(r#","drift_detected":false"#, ""));

    floo()
        .args(["--json", "doctor", "accounts", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .success() // exit 0 ⇔ drift_detected:false
        .stdout(predicate::str::contains(r#""drift_detected":false"#));
}

#[test]
fn test_doctor_accounts_json_drift_exits_one_and_body_agrees() {
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _doctor = mock_doctor_accounts(
        &mut server,
        &doctor_body(r#","drift_detected":true"#, DRIFT_ITEM),
    );

    // Exit code (1) and the body verdict (drift_detected:true) agree — the
    // core #1156 contract: an agent branching on either reaches the same answer.
    floo()
        .args(["--json", "doctor", "accounts", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains(r#""drift_detected":true"#))
        .stdout(predicate::str::contains("access_mode_drift"));
}

#[test]
fn test_doctor_accounts_json_old_api_no_field_derives_from_drift_list() {
    // An API predating drift_detected omits the field; the CLI derives the
    // verdict from the drift list and still emits a definitive bool + exit 1.
    let mut server = Server::new();
    let home = setup_config(&server);
    let _resolve = mock_resolve_app(&mut server);
    let _doctor = mock_doctor_accounts(&mut server, &doctor_body("", DRIFT_ITEM));

    floo()
        .args(["--json", "doctor", "accounts", "--app", TEST_APP_NAME])
        .env("HOME", home.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains(r#""drift_detected":true"#));
}
