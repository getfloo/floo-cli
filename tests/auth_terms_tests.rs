use assert_cmd::assert::OutputAssertExt;
use mockito::{Matcher, Mock, Server};
use predicates::prelude::*;
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use tempfile::TempDir;

mod support;

// Fresh CLI processes reset output modes; credentials and HTTP stay isolated.
fn command(server: &Server) -> (TempDir, Command) {
    let home = TempDir::new().unwrap();
    std::fs::write(
        home.path().join("config.json"),
        r#"{"api_key":"floo_test123"}"#,
    )
    .unwrap();
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("floo-local"));
    command
        .arg("auth")
        .env("FLOO_CONFIG_DIR", home.path())
        .env("FLOO_API_URL", server.url())
        .env("FLOO_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::null());
    (home, command)
}

fn profile(accepted: Option<&str>) -> Value {
    json!({
        "email": "person@example.test", "name": null,
        "current_terms_version": "2099-02-03", "terms_accepted_version": accepted,
    })
}

fn whoami(server: &mut Server, profile: Value) -> Mock {
    server
        .mock("GET", "/v1/auth/whoami")
        .match_header("authorization", "Bearer floo_test123")
        .with_body(profile.to_string())
        .create()
}

fn acceptance(server: &mut Server) -> Mock {
    server
        .mock("POST", "/v1/auth/terms/accept")
        .match_header("authorization", "Bearer floo_test123")
        .match_body(Matcher::Json(json!({"version": "2099-02-03"})))
        .with_body(profile(Some("2099-02-03")).to_string())
}

#[test]
fn explicit_yes_posts_the_server_version_and_returns_one_json_document() {
    let mut server = Server::new();
    let whoami = whoami(&mut server, profile(Some("older-version")));
    let post = acceptance(&mut server).create();
    let (_home, mut command) = command(&server);
    let result = command
        .args(["accept-terms", "--yes", "--json"])
        .assert()
        .success();
    let payload: Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["terms_accepted_version"], "2099-02-03");
    assert_eq!(payload["data"]["acceptance_required"], false);
    assert_eq!(
        payload["data"]["terms_url"],
        "https://getfloo.com/legal/terms"
    );
    assert_eq!(
        payload["data"]["privacy_url"],
        "https://getfloo.com/legal/privacy"
    );
    whoami.assert();
    post.assert();
}

#[test]
fn already_accepted_does_not_post_or_require_confirmation() {
    let mut server = Server::new();
    let whoami = whoami(&mut server, profile(Some("2099-02-03")));
    let post = acceptance(&mut server).expect(0).create();
    let (_home, mut command) = command(&server);
    command
        .arg("accept-terms")
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains("Already accepted"));
    whoami.assert();
    post.assert();
}

#[test]
fn non_tty_without_yes_fails_and_names_the_recovery_command() {
    let mut server = Server::new();
    let whoami = whoami(&mut server, profile(None));
    let post = acceptance(&mut server).expect(0).create();
    let (_home, mut command) = command(&server);
    command
        .arg("accept-terms")
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("floo auth accept-terms --yes"))
        .stderr(predicate::str::contains("https://getfloo.com/legal/terms"))
        .stderr(predicate::str::contains(
            "https://getfloo.com/legal/privacy",
        ))
        .stderr(predicate::str::contains("2099-02-03"))
        .stderr(predicate::str::contains("Do you agree").not());
    whoami.assert();
    post.assert();
}

#[test]
fn api_key_login_defers_acceptance_and_preserves_credentials_in_json_mode() {
    let mut server = Server::new();
    let whoami = whoami(&mut server, profile(None));
    let post = acceptance(&mut server).expect(0).create();
    let (home, mut command) = command(&server);
    let result = command
        .args(["login", "--api-key", "floo_test123", "--json"])
        .assert()
        .success();
    let payload: Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["success"], true);
    assert_eq!(payload["data"]["terms"]["acceptance_required"], true);
    assert!(payload["data"]["terms_acceptance_error"]["suggestion"]
        .as_str()
        .unwrap()
        .contains("floo auth accept-terms --yes"));
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(home.path().join("config.json")).unwrap())
            .unwrap();
    assert_eq!(config["api_key"], "floo_test123");
    assert!(config.get("terms_accepted_version").is_none());
    whoami.assert();
    post.assert();
}

#[cfg(unix)]
#[test]
fn interactive_y_accepts_after_showing_both_policies() {
    let mut server = Server::new();
    let whoami = whoami(&mut server, profile(None));
    let post = acceptance(&mut server).create();
    let (_home, mut command) = command(&server);
    let (mut master, slave) = support::stdout_terminal();
    std::io::Write::write_all(&mut master, b"y\n").unwrap();
    command
        .arg("accept-terms")
        .stdin(slave)
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains("https://getfloo.com/legal/terms"))
        .stderr(predicate::str::contains(
            "https://getfloo.com/legal/privacy",
        ))
        .stderr(predicate::str::contains(
            "Do you agree to the floo Terms of Service and Privacy Policy? [y/N]",
        ));
    whoami.assert();
    post.assert();
}

#[cfg(unix)]
#[test]
fn declining_during_login_does_not_post_or_fail_login() {
    let mut server = Server::new();
    let whoami = whoami(&mut server, profile(None));
    let post = acceptance(&mut server).expect(0).create();
    let (_home, mut command) = command(&server);
    let (mut master, slave) = support::stdout_terminal();
    std::io::Write::write_all(&mut master, b"\n").unwrap();
    command
        .arg("login")
        .stdin(slave)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "account cannot be used until you accept",
        ));
    whoami.assert();
    post.assert();
}

#[cfg(unix)]
#[test]
fn json_mode_never_prompts_even_with_a_tty() {
    let mut server = Server::new();
    let whoami = whoami(&mut server, profile(None));
    let post = acceptance(&mut server).expect(0).create();
    let (_home, mut command) = command(&server);
    let (mut master, slave) = support::stdout_terminal();
    std::io::Write::write_all(&mut master, b"y\n").unwrap();
    let result = command
        .args(["accept-terms", "--json"])
        .stdin(slave)
        .assert()
        .failure();
    let payload: Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["error"]["code"], "CONFIRMATION_REQUIRED");
    whoami.assert();
    post.assert();
}

#[cfg(unix)]
#[test]
fn missing_whoami_fields_skip_the_login_prompt_on_a_tty() {
    let mut server = Server::new();
    let whoami = whoami(&mut server, json!({"email": "person@example.test"}));
    let post = acceptance(&mut server).expect(0).create();
    let (_home, mut command) = command(&server);
    let (mut master, slave) = support::stdout_terminal();
    std::io::Write::write_all(&mut master, b"y\n").unwrap();
    command
        .arg("login")
        .stdin(slave)
        .assert()
        .success()
        .stderr(predicate::str::contains("Do you agree").not())
        .stderr(predicate::str::contains("accept-terms").not());
    whoami.assert();
    post.assert();
}
