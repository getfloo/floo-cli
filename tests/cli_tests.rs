use assert_cmd::Command;
use predicates::prelude::*;

#[allow(deprecated)]
fn floo() -> Command {
    Command::cargo_bin("floo-local").unwrap()
}

// --- Help & Version ---

#[test]
fn test_help() {
    floo()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Manage and observe web apps. Deploys are git-driven.",
        ));
}

/// Assert stdout contains a line that, when trimmed, equals exactly `tag`.
/// The regression test for `floo --version`'s stdout output needs to pin
/// the EXACT content, not just a substring — `contains("0.0.0-dev")` would
/// pass on "floo-local 0.0.0-dev\n" or "Warning: 0.0.0-dev stale\n", both
/// of which would break install.sh (which captures stdout verbatim and
/// embeds it into shell strings) even though the test would be green.
fn stdout_has_exact_line(tag: &'static str) -> impl Predicate<str> {
    predicate::function(move |s: &str| s.lines().any(|l| l.trim() == tag))
}

#[test]
fn test_version() {
    // `floo --version` is rewritten to the `version` subcommand in
    // cli::rewrite_bare_version_flag() so it hits the network, applies any
    // staged update, and refreshes SKILL.md — matching `floo version`.
    //
    // Output contract (both are load-bearing):
    //   - stdout: the bare version tag as an exact line. install.sh
    //     captures stdout from `floo --version` to verify the install
    //     worked, and Unix scripts expect `--version` on stdout. A
    //     prior revision (shipped in v2026.04.12.1) only wrote to
    //     stderr and broke install.sh in the wild — that's what the
    //     exact-line assertion below guards against.
    //   - stderr: the colored status line `✓ floo X.Y.Z`, matching the
    //     rest of floo's human output style.
    //
    // FLOO_NO_UPDATE_CHECK=1 exercises the skip-network arm so the test
    // doesn't try to reach GitHub (and, via run_update, clobber the
    // cargo test binary via install_binary).
    floo()
        .env("FLOO_NO_UPDATE_CHECK", "1")
        .arg("--version")
        .assert()
        .success()
        .stdout(stdout_has_exact_line("0.0.0-dev"))
        .stderr(predicate::str::contains("floo 0.0.0-dev"));
}

#[test]
fn test_version_command_human() {
    // Same output contract as test_version above. Uses the exact-line
    // predicate so a future prefix like "floo-local 0.0.0-dev" on stdout
    // would fail this test — install.sh captures stdout verbatim.
    floo()
        .env("FLOO_NO_UPDATE_CHECK", "1")
        .arg("version")
        .assert()
        .success()
        .stdout(stdout_has_exact_line("0.0.0-dev"))
        .stderr(predicate::str::contains("floo 0.0.0-dev"));
}

#[test]
fn test_version_command_json() {
    // JSON mode must NOT leak anything to stderr — agents parsing
    // `floo version --json` expect the entire response on stdout. The
    // empty-stderr assertion catches any future refactor that
    // accidentally reintroduces a human-mode write under --json.
    floo()
        .env("FLOO_NO_UPDATE_CHECK", "1")
        .args(["--json", "version"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""version":"0.0.0-dev""#))
        .stderr(predicate::str::is_empty());
}

#[test]
fn test_no_args_shows_help() {
    floo()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage: floo"));
}

// --- Auth subcommand ---

#[test]
fn test_auth_help() {
    floo()
        .args(["auth", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Authenticate and manage your account",
        ));
}

#[test]
fn test_auth_login_help() {
    floo()
        .args(["auth", "login", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Authenticate with the Floo API"));
}

#[test]
fn test_auth_register_help() {
    floo()
        .args(["auth", "register", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Create a new Floo account"));
}

#[test]
fn test_auth_register_missing_email() {
    floo()
        .args(["auth", "register"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_auth_whoami_not_authenticated() {
    floo()
        .args(["auth", "whoami"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_auth_whoami_json_not_authenticated() {
    floo()
        .args(["--json", "auth", "whoami"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NOT_AUTHENTICATED"#));
}

#[test]
fn test_auth_logout_succeeds() {
    floo()
        .args(["auth", "logout"])
        .env("HOME", "/tmp/floo-test-logout")
        .assert()
        .success()
        .stderr(predicate::str::contains("Logged out."));
}

#[test]
fn test_auth_logout_json() {
    floo()
        .args(["--json", "auth", "logout"])
        .env("HOME", "/tmp/floo-test-logout-json")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#));
}

#[test]
fn test_auth_whoami_help() {
    floo()
        .args(["auth", "whoami", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Show the currently authenticated user",
        ));
}

// --- Apps (unauthenticated) ---

#[test]
fn test_apps_list_not_authenticated() {
    floo()
        .args(["apps", "list"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_apps_list_json_not_authenticated() {
    floo()
        .args(["--json", "apps", "list"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NOT_AUTHENTICATED"#));
}

// --- Subcommand help ---

#[test]
fn test_apps_help() {
    floo()
        .args(["apps", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Manage your apps"));
}

#[test]
fn test_env_help() {
    floo()
        .args(["env", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Manage environment variables"));
}

#[test]
fn test_domains_help() {
    floo()
        .args(["domains", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Manage custom domains"));
}

#[test]
fn test_services_help() {
    floo()
        .args(["services", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Manage services for an app"));
}

#[test]
fn test_deploy_help() {
    floo()
        .args(["deploys", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("View and manage deploy history"));
}

#[test]
fn test_releases_promote_help() {
    floo()
        .args(["releases", "promote", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Promote an app to prod"));
}

#[test]
fn test_update_help() {
    floo()
        .args(["update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Update the CLI binary in-place"));
}

// --- Deploy (unauthenticated) ---
// Preflight (config resolution) runs before auth, so without a config file
// the error is NO_CONFIG_FOUND rather than NOT_AUTHENTICATED.

#[test]
fn test_deploy_not_authenticated() {
    floo()
        .arg("redeploy")
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "No floo.app.toml or floo.service.toml found.",
        ));
}

#[test]
fn test_deploy_json_not_authenticated() {
    floo()
        .args(["--json", "redeploy"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NO_CONFIG_FOUND"#));
}

// --- Env (unauthenticated) ---

#[test]
fn test_env_set_not_authenticated() {
    floo()
        .args(["env", "set", "KEY=value", "--app", "test"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_env_list_not_authenticated() {
    floo()
        .args(["env", "list", "--app", "test"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_env_remove_not_authenticated() {
    floo()
        .args(["env", "remove", "MY_KEY", "--app", "test"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_env_get_not_authenticated() {
    floo()
        .args(["env", "get", "MY_KEY", "--app", "test"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_env_import_not_authenticated() {
    floo()
        .args(["env", "import", "--app", "test"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

// --- Domains (unauthenticated) ---

#[test]
fn test_domains_add_not_authenticated() {
    floo()
        .args(["domains", "add", "example.com", "--app", "test"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_domains_list_not_authenticated() {
    floo()
        .args(["domains", "list", "--app", "test"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

// --- Services (unauthenticated) ---

#[test]
fn test_services_info_not_authenticated() {
    floo()
        .args(["services", "info", "db", "--app", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_services_info_json_not_authenticated() {
    floo()
        .args(["--json", "services", "info", "db", "--app", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NOT_AUTHENTICATED"#));
}

// --- Env format validation ---

#[test]
fn test_env_set_invalid_format() {
    // Create a fake config with an API key so we get past the auth check
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key": "floo_test123", "api_url": "https://api.test.local"}"#,
    )
    .unwrap();

    floo()
        .args(["env", "set", "NOEQUALS", "--app", "test"])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid format. Use KEY=VALUE."));
}

#[test]
fn test_env_set_invalid_format_json() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key": "floo_test123", "api_url": "https://api.test.local"}"#,
    )
    .unwrap();

    floo()
        .args(["--json", "env", "set", "NOEQUALS", "--app", "test"])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"INVALID_FORMAT"#));
}

// --- Deploy subcommands ---

#[test]
fn test_deploy_list_help() {
    floo()
        .args(["deploys", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("List deploy history"));
}

#[test]
fn test_deploy_list_not_authenticated() {
    floo()
        .args(["deploys", "list", "--app", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_deploy_list_json_not_authenticated() {
    floo()
        .args(["--json", "deploys", "list", "--app", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NOT_AUTHENTICATED"#));
}

#[test]
fn test_deploy_watch_help() {
    floo()
        .args(["deploys", "watch", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stream deploy progress"));
}

#[test]
fn test_deploy_rollback_help() {
    floo()
        .args(["deploys", "rollback", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Rollback to a previous deploy"));
}

#[test]
fn test_deploy_rollback_not_authenticated() {
    floo()
        .args(["deploys", "rollback", "my-app", "some-deploy-id"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_deploy_rollback_json_not_authenticated() {
    floo()
        .args(["--json", "deploys", "rollback", "my-app", "some-deploy-id"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NOT_AUTHENTICATED"#));
}

// --- Logs ---

#[test]
fn test_logs_help() {
    floo()
        .args(["logs", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("View runtime logs"));
}

#[test]
fn test_logs_not_authenticated() {
    floo()
        .args(["logs", "--app", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_logs_json_not_authenticated() {
    floo()
        .args(["--json", "logs", "--app", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NOT_AUTHENTICATED"#));
}

// --- Config file validation ---

#[test]
fn test_deploy_legacy_floo_toml() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key": "floo_test123", "api_url": "https://api.test.local"}"#,
    )
    .unwrap();

    // Create a project dir with a legacy floo.toml and a recognizable project file
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
    std::fs::write(
        project.path().join("floo.toml"),
        r#"[app]
name = "my-app"

[[services]]
name = "web"
type = "web"
path = "."
port = 3000
ingress = "public"
"#,
    )
    .unwrap();

    floo()
        .args(["redeploy", project.path().to_str().unwrap()])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains("no longer supported"));
}

#[test]
fn test_deploy_legacy_floo_toml_json() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key": "floo_test123", "api_url": "https://api.test.local"}"#,
    )
    .unwrap();

    // Create a project dir with a legacy floo.toml and a recognizable project file
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
    std::fs::write(
        project.path().join("floo.toml"),
        r#"[app]
name = "my-app"

[[services]]
name = "web"
type = "web"
path = "."
port = 3000
ingress = "public"
"#,
    )
    .unwrap();

    floo()
        .args(["--json", "redeploy", project.path().to_str().unwrap()])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"LEGACY_CONFIG"#));
}

#[test]
fn test_deploy_invalid_service_config() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key": "floo_test123", "api_url": "https://api.test.local"}"#,
    )
    .unwrap();

    // Create a project dir with an invalid floo.service.toml
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
    std::fs::write(
        project.path().join("floo.service.toml"),
        r#"[app]
name = "my-app"

[service]
name = "web"
type = "database"
port = 3000
ingress = "public"
"#,
    )
    .unwrap();

    floo()
        .args(["redeploy", project.path().to_str().unwrap()])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid floo.service.toml"));
}

#[test]
fn test_deploy_invalid_service_config_json() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key": "floo_test123", "api_url": "https://api.test.local"}"#,
    )
    .unwrap();

    // Create a project dir with an invalid floo.service.toml
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
    std::fs::write(
        project.path().join("floo.service.toml"),
        r#"[app]
name = "my-app"

[service]
name = "web"
type = "database"
port = 3000
ingress = "public"
"#,
    )
    .unwrap();

    floo()
        .args(["--json", "redeploy", project.path().to_str().unwrap()])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            r#""code":"INVALID_PROJECT_CONFIG"#,
        ));
}

// --- Top-level login/logout/whoami removed ---

#[test]
fn test_top_level_login_removed() {
    floo()
        .arg("login")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn test_top_level_logout_removed() {
    floo()
        .arg("logout")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn test_top_level_whoami_removed() {
    floo()
        .arg("whoami")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

// --- Deploy non-interactive (piped stdin) ---

#[test]
fn test_deploy_no_config_piped_errors() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key": "floo_test123", "api_url": "https://api.test.local"}"#,
    )
    .unwrap();

    // Empty project dir with a recognizable file but no floo config
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();

    // assert_cmd pipes stdin (no TTY), so this should error with NO_CONFIG_FOUND
    floo()
        .args(["redeploy", project.path().to_str().unwrap()])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("NO_CONFIG_FOUND").or(predicate::str::contains("floo init")),
        );
}

#[test]
fn test_deploy_no_config_piped_json_errors() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key": "floo_test123", "api_url": "https://api.test.local"}"#,
    )
    .unwrap();

    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();

    floo()
        .args(["--json", "redeploy", project.path().to_str().unwrap()])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NO_CONFIG_FOUND"#));
}

// --- Init command ---

#[test]
fn test_init_help() {
    floo()
        .args(["init", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialize a new Floo project"));
}

#[test]
fn test_init_creates_config_json() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"dependencies": {"next": "^14.0.0"}}"#,
    )
    .unwrap();

    floo()
        .args([
            "--json",
            "init",
            "myapp",
            "--path",
            project.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""app_name":"myapp"#))
        .stdout(predicate::str::contains(r#""success":true"#));

    // Service is now inline in floo.app.toml — no separate floo.service.toml
    assert!(project.path().join("floo.app.toml").exists());
    assert!(!project.path().join("floo.service.toml").exists());
}

#[test]
fn test_init_writes_header_with_access_mode_and_autodeploy_signal() {
    // Pins the in-file friction fixes for floo-artifact 2026-05-01
    // (`88e32b22` access_mode placement, `c9b70eb5` no auto-deploy signal).
    // Both points need to live IN the file the user opens — a hint in
    // post-init terminal output is too easy to skip past, and the user
    // who reported these had already done so.
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"dependencies": {"next": "^14.0.0"}}"#,
    )
    .unwrap();

    floo()
        .args([
            "--json",
            "init",
            "myapp",
            "--path",
            project.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let toml = std::fs::read_to_string(project.path().join("floo.app.toml")).unwrap();
    assert!(toml.starts_with("# floo.app.toml"), "header should lead the file");
    // git push auto-deploy contract is mentioned by name.
    assert!(toml.contains("git push"), "git push contract must be in header");
    // access_mode placement is shown in BOTH valid locations so the user
    // doesn't have to dig into hosted docs to find out which is right.
    assert!(toml.contains("[app] access_mode"));
    assert!(toml.contains("[environments.dev]"));
    // Output toml after the header is still parseable — sanity-check that
    // we didn't break TOML by prepending comments without a trailing newline.
    let cfg: toml::Value = toml::from_str(&toml).unwrap();
    assert_eq!(cfg["app"]["name"].as_str(), Some("myapp"));
}

#[test]
fn test_init_requires_name_in_json_mode() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();

    floo()
        .args(["--json", "init", "--path", project.path().to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"MISSING_APP_NAME"#));
}

#[test]
fn test_init_refuses_existing_config() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        "[app]\nname = \"existing\"\n",
    )
    .unwrap();

    floo()
        .args([
            "--json",
            "init",
            "myapp",
            "--path",
            project.path().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"CONFIG_EXISTS"#));
}

// --- Deploy --dry-run (replaces floo check) ---

#[test]
fn test_deploy_dry_run_valid_config() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        "[app]\nname = \"myapp\"\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("floo.service.toml"),
        r#"[app]
name = "myapp"

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
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""valid":true"#));
}

#[test]
fn test_deploy_dry_run_missing_service_toml() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        r#"[app]
name = "myapp"

[services.api]
type = "api"
path = "./api"
"#,
    )
    .unwrap();
    // Create the dir but NOT the service toml
    std::fs::create_dir(project.path().join("api")).unwrap();

    floo()
        .args(["--json", "preflight", project.path().to_str().unwrap()])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            r#""code":"SERVICE_CONFIG_MISSING""#,
        ));
}

#[test]
fn test_deploy_dry_run_port_mismatch_warning() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        "[app]\nname = \"myapp\"\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("floo.service.toml"),
        r#"[app]
name = "myapp"

[service]
name = "web"
type = "web"
port = 3000
ingress = "public"
"#,
    )
    .unwrap();
    // Dockerfile with different EXPOSE
    std::fs::write(
        project.path().join("Dockerfile"),
        "FROM node:18\nEXPOSE 8080\n",
    )
    .unwrap();

    floo()
        .args(["--json", "preflight", project.path().to_str().unwrap()])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains("EXPOSE 8080"));
}

#[test]
fn test_preflight_warns_for_rails_cloudsql_socket_database_url() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        r#"[app]
name = "railsapp"

[postgres]
tier = "basic"

[services.web]
type = "web"
path = "."
port = 3000
ingress = "public"
"#,
    )
    .unwrap();
    std::fs::write(
        project.path().join("Gemfile"),
        "source \"https://rubygems.org\"\ngem \"rails\"\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join(".env"),
        "DATABASE_URL=postgresql://user:pass@/floo_apps?host=/cloudsql/project:region:instance\n",
    )
    .unwrap();

    floo()
        .args(["--json", "preflight", project.path().to_str().unwrap()])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Cloud SQL socket-style DATABASE_URL",
        ))
        .stdout(predicate::str::contains("Rails parses DATABASE_URL"));
}

#[test]
fn test_preflight_json_shows_managed_env_injection_plan() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        r#"[app]
name = "myapp"

[postgres]
tier = "basic"

[redis]
tier = "basic"

[services.web]
type = "web"
path = "./web"
port = 3000
ingress = "public"

[services.web.env]
managed = []

[services.api]
type = "api"
path = "./api"
port = 8000
ingress = "internal"

[services.api.env]
required = ["STRIPE_SECRET_KEY"]
managed = ["postgres", "redis"]
"#,
    )
    .unwrap();
    std::fs::create_dir(project.path().join("web")).unwrap();
    std::fs::create_dir(project.path().join("api")).unwrap();

    floo()
        .args(["--json", "preflight", project.path().to_str().unwrap()])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""env_injection_plan""#))
        .stdout(predicate::str::contains(r#""mode":"explicit""#))
        .stdout(predicate::str::contains(r#""service":"api""#))
        .stdout(predicate::str::contains(r#""handle":"postgres""#))
        .stdout(predicate::str::contains(
            r#""keys":["DATABASE_URL","PGHOST""#,
        ))
        .stdout(predicate::str::contains(
            r#""managed":[],"optional":[],"required":[],"service":"web""#,
        ));
}

#[test]
fn test_preflight_env_injection_plan_reads_services_lock() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(
        project.path().join("floo.app.toml"),
        r#"[app]
name = "myapp"

[services.api]
type = "api"
path = "./api"
port = 8000
ingress = "public"
"#,
    )
    .unwrap();
    std::fs::create_dir(project.path().join("api")).unwrap();
    std::fs::create_dir(project.path().join(".floo")).unwrap();
    std::fs::write(
        project.path().join(".floo").join("services.lock"),
        r#"{"version":1,"managed_services":[{"type":"postgres","name":"default","status":"ready","created_at":null}]}"#,
    )
    .unwrap();

    floo()
        .args(["--json", "preflight", project.path().to_str().unwrap()])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""mode":"implicit_all""#))
        .stdout(predicate::str::contains(r#""handle":"postgres""#))
        .stdout(predicate::str::contains(
            r#""keys":["DATABASE_URL","PGHOST""#,
        ));
}

// --- Dry-run matrix ---
//
// Every command that advertises --dry-run gets two assertions:
//   - JSON mode: stdout has the structured action payload (real preview)
//   - Human mode: stderr has both the standardized "Dry run" header AND a
//     "Would <verb>" preview line that names the action (no theater).
//
// The "Would" line is what blocks regression to the prior bug where dry-run
// human output was just "✓ Dry run — no changes made." with the data dropped.

#[test]
fn test_dry_run_env_set() {
    floo()
        .args([
            "--json",
            "--dry-run",
            "env",
            "set",
            "KEY=value",
            "--app",
            "test",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"env_set"#))
        .stdout(predicate::str::contains(r#""success":true"#));
}

#[test]
fn test_dry_run_env_set_human_has_preview() {
    floo()
        .args(["--dry-run", "env", "set", "KEY=value", "--app", "test"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains("Dry run"))
        .stderr(predicate::str::contains("Would set KEY on test"));
}

#[test]
fn test_dry_run_env_set_human_includes_services_and_prod_scope() {
    // Codex review: dry-run preview must reflect --services and non-default
    // --env so an operator verifying a prod or service-specific change sees
    // the real scope, not just the app name.
    floo()
        .args([
            "--dry-run",
            "env",
            "set",
            "KEY=value",
            "--app",
            "test",
            "--services",
            "api",
            "--env",
            "prod",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Would set KEY on test (services: api) (env: prod).",
        ));
}

#[test]
fn test_dry_run_env_remove_human_includes_services_and_prod_scope() {
    floo()
        .args([
            "--dry-run",
            "env",
            "remove",
            "KEY",
            "--app",
            "test",
            "--services",
            "api",
            "--env",
            "prod",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Would remove KEY from test (services: api) (env: prod).",
        ));
}

#[test]
fn test_dry_run_env_set_restart_human() {
    floo()
        .args([
            "--dry-run",
            "env",
            "set",
            "KEY=value",
            "--app",
            "test",
            "--restart",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Would set KEY on test and restart.",
        ));
}

#[test]
fn test_dry_run_env_remove() {
    floo()
        .args([
            "--json",
            "--dry-run",
            "env",
            "remove",
            "KEY",
            "--app",
            "test",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"env_remove"#));
}

#[test]
fn test_dry_run_env_remove_human_has_preview() {
    floo()
        .args(["--dry-run", "env", "remove", "KEY", "--app", "test"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains("Dry run"))
        .stderr(predicate::str::contains("Would remove KEY from test"));
}

#[test]
fn test_dry_run_env_import() {
    let dir = tempfile::TempDir::new().unwrap();
    let env_file = dir.path().join(".env");
    std::fs::write(&env_file, "KEY=value\nFOO=bar\n").unwrap();

    floo()
        .args([
            "--json",
            "--dry-run",
            "env",
            "import",
            env_file.to_str().unwrap(),
            "--app",
            "test",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"env_import"#))
        .stdout(predicate::str::contains(r#""count":2"#));
}

#[test]
fn test_dry_run_env_import_human_has_preview() {
    let dir = tempfile::TempDir::new().unwrap();
    let env_file = dir.path().join(".env");
    std::fs::write(&env_file, "KEY=value\nFOO=bar\n").unwrap();

    floo()
        .args([
            "--dry-run",
            "env",
            "import",
            env_file.to_str().unwrap(),
            "--app",
            "test",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains("Would import 2 variable(s)"))
        .stderr(predicate::str::contains("Keys: KEY, FOO"));
}

#[test]
fn test_dry_run_apps_delete() {
    floo()
        .args(["--json", "--dry-run", "apps", "delete", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"delete"#));
}

#[test]
fn test_dry_run_apps_delete_human_has_preview() {
    floo()
        .args(["--dry-run", "apps", "delete", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Would permanently delete app 'my-app'",
        ));
}

#[test]
fn test_dry_run_domains_add() {
    floo()
        .args([
            "--json",
            "--dry-run",
            "domains",
            "add",
            "example.com",
            "--app",
            "test",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"domain_add"#));
}

#[test]
fn test_dry_run_domains_add_human_has_preview() {
    floo()
        .args([
            "--dry-run",
            "domains",
            "add",
            "example.com",
            "--app",
            "test",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Would add domain example.com to test",
        ));
}

#[test]
fn test_dry_run_domains_remove() {
    floo()
        .args([
            "--json",
            "--dry-run",
            "domains",
            "remove",
            "example.com",
            "--app",
            "test",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"domain_remove"#));
}

#[test]
fn test_dry_run_domains_remove_human_has_preview() {
    floo()
        .args([
            "--dry-run",
            "domains",
            "remove",
            "example.com",
            "--app",
            "test",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Would remove domain example.com from test",
        ));
}

#[test]
fn test_dry_run_cron_run() {
    floo()
        .args([
            "--json",
            "--dry-run",
            "cron",
            "run",
            "myjob",
            "--app",
            "test",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"run_cron_job"#));
}

#[test]
fn test_dry_run_cron_run_human_has_preview() {
    floo()
        .args(["--dry-run", "cron", "run", "myjob", "--app", "test"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Would trigger cron job 'myjob' on test",
        ));
}

#[test]
fn test_dry_run_rollback() {
    floo()
        .args([
            "--json",
            "--dry-run",
            "deploys",
            "rollback",
            "my-app",
            "deploy-123",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"rollback"#));
}

#[test]
fn test_dry_run_rollback_human_has_preview() {
    floo()
        .args(["--dry-run", "deploys", "rollback", "my-app", "deploy-123"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Would roll back app 'my-app' to deploy deploy-123",
        ));
}

#[test]
fn test_dry_run_redeploy_restart() {
    floo()
        .args(["--json", "--dry-run", "redeploy", "--app", "test-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"restart"#));
}

#[test]
fn test_dry_run_redeploy_restart_human_has_preview() {
    floo()
        .args(["--dry-run", "redeploy", "--app", "test-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains("Would restart app 'test-app'"));
}

#[test]
fn test_dry_run_redeploy_rebuild() {
    floo()
        .args([
            "--json",
            "--dry-run",
            "redeploy",
            "--app",
            "test-app",
            "--rebuild",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"rebuild"#));
}

#[test]
fn test_dry_run_redeploy_rebuild_human_has_preview() {
    floo()
        .args(["--dry-run", "redeploy", "--app", "test-app", "--rebuild"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains("Would rebuild app 'test-app'"));
}

#[test]
fn test_dry_run_db_migrate() {
    floo()
        .args([
            "--json",
            "--dry-run",
            "db",
            "migrate",
            "--app",
            "test",
            "--env",
            "dev",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"db_migrate"#))
        .stdout(predicate::str::contains(r#""env":"dev"#));
}

#[test]
fn test_dry_run_db_migrate_human_has_preview() {
    floo()
        .args([
            "--dry-run",
            "db",
            "migrate",
            "--app",
            "test",
            "--env",
            "prod",
        ])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Would run pending migrations on 'test' (env: prod)",
        ));
}

#[test]
fn test_dry_run_unsupported_init() {
    floo()
        .args(["--json", "--dry-run", "init", "my-app"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("not supported"));
}

/// Regression for the prior contract bug: human dry-run output was
/// just "✓ Dry run — no changes made." with the data dropped, while JSON
/// mode emitted a real preview. Ensure no command we just fixed reverts to
/// printing the legacy "✓ Dry run — no changes made." sentinel without a
/// "Would …" preview line above it.
#[test]
fn test_dry_run_human_never_emits_legacy_no_preview_line() {
    let cmd = floo()
        .args(["--dry-run", "apps", "delete", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .success();
    let stderr = String::from_utf8(cmd.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("Would permanently delete"),
        "expected human preview, got stderr:\n{stderr}",
    );
    assert!(
        !stderr.contains("Dry run — no changes made."),
        "human dry-run output should not emit the legacy preview-less sentinel; got:\n{stderr}",
    );
}

// --- Env Import --all ---

#[test]
fn test_env_import_all_conflicts_with_file() {
    floo()
        .args(["env", "import", ".env", "--all"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn test_env_import_all_conflicts_with_services() {
    floo()
        .args(["env", "import", "--all", "--services", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

// --- Version check disabled in JSON mode ---

#[test]
fn test_json_mode_no_version_check_output() {
    // In JSON mode, no version check messages should appear on stderr
    floo()
        .args(["--json", "version"])
        .env("HOME", "/tmp/floo-test-version-check-json")
        .assert()
        .success()
        .stderr(predicate::str::contains("Update").not())
        .stderr(predicate::str::contains("downloaded").not());
}

#[test]
fn test_no_update_check_env_var() {
    // With FLOO_NO_UPDATE_CHECK set, no version check messages should appear
    floo()
        .args(["version"])
        .env("HOME", "/tmp/floo-test-no-update-check")
        .env("FLOO_NO_UPDATE_CHECK", "1")
        .assert()
        .success()
        .stderr(predicate::str::contains("Update").not())
        .stderr(predicate::str::contains("downloaded").not());
}

// --- Deploy --sync-env ---

#[test]
fn test_deploy_sync_env_help() {
    floo()
        .args(["redeploy", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--sync-env"));
}

// --- Init env file detection ---

#[test]
fn test_init_detects_floo_env() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
    std::fs::write(project.path().join(".floo.env"), "SECRET=abc\n").unwrap();

    floo()
        .args([
            "--json",
            "init",
            "myapp",
            "--path",
            project.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let app_toml = std::fs::read_to_string(project.path().join("floo.app.toml")).unwrap();
    assert!(
        app_toml.contains(r#"env_file = ".floo.env""#),
        "Expected env_file = \".floo.env\" in floo.app.toml, got:\n{app_toml}"
    );
}

#[test]
fn test_init_falls_back_to_dot_env() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
    std::fs::write(project.path().join(".env"), "KEY=value\n").unwrap();

    floo()
        .args([
            "--json",
            "init",
            "myapp",
            "--path",
            project.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let app_toml = std::fs::read_to_string(project.path().join("floo.app.toml")).unwrap();
    assert!(
        app_toml.contains(r#"env_file = ".env""#),
        "Expected env_file = \".env\" in floo.app.toml, got:\n{app_toml}"
    );
}

#[test]
fn test_init_prefers_floo_env_over_dot_env() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
    std::fs::write(project.path().join(".env"), "LOCAL=dev\n").unwrap();
    std::fs::write(project.path().join(".floo.env"), "CLOUD=prod\n").unwrap();

    floo()
        .args([
            "--json",
            "init",
            "myapp",
            "--path",
            project.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let app_toml = std::fs::read_to_string(project.path().join("floo.app.toml")).unwrap();
    assert!(
        app_toml.contains(r#"env_file = ".floo.env""#),
        "Expected .floo.env to win over .env, got:\n{app_toml}"
    );
}
