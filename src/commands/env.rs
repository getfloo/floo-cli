use std::process;

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

pub fn set(key_value: &str, app_name: &str) {
    require_auth();

    if !key_value.contains('=') {
        output::error(
            "Invalid format. Use KEY=VALUE.",
            "INVALID_FORMAT",
            Some("Example: floo env set DATABASE_URL=postgres://..."),
        );
        process::exit(1);
    }

    let (key, value) = key_value.split_once('=').unwrap();
    let key = key.to_uppercase();

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

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    match client.set_env_var(app_id, &key, value) {
        Ok(result) => {
            output::success(&format!("Set {key} on {name}."), Some(result));
        }
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    }
}

pub fn list(app_name: &str) {
    require_auth();
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

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    let result = match client.list_env_vars(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let env_vars = result
        .get("env_vars")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if env_vars.is_empty() {
        if !output::is_json_mode() {
            output::info(
                &format!(
                    "No environment variables set on {name}. Set one with floo env set KEY=VALUE --app {name}."
                ),
                None,
            );
        } else {
            output::success("No env vars.", Some(serde_json::json!({"env_vars": []})));
        }
        return;
    }

    let rows: Vec<Vec<String>> = env_vars
        .iter()
        .map(|ev| {
            vec![
                ev.get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                ev.get("masked_value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
            ]
        })
        .collect();

    output::table(
        &["Key", "Value"],
        &rows,
        Some(serde_json::json!({"env_vars": env_vars})),
    );
}

pub fn remove(key: &str, app_name: &str) {
    let key = key.to_uppercase();
    require_auth();

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

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    if let Err(e) = client.delete_env_var(app_id, &key) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }

    output::success(
        &format!("Removed {key} from {name}."),
        Some(serde_json::json!({"key": key})),
    );
}
