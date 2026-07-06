use std::process;

use crate::api_types::EdgeRoute;
use crate::errors::ErrorCode;
use crate::output;

pub fn list_routes(app: Option<&str>, env: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let response = match client.list_edge_routes(&app_id, env) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            &format!("Edge routes for {app_name}."),
            Some(output::to_value(&response)),
        );
        return;
    }

    if response.routes.is_empty() {
        let scope = env.map(|value| format!(" in {value}")).unwrap_or_default();
        output::info(&format!("No edge routes for {app_name}{scope}."), None);
        return;
    }

    let rows: Vec<Vec<String>> = response.routes.iter().map(route_row).collect();
    output::table(
        &[
            "Host", "Path", "Env", "Service", "Policy", "Source", "Updated",
        ],
        &rows,
        Some(output::to_value(&response)),
    );
}

fn route_row(route: &EdgeRoute) -> Vec<String> {
    vec![
        route.host.clone(),
        route.path_prefix.clone(),
        route
            .environment_slug
            .as_deref()
            .or(route.environment_name.as_deref())
            .unwrap_or("unscoped")
            .to_string(),
        service_label(route),
        policy_label(route),
        route.source.clone(),
        route.updated_at.clone(),
    ]
}

fn service_label(route: &EdgeRoute) -> String {
    match (route.service_name.as_deref(), route.service_type.as_deref()) {
        (Some(name), Some(service_type)) => format!("{name} ({service_type})"),
        (Some(name), None) => name.to_string(),
        _ => "app".to_string(),
    }
}

fn policy_label(route: &EdgeRoute) -> String {
    match (route.api_key_enabled, route.required_scope.as_deref()) {
        (true, Some(scope)) => format!("{} + key:{scope}", route.access_mode),
        (true, None) => format!("{} + key", route.access_mode),
        (false, Some(scope)) => format!("{}:{scope}", route.access_mode),
        (false, None) => route.access_mode.clone(),
    }
}

// --- Edge policy (#1358): the app+environment IP/CIDR firewall ---

pub fn policy_get(app: Option<&str>, env: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let policy = match client.get_edge_policy(&app_id, env) {
        Ok(p) => p,
        Err(e) if e.code == "EDGE_POLICY_NOT_FOUND" => {
            // No policy is a valid state, not an error: all IPs admitted.
            output::info(
                &format!("No edge policy for {app_name} in {env} — all IPs admitted."),
                Some(serde_json::json!({ "policy": null, "environment": env })),
            );
            return;
        }
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            &format!("Edge policy for {app_name} in {env}."),
            Some(output::to_value(&policy)),
        );
        return;
    }

    let status = if policy.enabled {
        "enabled"
    } else {
        "DISABLED (stored but not enforced)"
    };
    output::info(
        &format!(
            "Edge policy for {app_name} in {env}: {status}, default {}.",
            policy.default_action
        ),
        None,
    );
    if policy.rules.is_empty() {
        output::info(
            &format!(
                "No rules — every request gets the default ({}).",
                policy.default_action
            ),
            None,
        );
        return;
    }
    let rows: Vec<Vec<String>> = policy
        .rules
        .iter()
        .enumerate()
        .map(|(i, rule)| vec![(i + 1).to_string(), rule.action.clone(), rule.cidr.clone()])
        .collect();
    output::table(
        &["#", "Action", "CIDR"],
        &rows,
        Some(output::to_value(&policy)),
    );
}

/// Parse `--rule allow:203.0.113.0/24` into an API rule. CIDR syntax is
/// validated server-side (one validation authority); the CLI only splits
/// the action prefix so typos fail fast with a local message.
fn parse_rule_arg(raw: &str) -> Result<crate::api_types::EdgePolicyRule, String> {
    let Some((action, cidr)) = raw.split_once(':') else {
        return Err(format!(
            "rule '{raw}' must be <allow|deny>:<CIDR>, e.g. allow:203.0.113.0/24"
        ));
    };
    if action != "allow" && action != "deny" {
        return Err(format!(
            "rule '{raw}' has action '{action}' — must be 'allow' or 'deny'"
        ));
    }
    if cidr.is_empty() {
        return Err(format!("rule '{raw}' is missing a CIDR after the ':'"));
    }
    Ok(crate::api_types::EdgePolicyRule {
        action: action.to_string(),
        cidr: cidr.to_string(),
    })
}

pub fn policy_set(
    app: Option<&str>,
    env: &str,
    rules: &[String],
    default_action: &str,
    disabled: bool,
) {
    let mut parsed = Vec::with_capacity(rules.len());
    for raw in rules {
        match parse_rule_arg(raw) {
            Ok(rule) => parsed.push(rule),
            Err(msg) => {
                output::error(
                    &msg,
                    &ErrorCode::InvalidFormat,
                    Some("Rules are <allow|deny>:<CIDR>, evaluated in order; first match wins."),
                );
                process::exit(1);
            }
        }
    }
    if parsed.is_empty() && default_action == "allow" {
        output::error(
            "This policy has no rules and default-action allow — it admits everything, same as no policy.",
            &ErrorCode::InvalidFormat,
            Some("Add at least one --rule, use --default-action deny, or run 'floo edge policy clear'."),
        );
        process::exit(1);
    }

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let policy = match client.set_edge_policy(&app_id, env, &parsed, default_action, !disabled) {
        Ok(p) => p,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let note = if disabled {
        " (stored DISABLED — not enforced)"
    } else {
        ""
    };
    output::success(
        &format!(
            "Edge policy set for {app_name} in {env}: {} rule(s), default {}{note}.",
            policy.rules.len(),
            policy.default_action
        ),
        Some(output::to_value(&policy)),
    );
}

pub fn policy_clear(app: Option<&str>, env: &str, yes: bool) {
    use crate::confirm::{confirm_tier2, ConfirmOutcome, RiskMetadata, Tier};

    if output::is_dry_run_mode() {
        let risk: RiskMetadata = Tier::Two.into();
        let target = app.unwrap_or("(reads from config)");
        output::dry_run_preview(
            &format!("Would remove the {env} edge policy from {target} — all IPs admitted again."),
            serde_json::json!({
                "action": "edge_policy_clear",
                "app": app,
                "environment": env,
                "destructive": risk.destructive,
                "data_loss": risk.data_loss,
                "tier": risk.tier,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    match confirm_tier2(
        "Remove edge policy",
        &format!("the {env} IP firewall for {app_name} (all IPs admitted again)"),
        yes,
    ) {
        ConfirmOutcome::Proceed => {}
        ConfirmOutcome::Aborted => {
            if !output::is_json_mode() {
                output::info("Cancelled — edge policy unchanged.", None);
            }
            process::exit(0);
        }
        ConfirmOutcome::Refused { suggestion } => {
            crate::confirm::exit_refused(
                &format!(
                    "Refusing to remove the {env} edge policy for {app_name} without explicit confirmation."
                ),
                &suggestion,
            );
        }
    }

    if let Err(e) = client.delete_edge_policy(&app_id, env) {
        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
        process::exit(1);
    }

    let risk: RiskMetadata = Tier::Two.into();
    output::success(
        &format!("Edge policy removed for {app_name} in {env} — all IPs admitted again."),
        Some(serde_json::json!({
            "environment": env,
            "destructive": risk.destructive,
            "data_loss": risk.data_loss,
            "tier": risk.tier,
        })),
    );
}
