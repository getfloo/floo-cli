use std::process;

use crate::errors::ErrorCode;
use crate::output;

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

    // Cap is cents-denominated; emit it as `spend_cap_cents` to match
    // `billing usage` and every other *_cents field. One key per concept so
    // agents never special-case `spend_cap` vs `spend_cap_cents` (#1161).
    let data = serde_json::json!({
        "spend_cap_cents": spend_cap,
        "current_period_spend_cents": current_spend,
        "spend_cap_exceeded": exceeded,
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
    if exceeded {
        output::warn("Spend cap exceeded \u{2014} deploys are blocked.");
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

    let (plan_label, plan_price) = plan_display(plan);

    let data = serde_json::json!({
        "plan": canonical_plan(plan),
        "spend_cap_cents": spend_cap,
        "max_spend_cap_cents": max_cap,
        "period_spend_cents": period_spend_cents,
        "spend_cap_exceeded": exceeded,
        "period": period,
        "total_cost_usd": breakdown.total_cost_usd,
        "included_cost_usd": breakdown.included_cost_usd,
        "apps": breakdown.apps.iter().map(|a| serde_json::json!({
            "app_id": a.app_id,
            "name": a.name,
            "total_cost_usd": a.total_cost_usd,
        })).collect::<Vec<_>>(),
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
        "  Compute used: ${:.2} ({})",
        breakdown.total_cost_usd, breakdown.period.label
    );

    match spend_cap {
        Some(cents) if cents > 0 => {
            let max_str = match max_cap {
                Some(m) => format!(" (max ${} for {})", m / 100, plan_label),
                None => String::new(),
            };
            eprintln!("  Spend cap: ${:.2}/month{}", cents as f64 / 100.0, max_str);

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
        eprintln!();
        eprintln!("  By app:");
        for app in &breakdown.apps {
            eprintln!("    {:<30}  ${:.2}", app.name, app.total_cost_usd);
        }
    }

    if exceeded {
        eprintln!();
        if period == "last_month" {
            output::warn(&format!(
                "Spend exceeded the cap in {}.",
                breakdown.period.label
            ));
        } else {
            output::warn("Spend cap exceeded \u{2014} deploys are blocked.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_plan, plan_display};
    use crate::output;

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
