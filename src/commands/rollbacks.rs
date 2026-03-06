use std::process;

use crate::errors::ErrorCode;
use crate::output;

pub fn rollback(app_name: &str, deploy_id: &str, force: bool) {
    if output::is_dry_run_mode() {
        output::success(
            "Dry run — no changes made.",
            Some(serde_json::json!({
                "action": "rollback",
                "app": app_name,
                "to_deploy": deploy_id,
            })),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);

    let app_data = super::resolve_app_or_exit(&client, app_name);
    let name = &app_data.name;
    let app_id = &app_data.id;

    if !force
        && !output::confirm(&format!(
            "Rollback '{name}' to deploy {deploy_id}? This will replace the current version."
        ))
    {
        if output::is_json_mode() {
            output::success("Cancelled.", Some(serde_json::json!({"cancelled": true})));
        } else {
            output::info("Cancelled.", None);
        }
        process::exit(0);
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

    output::success(
        &format!("Rolled back {name} to deploy {deploy_id}."),
        Some(serde_json::json!({"deploy": result})),
    );
}
