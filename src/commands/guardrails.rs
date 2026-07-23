use std::process;

use crate::api_client::FlooClient;
use crate::api_types::GuardrailPolicyResponse;
use crate::errors::ErrorCode;
use crate::output;

enum Target {
    Org { id: String, label: String },
    App { id: String, label: String },
}

fn resolve_target(client: &FlooClient, app: Option<&str>) -> Target {
    if let Some(app_name) = app {
        let resolved = super::resolve_app_or_exit(client, app_name);
        return Target::App {
            id: resolved.id,
            label: resolved.name,
        };
    }

    match client.get_org_me() {
        Ok(org) => Target::Org {
            label: org.display_name().unwrap_or(&org.id).to_string(),
            id: org.id,
        },
        Err(error) => {
            output::error(&error.message, &ErrorCode::from_api(&error.code), None);
            process::exit(1);
        }
    }
}

fn fetch(client: &FlooClient, target: &Target) -> GuardrailPolicyResponse {
    let result = match target {
        Target::Org { id, .. } => client.get_org_guardrails(id),
        Target::App { id, .. } => client.get_app_guardrails(id),
    };
    result.unwrap_or_else(|error| {
        output::error(&error.message, &ErrorCode::from_api(&error.code), None);
        process::exit(1);
    })
}

fn posture(gated: bool) -> &'static str {
    if gated {
        "approval required"
    } else {
        "automatic"
    }
}

fn render(policy: &GuardrailPolicyResponse, success_message: Option<&str>) {
    let value = output::to_value(policy);
    if output::is_json_mode() {
        output::success(success_message.unwrap_or("Guardrail policy"), Some(value));
        return;
    }
    if let Some(message) = success_message {
        output::success(message, None);
    }
    output::table(
        &["OPERATION", "DEV", "PROD / PREVIEW"],
        &[
            vec![
                "Recoverable (tier 2)".to_string(),
                posture(policy.gate_recoverable_dev).to_string(),
                posture(policy.gate_recoverable_prod).to_string(),
            ],
            vec![
                "Irreversible (tier 3)".to_string(),
                "approval required (immutable)".to_string(),
                "approval required (immutable)".to_string(),
            ],
            vec![
                "Additive (tier 1)".to_string(),
                "automatic (immutable)".to_string(),
                "automatic (immutable)".to_string(),
            ],
        ],
        None,
    );
    output::info(
        &format!("Effective source: {}", policy.source.replace('_', " ")),
        None,
    );
}

pub fn show(app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let target = resolve_target(&client, app);
    render(&fetch(&client, &target), None);
}

pub fn set(app: Option<&str>, dev: Option<bool>, prod: Option<bool>, inherit: bool) {
    super::require_auth();
    let desired = if inherit {
        None
    } else {
        Some(match (dev, prod) {
            (Some(dev), Some(prod)) => (dev, prod),
            _ => {
                output::error(
                    "Both --dev and --prod are required unless --inherit is used.",
                    &ErrorCode::InvalidFormat,
                    Some("Pass --dev <automatic|approval> --prod <automatic|approval>."),
                );
                process::exit(1);
            }
        })
    };
    let client = super::init_client(None);
    let target = resolve_target(&client, app);
    let result = if let Some((dev, prod)) = desired {
        match &target {
            Target::Org { id, .. } => client.set_org_guardrails(id, dev, prod),
            Target::App { id, .. } => client.set_app_guardrails(id, dev, prod),
        }
    } else {
        match &target {
            Target::Org { id, .. } => client.reset_org_guardrails(id),
            Target::App { id, .. } => client.reset_app_guardrails(id),
        }
    };
    let policy = result.unwrap_or_else(|error| {
        output::error(&error.message, &ErrorCode::from_api(&error.code), None);
        process::exit(1);
    });
    let target_label = match target {
        Target::Org { label, .. } => format!("org {label}"),
        Target::App { label, .. } => format!("app {label}"),
    };
    let action = if inherit { "Reset" } else { "Updated" };
    render(
        &policy,
        Some(&format!("{action} guardrail policy for {target_label}.")),
    );
}
