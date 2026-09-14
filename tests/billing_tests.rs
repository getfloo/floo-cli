use assert_cmd::{assert::Assert, Command};
use mockito::Server;
use predicates::prelude::*;
use tempfile::TempDir;

// Each command runs in a fresh process with output modes reset, isolated
// credentials, and an explicit local API URL so no real API can be contacted.
fn billing_command(plan: Option<&str>, args: &[&str]) -> Assert {
    let mut server = Server::new();
    let home = TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key":"floo_test123"}"#,
    )
    .unwrap();
    let org = server
        .mock("GET", "/v1/orgs/me")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "id": "org-1", "plan": plan, "spend_cap": null,
                "current_period_spend_cents": 0, "spend_cap_exceeded": false,
            })
            .to_string(),
        )
        .create();
    let _limits = server
        .mock("GET", "/v1/billing/limits")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({"plan": plan, "max_spend_cap_cents": null}).to_string())
        .create();
    let _breakdown = server
        .mock("GET", "/v1/billing/orgs/me/cost-breakdown")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"period":{"start":"2026-09-01T00:00:00Z","end":"2026-10-01T00:00:00Z","label":"This month"},"total_cost_usd":0.0,"included_cost_usd":0.0,"apps":[]}"#,
        )
        .create();

    let assertion = Command::new(assert_cmd::cargo::cargo_bin!("floo-local"))
        .arg("billing")
        .args(args)
        .env("HOME", home.path())
        .env("FLOO_API_URL", server.url())
        .env_remove("FLOO_CONFIG_DIR")
        .assert()
        .success();
    org.assert();
    assertion
}

#[test]
fn billing_null_plan_displays_no_plan_without_a_price() {
    billing_command(None, &["usage"])
        .stdout("")
        .stderr(predicate::str::contains("Plan: No plan\n"))
        .stderr(predicate::str::contains("Free").not());
}

#[test]
fn billing_legacy_free_displays_no_plan_without_a_price() {
    billing_command(Some("free"), &["usage"])
        .stdout("")
        .stderr(predicate::str::contains("Plan: No plan\n"))
        .stderr(predicate::str::contains("Free").not());
}

#[test]
fn billing_null_plan_is_json_null() {
    let result = billing_command(None, &["usage", "--json"]);
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"].get("plan"), Some(&serde_json::Value::Null));
}

#[test]
fn billing_legacy_free_is_json_null() {
    let result = billing_command(Some("free"), &["usage", "--json"]);
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"].get("plan"), Some(&serde_json::Value::Null));
}

#[test]
fn billing_null_plan_prompts_to_upgrade() {
    billing_command(None, &["spend-cap", "get"])
        .stdout("")
        .stderr(predicate::str::contains(
            "Upgrade: floo billing upgrade --plan paygo",
        ));
}

#[test]
fn billing_legacy_free_prompts_to_upgrade() {
    billing_command(Some("free"), &["spend-cap", "get"])
        .stdout("")
        .stderr(predicate::str::contains(
            "Upgrade: floo billing upgrade --plan paygo",
        ));
}

#[test]
fn billing_paid_plan_does_not_prompt_to_upgrade() {
    billing_command(Some("paygo"), &["spend-cap", "get"])
        .stdout("")
        .stderr(predicate::str::contains("Upgrade:").not());
}

fn billing_checkout(plan: Option<&str>, args: &[&str]) -> Assert {
    let mut server = Server::new();
    let home = TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key":"floo_test123"}"#,
    )
    .unwrap();
    let checkout = server
        .mock("POST", "/v1/billing/checkout")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({"upgraded": true, "plan": plan}).to_string())
        .create();

    let assertion = Command::new(assert_cmd::cargo::cargo_bin!("floo-local"))
        .args(["billing", "upgrade"])
        .args(args)
        .env("HOME", home.path())
        .env("FLOO_API_URL", server.url())
        .env_remove("FLOO_CONFIG_DIR")
        .assert()
        .success();
    checkout.assert();
    assertion
}

#[test]
fn billing_checkout_null_plan_displays_no_plan() {
    billing_checkout(None, &[])
        .stdout("")
        .stderr(predicate::str::contains("No plan"));
}

#[test]
fn billing_checkout_legacy_free_is_json_null() {
    let result = billing_checkout(Some("free"), &["--json"]);
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["upgraded"], true);
    assert_eq!(payload["data"].get("plan"), Some(&serde_json::Value::Null));
}
