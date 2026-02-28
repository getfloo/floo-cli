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
        .args(["rollbacks", "list", "--app", "my-app"])
        .env("HOME", "/tmp/floo-test-nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in."));
}

#[test]
fn test_rollbacks_list_json_not_authenticated() {
    floo()
        .args(["--json", "rollbacks", "list", "--app", "my-app"])
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
    let config_dir = home.path().join(".floo");
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
        .args(["deploy", project.path().to_str().unwrap()])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains("no longer supported"));
}

#[test]
fn test_deploy_legacy_floo_toml_json() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo");
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
        .args(["--json", "deploy", project.path().to_str().unwrap()])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""code":"LEGACY_CONFIG"#));
}

#[test]
fn test_deploy_invalid_service_config() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo");
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
        .args(["deploy", project.path().to_str().unwrap()])
        .env("HOME", home.path().to_str().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid floo.service.toml"));
}

#[test]
fn test_deploy_invalid_service_config_json() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".floo");
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
        .args(["--json", "deploy", project.path().to_str().unwrap()])
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
    let config_dir = home.path().join(".floo");
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
        .args(["deploy", project.path().to_str().unwrap()])
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
    let config_dir = home.path().join(".floo");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key": "floo_test123", "api_url": "https://api.test.local"}"#,
    )
    .unwrap();

    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name":"test"}"#).unwrap();

    floo()
        .args(["--json", "deploy", project.path().to_str().unwrap()])
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

    // Verify files were created
    assert!(project.path().join("floo.app.toml").exists());
    assert!(project.path().join("floo.service.toml").exists());
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
