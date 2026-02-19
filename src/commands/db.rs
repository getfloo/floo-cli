use std::process;

use serde_json::Value;

use crate::api_client::FlooClient;
use crate::config::load_config;
use crate::errors::FlooApiError;
use crate::output;
use crate::resolve::resolve_app;

struct DatabaseInfo {
    host: String,
    port: u16,
    database: String,
    status: String,
    username: Option<String>,
    schema_name: Option<String>,
}

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

fn parse_required_string(value: &Value, key: &str) -> Result<String, FlooApiError> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            FlooApiError::new(
                500,
                "PARSE_ERROR",
                "Failed to parse database info response from API.",
            )
        })
}

fn parse_required_port(value: &Value) -> Result<u16, FlooApiError> {
    let raw_port = value.get("port").and_then(|v| v.as_u64()).ok_or_else(|| {
        FlooApiError::new(
            500,
            "PARSE_ERROR",
            "Failed to parse database info response from API.",
        )
    })?;

    u16::try_from(raw_port).map_err(|_| {
        FlooApiError::new(
            500,
            "PARSE_ERROR",
            "Failed to parse database info response from API.",
        )
    })
}

fn parse_database_info(value: &Value) -> Result<DatabaseInfo, FlooApiError> {
    Ok(DatabaseInfo {
        host: parse_required_string(value, "host")?,
        port: parse_required_port(value)?,
        database: parse_required_string(value, "database")?,
        status: parse_required_string(value, "status")?,
        username: value
            .get("username")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        schema_name: value
            .get("schema_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

pub fn info(app_identifier: &str) {
    require_auth();

    let client = FlooClient::new(None);
    let app_data = match resolve_app(&client, app_identifier) {
        Ok(app) => app,
        Err(error) => {
            if error.code == "APP_NOT_FOUND" {
                output::error(
                    &format!("App '{app_identifier}' not found."),
                    "APP_NOT_FOUND",
                    Some("Check the app name or ID and try again."),
                );
            } else {
                output::error(&error.message, &error.code, None);
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
    let app_name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_identifier);

    let db_data = match client.get_database_info(app_id) {
        Ok(db) => db,
        Err(error) => {
            output::error(&error.message, &error.code, None);
            process::exit(1);
        }
    };

    let parsed = match parse_database_info(&db_data) {
        Ok(details) => details,
        Err(error) => {
            output::error(
                &error.message,
                &error.code,
                Some("This may indicate a CLI/API version mismatch. Try updating the CLI."),
            );
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(&format!("Database for {app_name}"), Some(db_data));
        return;
    }

    output::info(&format!("Database for {app_name}:"), None);
    output::info(&format!("  Host:     {}", parsed.host), None);
    output::info(&format!("  Port:     {}", parsed.port), None);
    output::info(&format!("  Database: {}", parsed.database), None);
    output::info(&format!("  Status:   {}", parsed.status), None);
    if let Some(username) = parsed.username {
        output::info(&format!("  Username: {username}"), None);
    }
    if let Some(schema_name) = parsed.schema_name {
        output::info(&format!("  Schema:   {schema_name}"), None);
    }
    output::info("", None);
    output::info("DATABASE_URL is injected as an environment variable.", None);
}
