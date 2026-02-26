use std::env;
use std::process;

use crate::output;
use crate::project_config::resolve_app_context;
use crate::resolve::resolve_app;

pub fn list(app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let cwd = env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            "CWD_ERROR",
            Some("Ensure the current directory exists and you have read permission."),
        );
        process::exit(1);
    });
    let resolved = match resolve_app_context(&cwd, app) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let app_data = match resolve_app(&client, &resolved.app_name) {
        Ok(a) => a,
        Err(e) => {
            if e.code == "APP_NOT_FOUND" {
                output::error(
                    &format!("App '{}' not found.", resolved.app_name),
                    "APP_NOT_FOUND",
                    Some("Check the app name or ID and try again."),
                );
            } else {
                output::error(&e.message, &e.code, None);
            }
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
    super::require_auth();
    let client = super::init_client(None);

    let app_data = match resolve_app(&client, app_name) {
        Ok(a) => a,
        Err(e) => {
            if e.code == "APP_NOT_FOUND" {
                output::error(
                    &format!("App '{app_name}' not found."),
                    "APP_NOT_FOUND",
                    Some("Check the app name or ID and try again."),
                );
            } else {
                output::error(&e.message, &e.code, None);
            }
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
