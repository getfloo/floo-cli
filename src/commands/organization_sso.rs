use std::process;

use crate::api_client::FlooClient;
use crate::api_types::{
    OrgResponse, OrganizationSsoBrowserHandoff, OrganizationSsoCredentialStatus,
    OrganizationSsoDoctorResponse, OrganizationSsoEnforcementAccess,
    OrganizationSsoEnforcementPolicy, OrganizationSsoLifecycle, OrganizationSsoNextAction,
    OrganizationSsoProviderReachability, OrganizationSsoProviderState, OrganizationSsoProviderType,
    OrganizationSsoStatusResponse,
};
use crate::config::{load_config, FlooConfig};
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;

struct OrganizationContext {
    client: FlooClient,
    org: OrgResponse,
    config: FlooConfig,
}

pub fn status(org_selector: Option<&str>) {
    let context = resolve_organization(org_selector);
    match context.client.get_organization_sso_status(&context.org.id) {
        Ok(status) => render_status(&context.org, &status),
        Err(error) => exit_api_error(error, Some(&context.org)),
    }
}

pub fn doctor(org_selector: Option<&str>) {
    let context = resolve_organization(org_selector);
    match context.client.get_organization_sso_doctor(&context.org.id) {
        Ok(doctor) => {
            let healthy = doctor.provider_reachability
                == OrganizationSsoProviderReachability::Reachable
                && matches!(
                    doctor.enforcement_access,
                    OrganizationSsoEnforcementAccess::NotRequired
                        | OrganizationSsoEnforcementAccess::Allowed
                )
                && doctor.next_action.is_none();
            render_doctor(&context.org, &doctor);
            if !healthy {
                process::exit(1);
            }
        }
        Err(error) => exit_api_error(error, Some(&context.org)),
    }
}

pub fn portal(org_selector: Option<&str>, no_browser: bool) {
    let context = resolve_organization(org_selector);
    match context
        .client
        .create_organization_sso_portal(&context.org.id)
    {
        Ok(portal) => {
            if output::is_json_mode() {
                output::success("", Some(output::to_value(&portal)));
                return;
            }
            output::success("Created a short-lived SSO setup session.", None);
            output::info(&format!("  {}", portal.url), None);
            if !no_browser {
                open_browser(&portal.url);
            }
        }
        Err(error) => exit_api_error(error, Some(&context.org)),
    }
}

pub fn enforce(org_selector: Option<&str>, no_browser: bool) {
    browser_handoff(org_selector, "enable", no_browser);
}

pub fn disable_enforcement(org_selector: Option<&str>, no_browser: bool) {
    browser_handoff(org_selector, "disable", no_browser);
}

fn browser_handoff(org_selector: Option<&str>, action: &str, no_browser: bool) {
    let context = resolve_organization(org_selector);
    if let Err(error) = context.client.get_organization_sso_doctor(&context.org.id) {
        exit_api_error(error, Some(&context.org));
    }
    let app_url = dashboard_url(&context.config.api_url).unwrap_or_else(|| {
        output::error(
            "The dashboard URL for this API environment is not configured.",
            &ErrorCode::Other("SSO_DASHBOARD_URL_UNAVAILABLE".to_string()),
            Some("Set FLOO_APP_URL to the matching floo dashboard origin and try again."),
        );
        process::exit(1);
    });
    let url = format!(
        "{app_url}/sso/manage?org={}&action={action}",
        context.org.id
    );
    let handoff = OrganizationSsoBrowserHandoff {
        organization_id: context.org.id.clone(),
        action: action.to_string(),
        url: url.clone(),
    };
    if output::is_json_mode() {
        output::success("", Some(output::to_value(&handoff)));
        return;
    }
    let verb = if action == "enable" {
        "enable SSO enforcement"
    } else {
        "disable SSO enforcement"
    };
    output::success(&format!("Created a browser handoff to {verb}."), None);
    output::info(&format!("  {url}"), None);
    if !no_browser {
        open_browser(&url);
    }
}

fn resolve_organization(selector: Option<&str>) -> OrganizationContext {
    super::require_auth();
    let mut config = load_config();
    let mut discovery_config = config.clone();
    if selector.is_some() {
        discovery_config.default_org = None;
    }
    let discovery_client = super::init_client(Some(discovery_config));
    let org = match selector {
        Some(value) => {
            let orgs = discovery_client.list_orgs().unwrap_or_else(|error| {
                exit_api_error(error, None);
            });
            orgs.orgs
                .into_iter()
                .find(|org| org.id == value || org.slug.as_deref() == Some(value))
                .unwrap_or_else(|| {
                    output::error(
                        &format!("Organization '{value}' not found."),
                        &ErrorCode::from_api("ORG_NOT_FOUND"),
                        Some("Run 'floo orgs switch <slug-or-id>' or choose an organization you belong to."),
                    );
                    process::exit(1);
                })
        }
        None => discovery_client.get_org_me().unwrap_or_else(|error| {
            exit_api_error(error, None);
        }),
    };
    config.default_org = Some(org.id.clone());
    let client = super::init_client(Some(config.clone()));
    OrganizationContext {
        client,
        org,
        config,
    }
}

fn dashboard_url(api_url: &str) -> Option<String> {
    if let Ok(value) = std::env::var("FLOO_APP_URL") {
        let value = value.trim_end_matches('/');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    match api_url.trim_end_matches('/') {
        "https://api.getfloo.com" => Some("https://app.getfloo.com".to_string()),
        "https://api.dev.getfloo.com" => Some("https://app.dev.getfloo.com".to_string()),
        "http://localhost:8000" | "http://127.0.0.1:8000" => {
            Some("http://localhost:5173".to_string())
        }
        _ => None,
    }
}

fn render_status(org: &OrgResponse, status: &OrganizationSsoStatusResponse) {
    if output::is_json_mode() {
        output::success("", Some(output::to_value(status)));
        return;
    }
    output::info(
        &format!(
            "Organization: {}",
            org.display_name().unwrap_or(org.id.as_str())
        ),
        None,
    );
    output::info(
        &format!("Connection:   {}", provider_state_label(status.status)),
        None,
    );
    output::info(
        &format!("Lifecycle:    {}", lifecycle_label(status.lifecycle)),
        None,
    );
    output::info(
        &format!(
            "Provider:     {}",
            status
                .provider_type
                .map(provider_type_label)
                .unwrap_or("Not configured")
        ),
        None,
    );
    output::info(
        &format!("Policy:       {}", policy_label(status.enforcement_policy)),
        None,
    );
    output::info(
        &format!(
            "Ready:        {}",
            if status.enforcement_ready {
                "Yes"
            } else {
                "No"
            }
        ),
        None,
    );
    output::info(
        &format!(
            "Recovery:     {}",
            if status.recovery_ready {
                "Ready"
            } else {
                "Not ready"
            }
        ),
        None,
    );
    output::info(
        &format!(
            "Observed:     {}",
            status.observed_at.as_deref().unwrap_or("Never")
        ),
        None,
    );
    if status.domains.is_empty() {
        output::info("Domains:      None", None);
    } else {
        let domains = status
            .domains
            .iter()
            .map(|domain| format!("{} ({})", domain.domain, string_label(&domain.state)))
            .collect::<Vec<_>>()
            .join(", ");
        output::info(&format!("Domains:      {domains}"), None);
    }
    let selector = org.display_name().unwrap_or(org.id.as_str());
    if status.lifecycle != OrganizationSsoLifecycle::Enabled
        || status.status != OrganizationSsoProviderState::Active
    {
        output::info(
            &format!("Next action:  run 'floo orgs sso portal --org {selector}'"),
            None,
        );
    } else if status.enforcement_policy == OrganizationSsoEnforcementPolicy::Optional {
        output::info(
            &format!("Next action:  run 'floo orgs sso enforce --org {selector}'"),
            None,
        );
    }
}

fn render_doctor(org: &OrgResponse, doctor: &OrganizationSsoDoctorResponse) {
    if output::is_json_mode() {
        output::success("", Some(output::to_value(doctor)));
        return;
    }
    output::info(
        &format!(
            "Organization: {}",
            org.display_name().unwrap_or(org.id.as_str())
        ),
        None,
    );
    output::info(
        &format!(
            "Provider:     {}",
            reachability_label(doctor.provider_reachability)
        ),
        None,
    );
    output::info(
        &format!(
            "Credential:   {}",
            credential_label(doctor.credential_status)
        ),
        None,
    );
    output::info(
        &format!("Access:       {}", access_label(doctor.enforcement_access)),
        None,
    );
    if let Some(action) = doctor.next_action {
        output::info(&format!("Next action:  {}", next_action(action, org)), None);
    }
}

fn next_action(action: OrganizationSsoNextAction, org: &OrgResponse) -> String {
    let selector = org.display_name().unwrap_or(org.id.as_str());
    match action {
        OrganizationSsoNextAction::OpenSsoSetup => {
            format!("run 'floo orgs sso portal --org {selector}'")
        }
        OrganizationSsoNextAction::UseOrganizationSsoRecovery => {
            "open the dashboard recovery flow with your stored recovery token".to_string()
        }
        OrganizationSsoNextAction::CliReauthenticateWithOrgSso => format!(
            "run 'floo orgs switch {selector}', then 'floo auth login --force' through organization SSO"
        ),
    }
}

fn exit_api_error(error: FlooApiError, org: Option<&OrgResponse>) -> ! {
    let suggestion = match error.code.as_str() {
        "SSO_BROWSER_STEP_UP_REQUIRED" | "SSO_STEP_UP_REQUIRED" => org.map(|org| {
            format!(
                "Run 'floo orgs switch {}', then 'floo auth login --force' through organization SSO.",
                org.display_name().unwrap_or(org.id.as_str())
            )
        }),
        "SSO_UNAVAILABLE" => Some(
            "Run 'floo orgs sso doctor' and use the dashboard recovery flow if directed."
                .to_string(),
        ),
        "ENTERPRISE_SSO_REQUIRED" => {
            Some("Enterprise SSO setup requires an Enterprise organization plan.".to_string())
        }
        _ => None,
    };
    output::error_with_details(
        &error.message,
        &ErrorCode::from_api(&error.code),
        suggestion.as_deref(),
        error.extra.as_ref(),
    );
    process::exit(1);
}

fn open_browser(url: &str) {
    if let Err(error) = open::that(url) {
        output::warn(&format!("Could not open a browser: {error}"));
    }
}

fn string_label(value: &str) -> &str {
    match value {
        "pending" => "Pending",
        "verified" => "Verified",
        other => other,
    }
}

fn provider_state_label(value: OrganizationSsoProviderState) -> &'static str {
    match value {
        OrganizationSsoProviderState::Pending => "Pending",
        OrganizationSsoProviderState::Active => "Active",
        OrganizationSsoProviderState::Inactive => "Inactive",
        OrganizationSsoProviderState::Deleted => "Deleted",
        OrganizationSsoProviderState::Invalid => "Invalid",
        OrganizationSsoProviderState::ProviderUnavailable => "Unavailable",
    }
}

fn lifecycle_label(value: OrganizationSsoLifecycle) -> &'static str {
    match value {
        OrganizationSsoLifecycle::Enabled => "Enabled",
        OrganizationSsoLifecycle::Disabled => "Disabled",
        OrganizationSsoLifecycle::Removed => "Removed",
    }
}

fn provider_type_label(value: OrganizationSsoProviderType) -> &'static str {
    match value {
        OrganizationSsoProviderType::Saml => "SAML",
        OrganizationSsoProviderType::Oidc => "OIDC",
    }
}

fn policy_label(value: OrganizationSsoEnforcementPolicy) -> &'static str {
    match value {
        OrganizationSsoEnforcementPolicy::Optional => "Optional",
        OrganizationSsoEnforcementPolicy::Enforced => "Required",
    }
}

fn reachability_label(value: OrganizationSsoProviderReachability) -> &'static str {
    match value {
        OrganizationSsoProviderReachability::Reachable => "Reachable",
        OrganizationSsoProviderReachability::Unavailable => "Unavailable",
        OrganizationSsoProviderReachability::NotConfigured => "Not configured",
    }
}

fn credential_label(value: OrganizationSsoCredentialStatus) -> &'static str {
    match value {
        OrganizationSsoCredentialStatus::ExactCurrentSso => "Exact current SSO",
        OrganizationSsoCredentialStatus::NotExactCurrentSso => "Not exact current SSO",
        OrganizationSsoCredentialStatus::NotApplicable => "Not applicable",
    }
}

fn access_label(value: OrganizationSsoEnforcementAccess) -> &'static str {
    match value {
        OrganizationSsoEnforcementAccess::NotRequired => "Not required",
        OrganizationSsoEnforcementAccess::Allowed => "Allowed",
        OrganizationSsoEnforcementAccess::StepUpRequired => "SSO sign-in required",
        OrganizationSsoEnforcementAccess::BindingUnavailable => "Binding unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_url_tracks_known_api_environments() {
        assert_eq!(
            dashboard_url("https://api.getfloo.com"),
            Some("https://app.getfloo.com".to_string())
        );
        assert_eq!(
            dashboard_url("https://api.dev.getfloo.com"),
            Some("https://app.dev.getfloo.com".to_string())
        );
        assert_eq!(
            dashboard_url("http://localhost:8000"),
            Some("http://localhost:5173".to_string())
        );
        assert_eq!(dashboard_url("https://api.custom.example"), None);
    }

    #[test]
    fn stable_next_actions_render_exact_commands() {
        let org = OrgResponse {
            id: "org-id".to_string(),
            name: Some("Example".to_string()),
            slug: Some("example".to_string()),
            plan: None,
            spend_cap: None,
            current_period_spend_cents: None,
            spend_cap_exceeded: None,
        };
        assert_eq!(
            next_action(OrganizationSsoNextAction::OpenSsoSetup, &org),
            "run 'floo orgs sso portal --org example'"
        );
        assert!(
            next_action(OrganizationSsoNextAction::CliReauthenticateWithOrgSso, &org)
                .contains("floo auth login --force")
        );
    }
}
