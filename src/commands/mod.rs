use std::process;

use crate::api_client::FlooClient;
use crate::config::{load_config, FlooConfig};
use crate::output;

pub mod analytics;
pub mod apps;
pub mod auth;
pub mod billing;
pub mod check;
pub mod deploy;
pub mod domains;
pub mod env;
pub mod init;
pub mod logs;
pub mod orgs;
pub mod releases;
pub mod rollbacks;
pub mod service_mgmt;
pub mod services;
pub mod skills;
pub mod update;

pub(crate) fn init_client(config: Option<FlooConfig>) -> FlooClient {
    match FlooClient::new(config) {
        Ok(client) => client,
        Err(error) => {
            output::error(
                &error.message,
                &error.code,
                Some("Check your network/TLS setup and try again."),
            );
            process::exit(1);
        }
    }
}

pub(crate) fn require_auth() {
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

pub(crate) fn expect_str_field<'a>(data: &'a serde_json::Value, field: &str) -> &'a str {
    let value = data.get(field).and_then(|v| v.as_str()).unwrap_or_else(|| {
        output::error(
            &format!("Response missing '{field}' field."),
            "PARSE_ERROR",
            Some("This is a bug. Please report it."),
        );
        process::exit(1);
    });
    if value.is_empty() {
        output::error(
            &format!("Response field '{field}' is empty."),
            "PARSE_ERROR",
            Some("This may indicate a CLI/API version mismatch. Try `floo update`."),
        );
        process::exit(1);
    }
    value
}

pub(crate) fn resolve_app_or_exit(client: &FlooClient, app_name: &str) -> serde_json::Value {
    match crate::resolve::resolve_app(client, app_name) {
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
    }
}

pub(crate) fn resolve_app_from_config(
    client: &FlooClient,
    app_flag: Option<&str>,
) -> (String, String) {
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            "CWD_ERROR",
            Some("Ensure the current directory exists and you have read permission."),
        );
        process::exit(1);
    });
    let resolved = match crate::project_config::resolve_app_context(&cwd, app_flag) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };
    let app_data = resolve_app_or_exit(client, &resolved.app_name);
    let app_id = expect_str_field(&app_data, "id").to_string();
    let app_name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&resolved.app_name)
        .to_string();
    (app_id, app_name)
}

/// Detect the deploy env file in a directory: prefers .floo.env, falls back to .env.
/// Used at config creation time (init, service add) to populate env_file in floo.service.toml.
pub(crate) fn detect_env_file(dir: &std::path::Path) -> Option<String> {
    if dir.join(".floo.env").exists() {
        Some(".floo.env".to_string())
    } else if dir.join(".env").exists() {
        Some(".env".to_string())
    } else {
        None
    }
}
