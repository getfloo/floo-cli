use std::net::IpAddr;
use std::process;

use ipnet::{IpNet, Ipv4Net, Ipv6Net};

use crate::api_types::{EdgePolicyRule, EdgeRoute};
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

/// Mirror of the gateway's `CompiledEdgePolicy.admits` (gateway/app/edge_policy.py):
/// ordered first-match CIDR containment, version-matched, else the default action.
/// Returns (admitted, matched_rule_index). The gateway is the authoritative
/// enforcer; this is a read-only preview, so a rule whose CIDR fails to parse
/// (server-validated, so vanishingly rare) is skipped rather than erroring.
fn admits(ip: IpAddr, rules: &[EdgePolicyRule], default_allow: bool) -> (bool, Option<usize>) {
    let ip = normalize_ip(ip);
    for (i, rule) in rules.iter().enumerate() {
        if let Some(net) = parse_rule_cidr(&rule.cidr) {
            // IpNet::contains returns false on version mismatch, matching the
            // gateway's explicit `address.version == network.version` guard.
            if net.contains(&ip) {
                return (rule.action == "allow", Some(i));
            }
        }
    }
    (default_allow, None)
}

/// Parse a stored rule CIDR, normalizing a bare IP to a host network (/32 or
/// /128) — the gateway's `ipaddress.ip_network(strict=False)` accepts bare
/// IPs, so `check` must too or it would disagree with enforcement on a rule
/// that reached the DB un-normalized. Genuinely unparseable input is skipped.
fn parse_rule_cidr(cidr: &str) -> Option<IpNet> {
    if let Ok(net) = cidr.parse::<IpNet>() {
        return Some(net);
    }
    match cidr.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => Ipv4Net::new(v4, 32).ok().map(IpNet::V4),
        Ok(IpAddr::V6(v6)) => Ipv6Net::new(v6, 128).ok().map(IpNet::V6),
        Err(_) => None,
    }
}

/// An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) evaluates as its IPv4 form,
/// matching the gateway's `_parse_client_ip`.
fn normalize_ip(ip: IpAddr) -> IpAddr {
    if let IpAddr::V6(v6) = ip {
        if let Some(v4) = v6.to_ipv4_mapped() {
            return IpAddr::V4(v4);
        }
    }
    ip
}

pub fn policy_check(ip_str: &str, app: Option<&str>, env: &str) {
    let ip: IpAddr = match ip_str.parse() {
        Ok(ip) => ip,
        Err(_) => {
            output::error(
                &format!("'{ip_str}' is not a valid IP address."),
                &ErrorCode::InvalidFormat,
                Some("Pass an IPv4 or IPv6 address, e.g. 203.0.113.7 or 2001:db8::1."),
            );
            process::exit(1);
        }
    };

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let policy = match client.get_edge_policy(&app_id, env) {
        Ok(p) => Some(p),
        Err(e) if e.code == "EDGE_POLICY_NOT_FOUND" => None,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    // No policy, or a stored-but-disabled policy, admits every IP (the gateway
    // only enforces an enabled policy).
    let (admitted, matched_idx, enforced) = match &policy {
        None => (true, None, false),
        Some(p) if !p.enabled => (true, None, false),
        Some(p) => {
            let (a, m) = admits(ip, &p.rules, p.default_action == "allow");
            (a, m, true)
        }
    };
    let matched_rule = matched_idx.and_then(|i| policy.as_ref().map(|p| &p.rules[i]));

    if output::is_json_mode() {
        let reason = if !enforced {
            if policy.is_none() {
                "no_policy"
            } else {
                "policy_disabled"
            }
        } else if matched_rule.is_some() {
            "rule_match"
        } else {
            "default_action"
        };
        output::success(
            &format!("Edge policy check for {app_name} in {env}."),
            Some(serde_json::json!({
                "ip": ip_str,
                "environment": env,
                "admitted": admitted,
                "enforced": enforced,
                "matched_rule": matched_rule,
                "reason": reason,
            })),
        );
        return;
    }

    let verdict = if admitted { "ADMITTED" } else { "DENIED" };
    let detail = match (&policy, matched_rule) {
        (None, _) => "no edge policy, all IPs admitted".to_string(),
        (Some(p), _) if !p.enabled => {
            "policy is disabled (not enforced), all IPs admitted".to_string()
        }
        (Some(_), Some(rule)) => format!("matched rule {} {}", rule.action, rule.cidr),
        (Some(p), None) => format!("no rule matched, default {}", p.default_action),
    };
    output::info(
        &format!("{ip_str} in {app_name}/{env}: {verdict} ({detail})."),
        None,
    );
}
