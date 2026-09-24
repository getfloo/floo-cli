use assert_cmd::{assert::Assert, Command};
use mockito::Server;
use predicates::prelude::*;
use tempfile::TempDir;

// Each command runs in a fresh process with output modes reset, isolated
// credentials, and an explicit local API URL so no real API can be contacted.
fn billing_command(plan: Option<&str>, args: &[&str]) -> Assert {
    billing_with_org(
        serde_json::json!({
            "id": "org-1", "plan": plan, "spend_cap": null,
            "current_period_spend_cents": 0, "spend_cap_exceeded": false,
        }),
        0.0,
        args,
    )
}

fn billing_with_org(org_data: serde_json::Value, period_spend: f64, args: &[&str]) -> Assert {
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
        .with_body(org_data.to_string())
        .create();
    let _limits = server
        .mock("GET", "/v1/billing/limits")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({"plan": org_data["plan"], "max_spend_cap_cents": null}).to_string(),
        )
        .create();
    let _breakdown = server
        .mock("GET", "/v1/billing/orgs/me/cost-breakdown")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "period": {"start": "2026-09-01T00:00:00Z", "end": "2026-10-01T00:00:00Z", "label": "Selected period"},
                "total_cost_usd": period_spend, "included_cost_usd": 0.0, "apps": [],
            }).to_string(),
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
fn billing_null_plan_displays_none() {
    billing_command(None, &["usage"])
        .stdout("")
        .stderr(predicate::str::contains("Plan: none\n"));
}

#[test]
fn billing_plan_displays_the_api_plan_id() {
    billing_command(Some("team"), &["usage"])
        .stdout("")
        .stderr(predicate::str::contains("Plan: team\n"));
}

#[test]
fn billing_null_plan_is_json_null() {
    let result = billing_command(None, &["usage", "--json"]);
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"].get("plan"), Some(&serde_json::Value::Null));
    assert_eq!(
        payload["data"].get("spend_cap_policy"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(payload["data"]["deploys_blocked"], false);
}

#[test]
fn billing_null_plan_prompts_to_upgrade() {
    billing_command(None, &["spend-cap", "get"])
        .stdout("")
        .stderr(predicate::str::contains(
            "Pick a plan: floo billing upgrade\n",
        ));
}

#[test]
fn billing_paid_plan_does_not_prompt_to_upgrade() {
    billing_command(Some("paygo"), &["spend-cap", "get"])
        .stdout("")
        .stderr(predicate::str::contains("Pick a plan").not());
}

#[test]
fn billing_upgrade_json_returns_the_dashboard_billing_url() {
    let home = TempDir::new().unwrap();
    let config_dir = home.path().join(".floo-local");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"api_key":"floo_test123"}"#,
    )
    .unwrap();
    let result = Command::new(assert_cmd::cargo::cargo_bin!("floo-local"))
        .args(["billing", "upgrade", "--json"])
        .env("HOME", home.path())
        .env("FLOO_API_URL", "http://127.0.0.1:9")
        .env("FLOO_APP_URL", "https://dashboard.example.test/")
        .env_remove("FLOO_CONFIG_DIR")
        .assert()
        .success();
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(
        payload["data"]["url"],
        "https://dashboard.example.test/billing"
    );
}

#[test]
fn spend_cap_alerts_only_json_blocks_nothing() {
    let result = billing_with_org(
        serde_json::json!({
            "id": "org-1", "spend_cap": 100, "current_period_spend_cents": 200,
            "spend_cap_exceeded": true, "spend_cap_policy": "alerts_only",
        }),
        2.0,
        &["spend-cap", "get", "--json"],
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["spend_cap_exceeded"], true);
    assert_eq!(payload["data"]["spend_cap_policy"], "alerts_only");
    assert_eq!(payload["data"]["deploys_blocked"], false);
}

#[test]
fn usage_freeze_new_spend_json_blocks_deploys() {
    let result = billing_with_org(
        serde_json::json!({
            "id": "org-1", "spend_cap": 100, "current_period_spend_cents": 200,
            "spend_cap_exceeded": true, "spend_cap_policy": "freeze_new_spend",
        }),
        2.0,
        &["usage", "--json"],
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["spend_cap_exceeded"], true);
    assert_eq!(payload["data"]["spend_cap_policy"], "freeze_new_spend");
    assert_eq!(payload["data"]["deploys_blocked"], true);
}

#[test]
fn spend_cap_hard_stop_json_blocks_deploys() {
    let result = billing_with_org(
        serde_json::json!({
            "id": "org-1", "spend_cap": 100, "current_period_spend_cents": 200,
            "spend_cap_exceeded": true, "spend_cap_policy": "hard_stop",
        }),
        2.0,
        &["spend-cap", "get", "--json"],
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["spend_cap_exceeded"], true);
    assert_eq!(payload["data"]["spend_cap_policy"], "hard_stop");
    assert_eq!(payload["data"]["deploys_blocked"], true);
}

#[test]
fn usage_null_policy_json_defaults_to_hard_stop() {
    let result = billing_with_org(
        serde_json::json!({
            "id": "org-1", "spend_cap": 100, "current_period_spend_cents": 200,
            "spend_cap_exceeded": true, "spend_cap_policy": null,
        }),
        2.0,
        &["usage", "--json"],
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["spend_cap_exceeded"], true);
    assert_eq!(
        payload["data"].get("spend_cap_policy"),
        Some(&serde_json::json!(null))
    );
    assert_eq!(payload["data"]["deploys_blocked"], true);
}

#[test]
fn spend_cap_unknown_policy_json_preserves_raw_string() {
    let result = billing_with_org(
        serde_json::json!({
            "id": "org-1", "spend_cap": 100, "current_period_spend_cents": 200,
            "spend_cap_exceeded": true, "spend_cap_policy": "future_policy",
        }),
        2.0,
        &["spend-cap", "get", "--json"],
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["spend_cap_exceeded"], true);
    assert_eq!(payload["data"]["spend_cap_policy"], "future_policy");
    assert_eq!(payload["data"]["deploys_blocked"], true);
}

#[test]
fn usage_alerts_only_json_blocks_nothing() {
    let result = billing_with_org(
        serde_json::json!({
            "id": "org-1", "spend_cap": 100, "current_period_spend_cents": 200,
            "spend_cap_exceeded": true, "spend_cap_policy": "alerts_only",
        }),
        2.0,
        &["usage", "--json"],
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["spend_cap_exceeded"], true);
    assert_eq!(payload["data"]["spend_cap_policy"], "alerts_only");
    assert_eq!(payload["data"]["deploys_blocked"], false);
}

#[test]
fn spend_cap_alerts_only_human_blocks_nothing() {
    billing_with_org(serde_json::json!({
        "id": "org-1", "spend_cap": 100, "current_period_spend_cents": 200,
        "spend_cap_exceeded": true, "spend_cap_policy": "alerts_only",
    }), 2.0, &["spend-cap", "get"])
        .stdout("")
        .stderr(predicate::str::contains("Spend cap exceeded. Budget alerts only: nothing is blocked. Raise the cap: floo billing spend-cap set <amount>"))
        .stderr(predicate::str::contains("deploys are blocked").not());
}

#[test]
fn usage_alerts_only_human_blocks_nothing() {
    billing_with_org(serde_json::json!({
        "id": "org-1", "spend_cap": 100, "current_period_spend_cents": 200,
        "spend_cap_exceeded": true, "spend_cap_policy": "alerts_only",
    }), 2.0, &["usage"])
        .stdout("")
        .stderr(predicate::str::contains("Spend cap exceeded. Budget alerts only: nothing is blocked. Raise the cap: floo billing spend-cap set <amount>"))
        .stderr(predicate::str::contains("deploys are blocked").not());
}

#[test]
fn usage_historical_exceedance_does_not_block_current_deploys() {
    let result = billing_with_org(
        serde_json::json!({
            "id": "org-1", "spend_cap": 100, "spend_cap_exceeded": false,
            "spend_cap_policy": "hard_stop",
        }),
        2.0,
        &["usage", "--period", "last_month", "--json"],
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["spend_cap_exceeded"], true);
    assert_eq!(payload["data"]["deploys_blocked"], false);
}

#[test]
fn usage_partial_period_under_cap_still_reports_api_block() {
    let result = billing_with_org(
        serde_json::json!({
            "id": "org-1", "spend_cap": 100, "spend_cap_exceeded": true,
        }),
        0.5,
        &["usage", "--period", "last_7d", "--json"],
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["spend_cap_exceeded"], false);
    assert_eq!(
        payload["data"].get("spend_cap_policy"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(payload["data"]["deploys_blocked"], true);
}
