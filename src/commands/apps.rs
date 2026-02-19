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

pub fn list() {
    require_auth();
    let client = FlooClient::new(None);
    let result = match client.list_apps(1, 20) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let apps = result
        .get("apps")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if apps.is_empty() {
        if !output::is_json_mode() {
            output::info("No apps yet. Deploy one with floo deploy.", None);
        } else {
            output::success("No apps.", Some(serde_json::json!({"apps": []})));
        }
        return;
    }

    let rows: Vec<Vec<String>> = apps
        .iter()
        .map(|a| {
            vec![
                a.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                a.get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                a.get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("\u{2014}")
                    .to_string(),
                a.get("runtime")
                    .and_then(|v| v.as_str())
                    .unwrap_or("\u{2014}")
                    .to_string(),
                a.get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
            ]
        })
        .collect();

    output::table(
        &["Name", "Status", "URL", "Runtime", "Created"],
        &rows,
        Some(serde_json::json!({"apps": apps})),
    );
}

pub fn status(app_name: &str) {
    require_auth();
    let client = FlooClient::new(None);
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

    if output::is_json_mode() {
        let name = app_data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(app_name);
        output::success(&format!("App {name}"), Some(app_data));
    } else {
        let name = app_data.get("name").and_then(|v| v.as_str()).unwrap_or("-");
        let st = app_data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let url = app_data
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("\u{2014}");
        let runtime = app_data
            .get("runtime")
            .and_then(|v| v.as_str())
            .unwrap_or("\u{2014}");
        let id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("-");
        let created = app_data
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("-");

        output::info(name, None);
        output::info(&format!("  Status:   {st}"), None);
        output::info(&format!("  URL:      {url}"), None);
        output::info(&format!("  Runtime:  {runtime}"), None);
        output::info(&format!("  ID:       {id}"), None);
        output::info(&format!("  Created:  {created}"), None);
    }
}

pub fn delete(app_name: &str, force: bool) {
    require_auth();
    let client = FlooClient::new(None);
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

    if !force && !output::confirm(&format!("Delete app '{name}'? This cannot be undone.")) {
        if !output::is_json_mode() {
            output::info("Cancelled.", None);
        }
        process::exit(0);
    }

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");

    if let Err(e) = client.delete_app(app_id) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }

    output::success(
        &format!("Deleted app '{name}'."),
        Some(serde_json::json!({"id": app_id})),
    );
}
