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
fn billing_upgrade_json_returns_the_org_scoped_dashboard_billing_url() {
    let mut server = Server::new();
    let org = server
        .mock("GET", "/v1/orgs/me")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({"id": "org-b", "plan": null}).to_string())
        .create();
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
        .env("FLOO_API_URL", server.url())
        .env("FLOO_APP_URL", "https://dashboard.example.test/")
        .env_remove("FLOO_CONFIG_DIR")
        .assert()
        .success();
    org.assert();
    let payload: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(
        payload["data"]["url"],
        "https://dashboard.example.test/billing?org_id=org-b"
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

#[test]
fn prepaid_balance_preserves_money_and_activation_without_claiming_authority() {
    for (amount, pending, activation, eligible, expected) in [
        ("25000000000", "1000000000", "active", true, "$25.00"),
        (
            "9007199254740993",
            "0",
            "active",
            true,
            "$9007199.254740993",
        ),
        ("-1", "0", "active", true, "-$0.000000001"),
        ("0", "0", "dormant", false, "$0.00"),
        ("25000000000", "0", "scheduled", true, "$25.00"),
    ] {
        for json in [false, true] {
            let mut server = Server::new();
            let home = TempDir::new().unwrap();
            let config_dir = home.path().join(".floo-local");
            std::fs::create_dir_all(&config_dir).unwrap();
            std::fs::write(
                config_dir.join("config.json"),
                r#"{"api_key":"floo_test123","default_org":"org-b"}"#,
            )
            .unwrap();
            let body = serde_json::json!({
                "billing_mode": "prepaid", "funding_eligible": eligible,
                "activation": activation, "usage_started_at": null,
                "balance_nanodollars": amount, "pending_nanodollars": pending,
                "recorded_at": "2026-09-26T12:00:00Z",
            });
            let read = server
                .mock("GET", "/v1/billing/paygo/balance")
                .match_header("authorization", "Bearer floo_test123")
                .match_header("x-floo-org-id", "org-b")
                .with_header("content-type", "application/json")
                .with_body(body.to_string())
                .create();
            let mut command = Command::new(assert_cmd::cargo::cargo_bin!("floo-local"));
            command
                .args(["billing", "balance"])
                .env("HOME", home.path())
                .env("FLOO_API_URL", server.url())
                .env_remove("FLOO_CONFIG_DIR");
            if json {
                command.arg("--json");
            }
            let result = command.assert().success();
            read.assert();
            if json {
                let payload: serde_json::Value =
                    serde_json::from_slice(&result.get_output().stdout).unwrap();
                assert_eq!(payload["data"], body);
                assert!(payload["data"].get("deploys_blocked").is_none());
            } else {
                result
                    .stdout("")
                    .stderr(predicate::str::contains(expected))
                    .stderr(predicate::str::contains(match activation {
                        "dormant" => "Prepaid billing: not activated",
                        "scheduled" => "Prepaid billing: activation scheduled",
                        _ => "Prepaid billing: active",
                    }))
                    .stderr(predicate::str::contains(if eligible {
                        "Add funds: floo billing upgrade"
                    } else {
                        "Prepaid funding is unavailable for this organization."
                    }))
                    .stderr(predicate::str::contains(if pending == "0" {
                        "Pending funding: $0.00 (not yet available)"
                    } else {
                        "Pending funding: $1.00 (not yet available)"
                    }))
                    .stderr(predicate::str::contains("floo billing upgrade"))
                    .stderr(predicate::str::contains("floo billing spend-cap get"))
                    .stderr(predicate::str::contains(
                        "Spend authority and recovery status are unavailable.",
                    ));
            }
        }
    }
}

#[test]
fn prepaid_balance_errors_never_become_zero_money() {
    for (status, body) in [
        (
            403,
            serde_json::json!({"detail":{"code":"INSUFFICIENT_KEY_SCOPE","message":"Read scope required"}}),
        ),
        (
            200,
            serde_json::json!({"activation":"active","funding_eligible":true}),
        ),
        (
            200,
            serde_json::json!({"activation":"active","funding_eligible":true,"balance_nanodollars":"NaN","pending_nanodollars":"0"}),
        ),
        (
            200,
            serde_json::json!({
                "activation":"active", "funding_eligible":true,
                "balance_nanodollars":"25000000000", "pending_nanodollars":"0",
                "authority": {
                    "state":"available", "binding_authority":"funds", "balance_sequence":1,
                    "funds_remaining_nanodollars":"NaN", "period_usage_nanodollars":"0",
                    "hard_limit_nanodollars":"50000000000", "remaining_nanodollars":"25000000000",
                    "observed_at":"2026-09-29T02:00:00Z"
                }
            }),
        ),
    ] {
        let mut server = Server::new();
        let home = TempDir::new().unwrap();
        let config_dir = home.path().join(".floo-local");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.json"),
            r#"{"api_key":"floo_test123"}"#,
        )
        .unwrap();
        let read = server
            .mock("GET", "/v1/billing/paygo/balance")
            .with_status(status)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create();
        let result = Command::new(assert_cmd::cargo::cargo_bin!("floo-local"))
            .args(["billing", "balance", "--json"])
            .env("HOME", home.path())
            .env("FLOO_API_URL", server.url())
            .env_remove("FLOO_CONFIG_DIR")
            .assert()
            .failure();
        read.assert();
        let payload: serde_json::Value =
            serde_json::from_slice(&result.get_output().stdout).unwrap();
        assert_eq!(payload["success"], false);
        assert!(payload["data"].get("balance_nanodollars").is_none());
    }
}

#[test]
fn prepaid_recovery_preserves_server_evidence_and_distinguishes_provider_progress() {
    for (state, cause, desired, applied, expected) in [
        (
            "exhausted",
            "funds",
            true,
            1,
            "Workload stops are still in progress.",
        ),
        (
            "exhausted",
            "hard_limit",
            true,
            2,
            "Adding funds does not raise it",
        ),
        (
            "unknown",
            "meter_freshness",
            false,
            2,
            "adding funds will not resolve this delay",
        ),
        (
            "available",
            "funds",
            false,
            1,
            "Workload recovery is still in progress.",
        ),
        (
            "available",
            "funds",
            false,
            2,
            "Billing restrictions have been cleared.",
        ),
        (
            "available",
            "funds",
            true,
            2,
            "recovery is awaiting the next check",
        ),
        (
            "exhausted",
            "hard_limit",
            false,
            1,
            "Workload status is awaiting the next billing check.",
        ),
    ] {
        let mut server = Server::new();
        let home = TempDir::new().unwrap();
        std::fs::create_dir_all(home.path().join(".floo-local")).unwrap();
        std::fs::write(
            home.path().join(".floo-local/config.json"),
            r#"{"api_key":"floo_test123"}"#,
        )
        .unwrap();
        let body = serde_json::json!({
            "billing_mode":"prepaid", "funding_eligible":true, "activation":"active",
            "usage_started_at":null, "recorded_at":null,
            "balance_nanodollars":"9007199254740993", "pending_nanodollars":"0",
            "authority": {
                "state":state, "binding_authority":cause, "balance_sequence":9,
                "funds_remaining_nanodollars":"9007199254740993", "period_usage_nanodollars":"1",
                "hard_limit_nanodollars":"25000000000", "remaining_nanodollars":"24999999999",
                "observed_at":"2026-09-29T02:00:00Z"
            },
            "enforcement": {"desired_blocked":desired, "generation":2, "applied_generation":applied}
        });
        let read = server
            .mock("GET", "/v1/billing/paygo/balance")
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .expect(2)
            .create();
        for json in [false, true] {
            let mut command = Command::new(assert_cmd::cargo::cargo_bin!("floo-local"));
            command
                .args(["billing", "balance"])
                .env("HOME", home.path())
                .env("FLOO_API_URL", server.url())
                .env_remove("FLOO_CONFIG_DIR");
            if json {
                command.arg("--json");
            }
            let result = command.assert().success();
            if json {
                let payload: serde_json::Value =
                    serde_json::from_slice(&result.get_output().stdout).unwrap();
                assert_eq!(payload["data"], body);
            } else {
                result
                    .stdout("")
                    .stderr(predicate::str::contains(expected))
                    .stderr(predicate::str::contains(
                        "Current spend headroom: $24.999999999",
                    ));
            }
        }
        read.assert();
    }
}

#[test]
fn billing_notifications_preserve_evidence_and_cursor_in_both_output_modes() {
    for json in [false, true] {
        let mut server = Server::new();
        let home = TempDir::new().unwrap();
        let config_dir = home.path().join(".floo-local");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.json"),
            r#"{"api_key":"floo_test123","default_org":"org-b"}"#,
        )
        .unwrap();
        let body = serde_json::json!({"records":[{
            "id":"notice-1", "state":"imminent", "subject":"Low balance",
            "body":"Add funds to keep your apps running.",
            "first_attempt_at":"2026-09-29T00:00:00Z", "last_attempt_at":"2026-09-29T00:00:00Z",
            "provider_accepted":false, "closed_reason":"acceptance_unresolved"
        }], "next_before_id":"notice-1"});
        let read = server
            .mock("GET", "/v1/billing/paygo/notifications")
            .match_header("authorization", "Bearer floo_test123")
            .match_header("x-floo-org-id", "org-b")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("limit".into(), "1".into()),
                mockito::Matcher::UrlEncoded("before_id".into(), "cursor&limit=100".into()),
            ]))
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create();
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("floo-local"));
        command
            .args([
                "billing",
                "notifications",
                "--limit",
                "1",
                "--before-id",
                "cursor&limit=100",
            ])
            .env("HOME", home.path())
            .env("FLOO_API_URL", server.url())
            .env_remove("FLOO_CONFIG_DIR");
        if json {
            command.arg("--json");
        }
        let result = command.assert().success();
        if json {
            let payload: serde_json::Value =
                serde_json::from_slice(&result.get_output().stdout).unwrap();
            assert_eq!(payload["data"], body);
        } else {
            result.stdout("").stderr(
                predicate::str::contains("Submission outcome unknown.")
                    .and(predicate::str::contains(
                        "Provider acceptance does not confirm inbox delivery.",
                    ))
                    .and(predicate::str::contains(
                        "Last attempted: 2026-09-29T00:00:00Z",
                    ))
                    .and(predicate::str::contains("--before-id notice-1 --limit 1")),
            );
        }
        read.assert();
    }
}
