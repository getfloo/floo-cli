use std::process;

use crate::api_client::FlooClient;
use crate::api_types::App;
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

pub(crate) fn resolve_app_or_exit(client: &FlooClient, app_name: &str) -> App {
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
    let app = resolve_app_or_exit(client, &resolved.app_name);
    (app.id.clone(), app.name.clone())
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
