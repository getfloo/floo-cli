use assert_cmd::Command;
use predicates::prelude::*;

#[allow(deprecated)]
fn floo() -> Command {
    Command::cargo_bin("floo").unwrap()
}

// --- Help & Version ---

#[test]
fn test_help() {
    floo()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Deploy, manage, and observe web apps.",
        ));
}

#[test]
fn test_version() {
    floo()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("floo 0.1.0"));
}

#[test]
fn test_version_command_human() {
    floo()
        .arg("version")
        .assert()
        .success()
        .stderr(predicate::str::contains("floo 0.1.0"));
}

#[test]
fn test_version_command_json() {
    floo()
        .args(["--json", "version"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#))
        .stdout(predicate::str::contains(r#""version":"0.1.0""#));
}

#[test]
fn test_no_args_shows_help() {
    floo()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage: floo"));
}

// --- Auth (unauthenticated) ---

#[test]
fn test_whoami_not_authenticated() {
    floo()
        .arg("whoami")
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_whoami_json_not_authenticated() {
    floo()
        .args(["--json", "whoami"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NOT_AUTHENTICATED"#));
}

#[test]
fn test_logout_succeeds() {
    floo()
        .arg("logout")
        .env("HOME", "/tmp/floo-test-logout")
        .assert()
        .success()
        .stderr(predicate::str::contains("Logged out."));
}

#[test]
fn test_logout_json() {
    floo()
        .args(["--json", "logout"])
        .env("HOME", "/tmp/floo-test-logout-json")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""success":true"#));
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
fn test_db_help() {
    floo()
        .args(["db", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Show database details"));
}

#[test]
fn test_deploy_help() {
    floo()
        .args(["deploy", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deploy a project to Floo"));
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

#[test]
fn test_deploy_not_authenticated() {
    floo()
        .arg("deploy")
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_deploy_json_not_authenticated() {
    floo()
        .args(["--json", "deploy"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NOT_AUTHENTICATED"#));
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

// --- Database (unauthenticated) ---

#[test]
fn test_db_info_not_authenticated() {
    floo()
        .args(["db", "info", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_db_info_json_not_authenticated() {
    floo()
        .args(["--json", "db", "info", "my-app"])
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
    let config_dir = home.path().join(".floo");
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
    let config_dir = home.path().join(".floo");
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

// --- Rollbacks ---

#[test]
fn test_rollbacks_help() {
    floo()
        .args(["rollbacks", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("View deploy history"));
}

#[test]
fn test_rollbacks_list_not_authenticated() {
    floo()
        .args(["rollbacks", "list", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_rollbacks_list_json_not_authenticated() {
    floo()
        .args(["--json", "rollbacks", "list", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NOT_AUTHENTICATED"#));
}

// --- Rollback ---

#[test]
fn test_rollback_help() {
    floo()
        .args(["rollback", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Rollback to a previous deploy"));
}

#[test]
fn test_rollback_not_authenticated() {
    floo()
        .args(["rollback", "my-app", "some-deploy-id"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_rollback_json_not_authenticated() {
    floo()
        .args(["--json", "rollback", "my-app", "some-deploy-id"])
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
        .args(["logs", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_logs_json_not_authenticated() {
    floo()
        .args(["--json", "logs", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"NOT_AUTHENTICATED"#));
}
