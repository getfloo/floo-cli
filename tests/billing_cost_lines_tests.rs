use assert_cmd::{assert::Assert, Command};
use mockito::{Matcher, Server};
use predicates::prelude::*;
use serde_json::{json, Value};
use tempfile::TempDir;

const APP_ID: &str = "11111111-1111-4111-8111-111111111111";
const APP_PATH: &str = "/v1/billing/apps/11111111-1111-4111-8111-111111111111/cost-breakdown";

fn app_fixture() -> Value {
    // API test_mixed_versions_keep_recorded_rates_and_existing_app_fields
    // in api/tests/test_cost_lines.py (getfloo/floo#2941), with fixed UUIDs.
    serde_json::from_str(include_str!("fixtures/billing_cost_breakdown.json")).unwrap()
}

fn org_fixture() -> Value {
    let app = app_fixture();
    json!({
        "period": app["period"], "total_cost_usd": 0.10, "included_cost_usd": 0.0,
        "apps": [{"app_id": APP_ID, "name": "line-app", "total_cost_usd": 0.096}],
        "lines": app["services"][0]["lines"],
    })
}

fn billing_response(path: &str, response: Value, args: &[&str]) -> Assert {
    // Fresh subprocesses reset output modes; all HTTP uses this local fake.
    let mut server = Server::new();
    let home = TempDir::new().unwrap();
    let config = home.path().join(".floo-local");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.json"), r#"{"api_key":"floo_test123"}"#).unwrap();
    let _org = server
        .mock("GET", "/v1/orgs/me")
        .with_body(r#"{"id":"org-1","spend_cap_exceeded":false}"#)
        .create();
    let _limits = server
        .mock("GET", "/v1/billing/limits")
        .with_body(r#"{"plan":null,"max_spend_cap_cents":null}"#)
        .create();
    let _app = server
        .mock("GET", format!("/v1/apps/{APP_ID}").as_str())
        .with_body(json!({"id": APP_ID, "name": "line-app"}).to_string())
        .create();
    let breakdown = server
        .mock("GET", path)
        .match_query(Matcher::UrlEncoded("period".into(), "current_month".into()))
        .with_body(response.to_string())
        .create();
    let assertion = Command::new(assert_cmd::cargo::cargo_bin!("floo-local"))
        .arg("billing")
        .args(args)
        .env("HOME", home.path())
        .env("FLOO_API_URL", server.url())
        .env_remove("FLOO_CONFIG_DIR")
        .assert()
        .success();
    breakdown.assert();
    assertion
}

#[test]
fn usage_json_keeps_both_rate_versions_and_all_recorded_evidence() {
    let fixture = org_fixture();
    let result = billing_response(
        "/v1/billing/orgs/me/cost-breakdown",
        fixture.clone(),
        &["usage", "--json"],
    );
    // Parsing all stdout also rejects a second JSON object or human prose.
    let payload: Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"]["lines"], fixture["lines"]);
    assert_eq!(payload["data"]["lines"].as_array().unwrap().len(), 2);
    assert_eq!(payload["data"]["total_cost_usd"], 0.10);
    result.stderr("");
}

#[test]
fn usage_human_shows_both_rates_quantities_versions_and_amounts_on_stderr() {
    billing_response(
        "/v1/billing/orgs/me/cost-breakdown",
        org_fixture(),
        &["usage"],
    )
    .stdout("")
    .stderr(predicate::str::contains("Organization cost lines:"))
    .stderr(predicate::str::contains(
        "Quantity: 1000 vCPU-second | Rate: $0.216 / vCPU-hour | Amount: $0.06",
    ))
    .stderr(predicate::str::contains(
        "Quantity: 1000 vCPU-second | Rate: $0.1296 / vCPU-hour | Amount: $0.036",
    ))
    .stderr(predicate::str::contains(
        "Rate card: floo-credits-2026-09-13",
    ))
    .stderr(predicate::str::contains(
        "Rate card: floo-credits-2026-09-18",
    ));
}

#[test]
fn app_json_preserves_the_api_shape_and_ignores_future_fields() {
    let expected = app_fixture();
    let mut response = expected.clone();
    response["future_field"] = json!(true);
    response["services"][0]["lines"][0]["future_field"] = json!("new metadata");
    let result = billing_response(
        APP_PATH,
        response,
        &["cost-breakdown", "--app", APP_ID, "--json"],
    );
    let payload: Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"], expected);
    result.stderr("");
}

#[test]
fn app_human_shows_service_attribution_and_both_rate_versions() {
    billing_response(
        APP_PATH,
        app_fixture(),
        &["cost-breakdown", "--app", APP_ID],
    )
    .stdout("")
    .stderr(predicate::str::contains("Service: web — $0.10\n"))
    .stderr(predicate::str::contains(
        "Rate card: floo-credits-2026-09-13",
    ))
    .stderr(predicate::str::contains(
        "Rate card: floo-credits-2026-09-18",
    ));
}

fn legacy_managed_fixture() -> Value {
    let mut app = app_fixture();
    app["services"] = json!([]);
    app["total_cost_usd"] = json!(0.000000009);
    app["managed_resources"] = json!([{
        "managed_service_id": null, "environment_managed_resource_id": APP_ID,
        "name": "database", "environment": "production", "costs": {"storage": {"cost_usd": 0.000000009}},
        "total_usd": 0.000000009,
        "lines": [{"rate_key": null, "label": null, "quantity": 42.0, "unit": "GiB-second",
            "display_unit": null, "rate": null, "display_rate": null, "rate_card_version": null,
            "effective_from": null, "cost_usd": 0.000000009}]
    }]);
    app
}

#[test]
fn managed_json_preserves_null_metadata_and_nanodollar_amounts() {
    let expected = legacy_managed_fixture();
    let result = billing_response(
        APP_PATH,
        expected.clone(),
        &["cost-breakdown", "--app", APP_ID, "--json"],
    );
    let payload: Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(payload["data"], expected);
    result.stderr("");
}

#[test]
fn managed_human_shows_environment_and_unknown_rates_without_rounding_away_cost() {
    billing_response(
        APP_PATH,
        legacy_managed_fixture(),
        &["cost-breakdown", "--app", APP_ID],
    )
    .stdout("")
    .stderr(predicate::str::contains(
        "Managed resource: database (production)",
    ))
    .stderr(predicate::str::contains(
        "Rate: unknown / GiB-second | Amount: <$0.0001",
    ))
    .stderr(predicate::str::contains(
        "Rate card: unknown | Effective from: unknown",
    ));
}

#[test]
fn older_app_response_without_lines_still_shows_resource_costs() {
    let mut app = app_fixture();
    app["services"][0].as_object_mut().unwrap().remove("lines");
    billing_response(APP_PATH, app, &["cost-breakdown", "--app", APP_ID])
        .stdout("")
        .stderr(predicate::str::contains("compute: $0.10\n"));
}

#[test]
fn managed_rates_use_display_prices_and_units_without_credit_rates() {
    let mut app = legacy_managed_fixture();
    app["managed_resources"][0]["lines"] = json!([
        {"quantity": 730.0, "unit": "GiB-hour", "rate": 0.000479,
         "display_rate": 0.35, "display_unit": "GB-month", "cost_usd": 0.35},
        {"quantity": 1.0, "unit": "GiB", "rate": 0.161061,
         "display_rate": 0.15, "display_unit": "GB", "cost_usd": 0.15}
    ]);
    billing_response(APP_PATH, app, &["cost-breakdown", "--app", APP_ID])
        .stdout("")
        .stderr(predicate::str::contains("Rate: $0.35 / GB-month"))
        .stderr(predicate::str::contains("Rate: $0.15 / GB"))
        .stderr(predicate::str::contains("credits/").not());
}

#[test]
fn incomplete_display_metadata_falls_back_to_recorded_rate_and_unit() {
    let mut app = app_fixture();
    app["services"][0]["lines"][0]["display_rate"] = Value::Null;
    app["services"][0]["lines"][1]["display_unit"] = Value::Null;
    billing_response(APP_PATH, app, &["cost-breakdown", "--app", APP_ID])
        .stdout("")
        .stderr(predicate::str::contains("Rate: <$0.0001 / vCPU-second").count(2))
        .stderr(predicate::str::contains(" / vCPU-hour").not());
}

#[test]
fn money_rounding_hides_float_noise_and_limits_line_precision() {
    let mut app = app_fixture();
    app["total_cost_usd"] = json!(0.30000000000000004);
    app["unattributed_cost_usd"] = json!(0.30000000000000004);
    app["services"][0]["total_usd"] = json!(0.30000000000000004);
    app["services"][0]["costs"]["compute"]["cost_usd"] = json!(0.30000000000000004);
    app["services"][0]["lines"][0]["cost_usd"] = json!(0.30000000000000004);
    app["services"][0]["lines"][0]["display_rate"] = json!(0.30000000000000004);
    app["services"][0]["lines"][1]["cost_usd"] = json!(0.123456);
    app["services"][0]["lines"][1]["display_rate"] = json!(0.123456);
    billing_response(APP_PATH, app, &["cost-breakdown", "--app", APP_ID])
        .stdout("")
        .stderr(predicate::str::contains("Total cost: $0.30\n"))
        .stderr(predicate::str::contains("Service: web — $0.30\n"))
        .stderr(predicate::str::contains("compute: $0.30\n"))
        .stderr(predicate::str::contains("Unattributed cost: $0.30\n"))
        .stderr(predicate::str::contains(
            "Rate: $0.3 / vCPU-hour | Amount: $0.3\n",
        ))
        .stderr(predicate::str::contains(
            "Rate: $0.1235 / vCPU-hour | Amount: $0.1235\n",
        ))
        .stderr(predicate::str::contains("0.30000000000000004").not());
}

#[test]
fn zero_and_minimum_line_amounts_are_not_reported_as_below_threshold() {
    let mut app = app_fixture();
    app["services"][0]["lines"][0]["cost_usd"] = json!(0.0);
    app["services"][0]["lines"][0]["display_rate"] = json!(0.0);
    app["services"][0]["lines"][1]["cost_usd"] = json!(0.0001);
    app["services"][0]["lines"][1]["display_rate"] = json!(0.0001);
    billing_response(APP_PATH, app, &["cost-breakdown", "--app", APP_ID])
        .stdout("")
        .stderr(predicate::str::contains(
            "Rate: $0 / vCPU-hour | Amount: $0\n",
        ))
        .stderr(predicate::str::contains(
            "Rate: $0.0001 / vCPU-hour | Amount: $0.0001\n",
        ));
}

#[test]
fn service_and_managed_resource_costs_are_displayed_alphabetically() {
    let mut app = legacy_managed_fixture();
    app["services"] = app_fixture()["services"].clone();
    app["services"][0]["costs"] =
        json!({"storage": {"cost_usd": 0.2}, "compute": {"cost_usd": 0.1}});
    app["managed_resources"][0]["costs"] =
        json!({"storage": {"cost_usd": 0.2}, "compute": {"cost_usd": 0.1}});
    billing_response(APP_PATH, app, &["cost-breakdown", "--app", APP_ID])
        .stdout("")
        .stderr(predicate::str::contains("    compute: $0.10\n    storage: $0.20\n").count(2));
}
