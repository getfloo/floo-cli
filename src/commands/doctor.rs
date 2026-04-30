//! `floo doctor accounts` — accounts-mode introspection (feedback 64268e05).
//!
//! Thin wrapper over `GET /v1/apps/{app_id}/doctor/accounts`. The endpoint
//! does the joining and drift computation; the CLI's job is rendering and
//! exit-code semantics so an agent can act on the response without parsing
//! prose.

use std::process;

use crate::api_types::{AccountsDoctorDrift, AccountsDoctorResponse};
use crate::errors::ErrorCode;
use crate::output;

pub fn accounts(app_flag: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _app_name) = super::resolve_app_from_config(&client, app_flag);

    let result = match client.diagnose_accounts(&app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        // Agents read the structured response directly; rendering decisions
        // live on the human path only.
        output::success(
            "Accounts doctor diagnosis.",
            Some(output::to_value(&result)),
        );
        // Exit non-zero when drift was found so scripted agents can branch
        // on `floo doctor accounts --json` exit code without parsing the
        // body. Same convention as `preflight` and `db migrate --dry-run`.
        if !result.drift.is_empty() {
            process::exit(1);
        }
        return;
    }

    render_human(&result);

    if !result.drift.is_empty() {
        process::exit(1);
    }
}

fn render_human(result: &AccountsDoctorResponse) {
    output::info(
        &format!("App: {} ({})", result.app_name, result.app_id),
        None,
    );
    eprintln!();

    output::info("Requested config:", None);
    eprintln!("  access_mode:    {}", result.requested.access_mode);
    eprintln!("  access_policy:  {}", result.requested.access_policy);
    eprintln!(
        "  allowed_domains: {}",
        if result.requested.allowed_domains.is_empty() {
            "(none)".to_string()
        } else {
            result.requested.allowed_domains.join(", ")
        }
    );
    eprintln!();

    output::info(
        &format!("Gateway routes ({} total):", result.serving.len()),
        None,
    );
    if result.serving.is_empty() {
        eprintln!("  (none — no live deploy has bound a host yet)");
    } else {
        let rows: Vec<Vec<String>> = result
            .serving
            .iter()
            .map(|r| {
                let mode_cell = if r.serving_access_mode == r.expected_access_mode {
                    r.serving_access_mode.clone()
                } else {
                    format!(
                        "{} (expected {})",
                        r.serving_access_mode, r.expected_access_mode
                    )
                };
                vec![
                    format!("{}{}", r.host, r.path_prefix),
                    mode_cell,
                    bool_yn(r.floo_endpoints_wired),
                    bool_yn(r.identity_headers_injected),
                ]
            })
            .collect();
        output::table(
            &["Route", "Mode", "/__floo wired", "Identity headers"],
            &rows,
            None,
        );
    }
    eprintln!();

    if let Some(deploy) = &result.latest_deploy {
        output::info("Latest deploy:", None);
        eprintln!("  id:                          {}", deploy.id);
        eprintln!("  status:                      {}", deploy.status);
        eprintln!(
            "  requested_app_access_mode:   {}",
            deploy
                .requested_app_access_mode
                .as_deref()
                .unwrap_or("(none)")
        );
        eprintln!(
            "  propagated:                  {}",
            bool_yn(deploy.propagated)
        );
        eprintln!("  created_at:                  {}", deploy.created_at);
        eprintln!();
    }

    if result.drift.is_empty() {
        output::info("No drift detected.", None);
    } else {
        output::info(
            &format!(
                "Drift detected ({} item{}):",
                result.drift.len(),
                if result.drift.len() == 1 { "" } else { "s" }
            ),
            None,
        );
        for d in &result.drift {
            render_drift(d);
        }
    }
}

fn render_drift(d: &AccountsDoctorDrift) {
    eprintln!();
    eprintln!("  • [{}] {}", d.kind, d.summary);
    if let Some(fix) = &d.likely_fix {
        eprintln!("    Suggested fix: {fix}");
    }
}

fn bool_yn(b: bool) -> String {
    if b {
        "yes".to_string()
    } else {
        "no".to_string()
    }
}
