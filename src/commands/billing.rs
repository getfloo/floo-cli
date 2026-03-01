use std::process;

use crate::output;

pub fn spend_cap_get() {
    super::require_auth();
    let client = super::init_client(None);

    let org = match client.get_org_me() {
        Ok(o) => o,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let spend_cap_value = org.get("spend_cap").unwrap_or_else(|| {
        output::error(
            "Response missing 'spend_cap' field.",
            "PARSE_ERROR",
            Some("This is a bug. Please report it."),
        );
        process::exit(1);
    });
    let spend_cap = if spend_cap_value.is_null() {
        None
    } else {
        Some(spend_cap_value.as_u64().unwrap_or_else(|| {
            output::error(
                &format!("Unexpected spend_cap value: {spend_cap_value}"),
                "PARSE_ERROR",
                Some("This is a bug. Please report it."),
            );
            process::exit(1);
        }))
    };
    let current_spend = org
        .get("current_period_spend_cents")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| {
            output::error(
                "Response missing 'current_period_spend_cents' field.",
                "PARSE_ERROR",
                Some("This is a bug. Please report it."),
            );
            process::exit(1);
        });
    let exceeded = org
        .get("spend_cap_exceeded")
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| {
            output::error(
                "Response missing 'spend_cap_exceeded' field.",
                "PARSE_ERROR",
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
        Some(0) => eprintln!("  Spend cap: none (unlimited)"),
        Some(cents) => eprintln!("  Spend cap: ${:.2}/month", cents as f64 / 100.0),
        None => eprintln!("  Spend cap: none (unlimited)"),
    }
    eprintln!("  Current spend: ${:.2}", current_spend as f64 / 100.0);
    if exceeded {
        output::warn("Spend cap exceeded — deploys are blocked.");
    }
}

pub fn spend_cap_set(amount: f64) {
    super::require_auth();
    let client = super::init_client(None);

    if !amount.is_finite() || amount < 0.0 {
        output::error(
            "Spend cap amount must be a finite, non-negative number.",
            "INVALID_AMOUNT",
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
                    &format!("Spend cap set to ${:.2}/month.", cents as f64 / 100.0),
                    Some(result),
                );
            }
        }
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    }
}
