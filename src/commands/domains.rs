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

pub fn add(hostname: &str, app_name: &str) {
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

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    let result = match client.add_domain(app_id, hostname) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(&format!("Added {hostname}"), Some(result));
    } else {
        output::success(
            &format!("Added domain {hostname} to {name}."),
            Some(serde_json::Value::Null),
        );
        let status = result.get("status").and_then(|v| v.as_str()).unwrap_or("-");
        output::info(&format!("  Status: {status}"), None);
        if let Some(dns) = result.get("dns_instructions").and_then(|v| v.as_str()) {
            output::info(&format!("  DNS:    {dns}"), None);
        }
    }
}

pub fn list(app_name: &str) {
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

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    let result = match client.list_domains(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let domains = result
        .get("domains")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if domains.is_empty() {
        if !output::is_json_mode() {
            output::info(
                &format!(
                    "No custom domains on {name}. Add one with floo domains add example.com --app {name}."
                ),
                None,
            );
        } else {
            output::success("No domains.", Some(serde_json::json!({"domains": []})));
        }
        return;
    }

    let rows: Vec<Vec<String>> = domains
        .iter()
        .map(|d| {
            vec![
                d.get("hostname")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                d.get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                d.get("dns_instructions")
                    .and_then(|v| v.as_str())
                    .unwrap_or("\u{2014}")
                    .to_string(),
            ]
        })
        .collect();

    output::table(
        &["Domain", "Status", "DNS"],
        &rows,
        Some(serde_json::json!({"domains": domains})),
    );
}

pub fn remove(hostname: &str, app_name: &str) {
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

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    if let Err(e) = client.delete_domain(app_id, hostname) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }

    output::success(
        &format!("Removed domain {hostname} from {name}."),
        Some(serde_json::json!({"hostname": hostname})),
    );
}
