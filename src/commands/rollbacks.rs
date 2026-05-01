use std::process;

use crate::confirm::{confirm_tier2, ConfirmOutcome, RiskMetadata, Tier};
use crate::errors::ErrorCode;
use crate::output;

pub fn rollback(app_name: &str, deploy_id: &str, yes: bool) {
    if output::is_dry_run_mode() {
        let risk: RiskMetadata = Tier::Two.into();
        let preview = format!("Would roll back app '{app_name}' to deploy {deploy_id}.");
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "rollback",
                "app": app_name,
                "to_deploy": deploy_id,
                "destructive": risk.destructive,
                "data_loss": risk.data_loss,
                "tier": risk.tier,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);

    let app_data = super::resolve_app_or_exit(&client, app_name);
    let name = &app_data.name;
    let app_id = &app_data.id;

    // Accept truncated UUIDs from `floo deploys list` output (`fed461a6...`)
    // via git-style prefix resolution. Without this, the API returns a raw
    // FastAPI validation array that surfaces as an unformatted error.
    let deploy_id = super::deploys::resolve_deploy_id(&client, app_id, deploy_id);
    let deploy_id = deploy_id.as_str();

    match confirm_tier2("Rollback", &format!("{name} to deploy {deploy_id}"), yes) {
        ConfirmOutcome::Proceed => {}
        ConfirmOutcome::Aborted => {
            if output::is_json_mode() {
                output::success("Cancelled.", Some(serde_json::json!({"cancelled": true})));
            } else {
                output::info("Cancelled.", None);
            }
            process::exit(0);
        }
        ConfirmOutcome::Refused { suggestion } => {
            crate::confirm::exit_refused(
                &format!(
                    "Refusing to rollback '{name}' to deploy {deploy_id} without explicit confirmation."
                ),
                &suggestion,
            );
        }
    }

    let spinner = output::Spinner::new("Rolling back...");

    let result = match client.rollback_deploy(app_id, deploy_id) {
        Ok(r) => r,
        Err(e) => {
            spinner.finish();
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    spinner.finish();

    let risk: RiskMetadata = Tier::Two.into();
    output::success(
        &format!("Rolled back {name} to deploy {deploy_id}."),
        Some(serde_json::json!({
            "deploy": result,
            "destructive": risk.destructive,
            "data_loss": risk.data_loss,
            "tier": risk.tier,
        })),
    );
}
