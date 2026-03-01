use std::process;

use crate::output;

pub fn list(app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, _app_name) = super::resolve_app_from_config(&client, app);
    let app_id = app_id.as_str();

    let result = match client.list_deploys(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    if result.deploys.is_empty() {
        if output::is_json_mode() {
            output::success(
                "No deploys found.",
                Some(serde_json::json!({"deploys": []})),
            );
        } else {
            output::info("No deploys found.", None);
        }
        return;
    }

    let rows: Vec<Vec<String>> = result
        .deploys
        .iter()
        .map(|d| {
            vec![
                d.id.clone(),
                d.status.as_deref().unwrap_or("-").to_string(),
                d.runtime.as_deref().unwrap_or("\u{2014}").to_string(),
                d.created_at.as_deref().unwrap_or("-").to_string(),
            ]
        })
        .collect();

    output::table(
        &["Deploy ID", "Status", "Runtime", "Created"],
        &rows,
        Some(output::to_value(&result)),
    );
}

pub fn rollback(app_name: &str, deploy_id: &str, force: bool) {
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
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    spinner.finish();

    output::success(
        &format!("Rolled back {name} to deploy {deploy_id}."),
        Some(serde_json::json!({"deploy": result})),
    );
}
