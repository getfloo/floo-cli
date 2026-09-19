use std::{collections::BTreeMap, process};

use crate::api_types::{CostLine, ResourceCost, SpendCapPolicy};
use crate::errors::ErrorCode;
use crate::output;

struct SpendCapStatus {
    deploys_blocked: bool,
    message: Option<&'static str>,
}

fn spend_cap_status(exceeded: bool, policy: Option<&SpendCapPolicy>) -> SpendCapStatus {
    if !exceeded {
        return SpendCapStatus {
            deploys_blocked: false,
            message: None,
        };
    }
    let message = match policy {
        Some(SpendCapPolicy::AlertsOnly) => "Spend cap exceeded. Budget alerts only: nothing is blocked. Raise the cap: floo billing spend-cap set <amount>",
        Some(SpendCapPolicy::FreezeNewSpend) => "Spend cap exceeded. New deploys are paused until you raise the cap or the next billing period begins. Raise the cap: floo billing spend-cap set <amount>",
        // Missing policies default to hard_stop; unknown policies use the same
        // conservative messaging until the CLI understands their behavior.
        Some(SpendCapPolicy::HardStop | SpendCapPolicy::Unknown(_)) | None => "Spend cap exceeded. Deploys are blocked and running services are scaled to zero until you raise the cap or the next billing period begins. Raise the cap: floo billing spend-cap set <amount>",
    };
    SpendCapStatus {
        deploys_blocked: !matches!(policy, Some(SpendCapPolicy::AlertsOnly)),
        message: Some(message),
    }
}

fn canonical_plan(plan: Option<&str>) -> Option<&str> {
    match plan {
        // Older APIs use "free" for an org that has no plan yet.
        Some("free") => None,
        Some("hobby" | "pro") => Some("paygo"),
        current => current,
    }
}

fn plan_display(plan: Option<&str>) -> (&'static str, Option<&'static str>) {
    match canonical_plan(plan) {
        Some("paygo") => ("Pay as you go", Some("$0 commitment")),
        Some("team") => ("Team", Some("$250/mo")),
        Some("enterprise") => ("Enterprise", Some("Custom")),
        Some(_) => ("Unknown plan", None),
        None => ("No plan", None),
    }
}

pub fn upgrade(plan: Option<String>) {
    super::require_auth();
    let client = super::init_client(None);

    match client.create_billing_checkout(plan.as_deref()) {
        Ok(result) => {
            if result.upgraded {
                let plan_name = canonical_plan(result.plan.as_deref());
                if output::is_json_mode() {
                    output::success(
                        "",
                        Some(serde_json::json!({"upgraded": true, "plan": plan_name})),
                    );
                } else {
                    output::success(
                        &format!("Upgraded to {}", plan_name.unwrap_or("No plan")),
                        None,
                    );
                }
            } else if let Some(url) = &result.url {
                if output::is_json_mode() {
                    output::success("", Some(serde_json::json!({"url": url})));
                } else {
                    output::info("Opening billing page in browser...", None);
                    if open::that(url).is_err() {
                        output::warn(&format!("Open this URL manually: {url}"));
                    }
                }
            }
        }
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    }
}

pub fn contact() {
    if output::is_json_mode() {
        output::success(
            "",
            Some(serde_json::json!({
                "email": "sales@getfloo.com",
                "subject": "Enterprise inquiry",
            })),
        );
    } else {
        eprintln!("  Enterprise & custom plans: sales@getfloo.com");
        eprintln!("  Subject: Enterprise inquiry");
    }
}

pub fn spend_cap_get() {
    super::require_auth();
    let client = super::init_client(None);

    let org = match client.get_org_me() {
        Ok(o) => o,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let spend_cap = org.spend_cap;
    let current_spend = org.current_period_spend_cents.unwrap_or_else(|| {
        output::error(
            "Response missing 'current_period_spend_cents' field.",
            &ErrorCode::ParseError,
            Some("This is a bug. Please report it."),
        );
        process::exit(1);
    });
    let exceeded = org.spend_cap_exceeded.unwrap_or_else(|| {
        output::error(
            "Response missing 'spend_cap_exceeded' field.",
            &ErrorCode::ParseError,
            Some("This is a bug. Please report it."),
        );
        process::exit(1);
    });
    let cap_status = spend_cap_status(exceeded, org.spend_cap_policy.as_ref());

    // Cap is cents-denominated; emit it as `spend_cap_cents` to match
    // `billing usage` and every other *_cents field. One key per concept so
    // agents never special-case `spend_cap` vs `spend_cap_cents` (#1161).
    let data = serde_json::json!({
        "spend_cap_cents": spend_cap,
        "current_period_spend_cents": current_spend,
        "spend_cap_exceeded": exceeded,
        "spend_cap_policy": org.spend_cap_policy,
        "deploys_blocked": cap_status.deploys_blocked,
    });

    if output::is_json_mode() {
        output::success("", Some(data));
        return;
    }

    match spend_cap {
        Some(cents) if cents > 0 => {
            eprintln!("  Spend cap: ${:.2}/month", cents as f64 / 100.0)
        }
        _ => eprintln!("  Spend cap: none (unlimited)"),
    }
    eprintln!("  Current spend: ${:.2}", current_spend as f64 / 100.0);
    if let Some(message) = cap_status.message {
        output::warn(message);
    }
    if canonical_plan(org.plan.as_deref()).is_none() {
        eprintln!("  Upgrade: floo billing upgrade --plan paygo");
    }
}

pub fn spend_cap_set(amount: f64) {
    super::require_auth();
    let client = super::init_client(None);

    if !amount.is_finite() || !(0.0..=1_000_000.0).contains(&amount) {
        output::error(
            "Spend cap must be between $0 and $1,000,000.",
            &ErrorCode::InvalidAmount,
            Some("Use a positive dollar amount, or 0 for no cap."),
        );
        process::exit(1);
    }

    let cents = (amount * 100.0).round() as u64;

    match client.set_spend_cap(cents) {
        Ok(result) => {
            if cents == 0 {
                output::success("Spend cap removed (unlimited).", Some(result));
            } else {
                output::success(
                    &format!("Spend cap set to ${amount:.2}/month."),
                    Some(result),
                );
            }
        }
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    }
}

pub fn usage(period: &str) {
    super::require_auth();
    let client = super::init_client(None);

    let org = match client.get_org_me() {
        Ok(o) => o,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let limits = match client.get_billing_limits() {
        Ok(l) => l,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let breakdown = match client.get_org_cost_breakdown(period) {
        Ok(b) => b,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let plan = org.plan.as_deref();
    let spend_cap = org.spend_cap;
    let max_cap = limits.max_spend_cap_cents;

    // Every period-derived field reads from the period-scoped breakdown — the
    // authoritative spend for the requested `--period` — instead of the org's
    // always-current-month `current_period_spend_cents` / `spend_cap_exceeded`
    // columns. `total_cost_usd` is `org_usage_spend_cents(period) / 100`, so
    // rounding back recovers the exact period cents and keeps `% of cap`, the
    // progress bar, and `spend_cap_exceeded` consistent with the spend the
    // command actually displays (#1161).
    let period_spend_cents = (breakdown.total_cost_usd * 100.0).round() as u64;
    let exceeded = matches!(spend_cap, Some(cap) if cap > 0 && period_spend_cents >= cap);

    // Deployment restrictions reflect the API's current status, independently
    // of the historical/partial-period spend displayed above.
    let current_exceeded = org.spend_cap_exceeded.unwrap_or_else(|| {
        output::error(
            "Response missing 'spend_cap_exceeded' field.",
            &ErrorCode::ParseError,
            Some("This is a bug. Please report it."),
        );
        process::exit(1);
    });
    let cap_status = spend_cap_status(current_exceeded, org.spend_cap_policy.as_ref());

    let (plan_label, plan_price) = plan_display(plan);

    let data = serde_json::json!({
        "plan": canonical_plan(plan),
        "spend_cap_cents": spend_cap,
        "max_spend_cap_cents": max_cap,
        "period_spend_cents": period_spend_cents,
        "spend_cap_exceeded": exceeded,
        "spend_cap_policy": org.spend_cap_policy,
        "deploys_blocked": cap_status.deploys_blocked,
        "period": period,
        "total_cost_usd": breakdown.total_cost_usd,
        "included_cost_usd": breakdown.included_cost_usd,
        "apps": breakdown.apps,
        "lines": breakdown.lines,
    });

    if output::is_json_mode() {
        output::success("", Some(data));
        return;
    }

    match plan_price {
        Some(price) => eprintln!("  Plan: {plan_label} ({price})"),
        None => eprintln!("  Plan: {plan_label}"),
    }
    eprintln!(
        "  floo credits included: {:.2}/month",
        breakdown.included_cost_usd
    );
    eprintln!(
        "  Compute used: {} ({})",
        format_money(breakdown.total_cost_usd, MoneyFormat::Total),
        breakdown.period.label
    );

    match spend_cap {
        Some(cents) if cents > 0 => {
            let max_str = match max_cap {
                Some(m) => format!(
                    " (max {} for {})",
                    format_money(m as f64 / 100.0, MoneyFormat::Total),
                    plan_label
                ),
                None => String::new(),
            };
            eprintln!(
                "  Spend cap: {}/month{}",
                format_money(cents as f64 / 100.0, MoneyFormat::Total),
                max_str
            );

            let pct = ((period_spend_cents as f64 / cents as f64) * 100.0).min(100.0);
            let filled = (pct / 100.0 * 30.0).round() as usize;
            let empty = 30 - filled;
            eprintln!("  Usage: {:.0}% of cap", pct);
            eprintln!(
                "  {}{}  {:.0}%",
                "\u{2588}".repeat(filled),
                "\u{2591}".repeat(empty),
                pct
            );
        }
        _ => eprintln!("  Spend cap: none (unlimited)"),
    }

    if !breakdown.apps.is_empty() {
        eprintln!("  By app:");
        for app in &breakdown.apps {
            eprintln!(
                "    {:<30}  {}",
                app.name,
                format_money(app.total_cost_usd, MoneyFormat::Total)
            );
        }
    }

    if !breakdown.lines.is_empty() {
        eprintln!("  Organization cost lines:");
        render_cost_lines(&breakdown.lines);
    }

    if exceeded && period == "last_month" {
        output::warn(&format!(
            "Spend exceeded the cap in {}.",
            breakdown.period.label
        ));
    }
    if let Some(message) = cap_status.message {
        output::warn(message);
    }
}

/// Show the API's per-app costs without reconstructing quantities or prices.
pub fn cost_breakdown(app: Option<&str>, period: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let breakdown = match client.get_app_cost_breakdown(&app_id, period) {
        Ok(b) => b,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };
    if output::is_json_mode() {
        output::success("", Some(output::to_value(&breakdown)));
        return;
    }
    eprintln!("  App: {app_name} ({})", breakdown.period.label);
    eprintln!(
        "  Total cost: {}",
        format_money(breakdown.total_cost_usd, MoneyFormat::Total)
    );
    for service in &breakdown.services {
        render_resource_costs(
            &format!("Service: {}", service.name),
            service.total_usd,
            &service.costs,
            &service.lines,
        );
    }
    for resource in &breakdown.managed_resources {
        render_resource_costs(
            &format!(
                "Managed resource: {} ({})",
                resource.name,
                resource
                    .environment
                    .as_deref()
                    .unwrap_or("unknown environment")
            ),
            resource.total_usd,
            &resource.costs,
            &resource.lines,
        );
    }
    eprintln!(
        "  Unattributed cost: {}",
        format_money(breakdown.unattributed_cost_usd, MoneyFormat::Total)
    );
}

enum MoneyFormat {
    Total,
    Line,
}

fn render_resource_costs(
    label: &str,
    total: f64,
    costs: &BTreeMap<String, ResourceCost>,
    lines: &[CostLine],
) {
    eprintln!("  {label} — {}", format_money(total, MoneyFormat::Total));
    for (kind, cost) in costs {
        eprintln!(
            "    {kind}: {}",
            format_money(cost.cost_usd, MoneyFormat::Total)
        );
    }
    render_cost_lines(lines);
}

fn format_money(amount: f64, format: MoneyFormat) -> String {
    const MIN_LINE_AMOUNT: f64 = 0.0001;
    match format {
        MoneyFormat::Total => format!("${amount:.2}"),
        MoneyFormat::Line if amount > 0.0 && amount < MIN_LINE_AMOUNT => "<$0.0001".into(),
        MoneyFormat::Line => {
            let rounded = format!("{amount:.4}");
            format!("${}", rounded.trim_end_matches('0').trim_end_matches('.'))
        }
    }
}

fn render_cost_lines(lines: &[CostLine]) {
    for line in lines {
        let (rate, rate_unit) = match (line.display_rate, line.display_unit.as_deref()) {
            (Some(rate), Some(unit)) => (Some(rate), unit),
            _ => (line.rate, line.unit.as_str()),
        };
        let rate = rate.map_or_else(|| "unknown".into(), |r| format_money(r, MoneyFormat::Line));
        eprintln!(
            "    {} ({})",
            line.label.as_deref().unwrap_or("Cost line"),
            line.rate_key.as_deref().unwrap_or("unknown rate key")
        );
        eprintln!(
            "      Quantity: {} {} | Rate: {} / {} | Amount: {}",
            line.quantity,
            line.unit,
            rate,
            rate_unit,
            format_money(line.cost_usd, MoneyFormat::Line)
        );
        eprintln!(
            "      Rate card: {} | Effective from: {}",
            line.rate_card_version.as_deref().unwrap_or("unknown"),
            line.effective_from.as_deref().unwrap_or("unknown")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_plan, plan_display, spend_cap_status};
    use crate::api_types::SpendCapPolicy;
    use crate::output;

    #[test]
    fn cap_not_exceeded_has_no_message_or_block() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        let status = spend_cap_status(false, Some(&SpendCapPolicy::HardStop));
        assert_eq!(status.message, None);
        assert!(!status.deploys_blocked);
    }

    #[test]
    fn alerts_only_exceeded_blocks_nothing() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        let policy: SpendCapPolicy = serde_json::from_str(r#""alerts_only""#).unwrap();
        let status = spend_cap_status(true, Some(&policy));
        assert_eq!(status.message, Some("Spend cap exceeded. Budget alerts only: nothing is blocked. Raise the cap: floo billing spend-cap set <amount>"));
        assert!(!status.deploys_blocked);
    }

    #[test]
    fn freeze_new_spend_exceeded_pauses_new_deploys() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        let policy: SpendCapPolicy = serde_json::from_str(r#""freeze_new_spend""#).unwrap();
        let status = spend_cap_status(true, Some(&policy));
        assert_eq!(status.message, Some("Spend cap exceeded. New deploys are paused until you raise the cap or the next billing period begins. Raise the cap: floo billing spend-cap set <amount>"));
        assert!(status.deploys_blocked);
    }

    #[test]
    fn hard_stop_exceeded_blocks_deploys_and_scales_services_to_zero() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        let policy: SpendCapPolicy = serde_json::from_str(r#""hard_stop""#).unwrap();
        let status = spend_cap_status(true, Some(&policy));
        assert_eq!(status.message, Some("Spend cap exceeded. Deploys are blocked and running services are scaled to zero until you raise the cap or the next billing period begins. Raise the cap: floo billing spend-cap set <amount>"));
        assert!(status.deploys_blocked);
    }

    #[test]
    fn null_policy_exceeded_defaults_to_hard_stop() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        let policy: Option<SpendCapPolicy> = serde_json::from_str("null").unwrap();
        let status = spend_cap_status(true, policy.as_ref());
        assert_eq!(status.message, Some("Spend cap exceeded. Deploys are blocked and running services are scaled to zero until you raise the cap or the next billing period begins. Raise the cap: floo billing spend-cap set <amount>"));
        assert!(status.deploys_blocked);
    }

    #[test]
    fn unknown_policy_exceeded_uses_hard_stop_messaging() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        let policy: SpendCapPolicy = serde_json::from_str(r#""future_policy""#).unwrap();
        let status = spend_cap_status(true, Some(&policy));
        assert_eq!(status.message, Some("Spend cap exceeded. Deploys are blocked and running services are scaled to zero until you raise the cap or the next billing period begins. Raise the cap: floo billing spend-cap set <amount>"));
        assert!(status.deploys_blocked);
    }

    #[test]
    fn plan_display_projects_legacy_offers_into_the_canonical_catalog() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        for plan in ["hobby", "pro", "paygo"] {
            assert_eq!(canonical_plan(Some(plan)), Some("paygo"));
            assert_eq!(
                plan_display(Some(plan)),
                ("Pay as you go", Some("$0 commitment"))
            );
        }
        assert_eq!(plan_display(Some("team")), ("Team", Some("$250/mo")));
        assert_eq!(
            plan_display(Some("enterprise")),
            ("Enterprise", Some("Custom"))
        );
    }

    #[test]
    fn missing_plan_has_no_price() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        assert_eq!(canonical_plan(None), None);
        assert_eq!(plan_display(None), ("No plan", None));
    }

    #[test]
    fn legacy_free_is_a_missing_plan() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        assert_eq!(canonical_plan(Some("free")), None);
        assert_eq!(plan_display(Some("free")), ("No plan", None));
    }
}
