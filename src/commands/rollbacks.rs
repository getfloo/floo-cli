use std::process;

use crate::api_client::FlooClient;
use crate::config::load_config;
use crate::output;
use crate::resolve::resolve_app;

fn require_auth() {
    let config = load_config();
    if config.api_key.is_none() {
        output::error(
            "Not logged in.",
            "NOT_AUTHENTICATED",
            Some("Run 'floo login' to authenticate."),
        );
        process::exit(1);
    }
}

pub fn list(app_name: &str) {
    require_auth();
    let client = FlooClient::new(None);

    let app_data = match resolve_app(&client, app_name) {
        Some(a) => a,
        None => {
            output::error(
                &format!("App '{app_name}' not found."),
                "APP_NOT_FOUND",
                Some("Check the app name or ID and try again."),
            );
            process::exit(1);
        }
    };

    let app_id = match app_data.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id,
        _ => {
            output::error(
                "Failed to read app ID from API response.",
                "PARSE_ERROR",
                Some("This may indicate a CLI/API version mismatch. Try updating the CLI."),
            );
            process::exit(1);
        }
    };

    let result = match client.list_deploys(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let deploys = match result.get("deploys").and_then(|v| v.as_array()) {
        Some(arr) => arr.clone(),
        None => {
            output::error(
                "Unexpected response format from the API.",
                "PARSE_ERROR",
                Some("Try updating the CLI: curl -fsSL https://getfloo.com/install.sh | bash"),
            );
            process::exit(1);
        }
    };

    if deploys.is_empty() {
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

    let rows: Vec<Vec<String>> = deploys
        .iter()
        .map(|d| {
            vec![
                d.get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                d.get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                d.get("runtime")
                    .and_then(|v| v.as_str())
                    .unwrap_or("\u{2014}")
                    .to_string(),
                d.get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
            ]
        })
        .collect();

    output::table(
        &["Deploy ID", "Status", "Runtime", "Created"],
        &rows,
        Some(serde_json::json!({"deploys": deploys})),
    );
}

pub fn rollback(app_name: &str, deploy_id: &str, force: bool) {
    require_auth();
    let client = FlooClient::new(None);

    let app_data = match resolve_app(&client, app_name) {
        Some(a) => a,
        None => {
            output::error(
                &format!("App '{app_name}' not found."),
                "APP_NOT_FOUND",
                Some("Check the app name or ID and try again."),
            );
            process::exit(1);
        }
    };

    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    let app_id = match app_data.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id,
        _ => {
            output::error(
                "Failed to read app ID from API response.",
                "PARSE_ERROR",
                Some("This may indicate a CLI/API version mismatch. Try updating the CLI."),
            );
            process::exit(1);
        }
    };

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
