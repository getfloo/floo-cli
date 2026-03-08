use std::process;

use crate::errors::ErrorCode;
use crate::output;

pub fn upgrade(plan: Option<String>) {
    super::require_auth();
    let client = super::init_client(None);

    match client.create_billing_checkout(plan.as_deref()) {
        Ok(result) => {
            if output::is_json_mode() {
                output::success("", Some(serde_json::json!({"url": result.url})));
            } else {
                output::info("Opening billing page in browser...", None);
                if open::that(&result.url).is_err() {
                    output::warn(&format!("Open this URL manually: {}", result.url));
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
                "email": "solutions@getfloo.com",
                "subject": "Enterprise inquiry",
            })),
        );
    } else {
        eprintln!("  Enterprise & custom plans: solutions@getfloo.com");
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

    let data = serde_json::json!({
        "spend_cap": spend_cap,
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
    if org.plan.as_deref() == Some("free") {
        eprintln!("  Upgrade: floo billing upgrade --plan growth");
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

pub fn usage() {
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

    let plan = org.plan.as_deref().unwrap_or("free");
    let current_spend = org.current_period_spend_cents.unwrap_or(0);
    let spend_cap = org.spend_cap;
    let exceeded = org.spend_cap_exceeded.unwrap_or(false);
    let max_cap = limits.max_spend_cap_cents;

    let plan_price = match plan {
        "hobby" => "$5/mo",
        "pro" => "$20/mo",
        "team" => "$200/mo",
        "enterprise" => "Custom",
        _ => "$0",
    };

    let included_cents: u64 = match plan {
        "free" => 500,
        "hobby" => 500,
        "pro" => 2000,
        "team" => 20000,
        _ => 0,
    };

    let data = serde_json::json!({
        "plan": plan,
        "spend_cap_cents": spend_cap,
        "max_spend_cap_cents": max_cap,
        "current_period_spend_cents": current_spend,
        "spend_cap_exceeded": exceeded,
    });

    if output::is_json_mode() {
        output::success("", Some(data));
        return;
    }

    let plan_label = plan[..1].to_uppercase() + &plan[1..];
    eprintln!("  Plan: {} ({})", plan_label, plan_price);
    eprintln!(
        "  Included compute: ${:.2}/month",
        included_cents as f64 / 100.0
    );
    eprintln!("  Current spend: ${:.2}", current_spend as f64 / 100.0);

    match spend_cap {
        Some(cents) if cents > 0 => {
            let max_str = match max_cap {
                Some(m) => format!(" (max ${} for {})", m / 100, plan_label),
                None => String::new(),
            };
            eprintln!("  Spend cap: ${:.2}/month{}", cents as f64 / 100.0, max_str);

            let pct = ((current_spend as f64 / cents as f64) * 100.0).min(100.0);
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

    if exceeded {
        output::warn("Spend cap exceeded \u{2014} deploys are blocked.");
    }
}
