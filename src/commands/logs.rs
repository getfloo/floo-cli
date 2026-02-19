use std::path::Path;
use std::process;

use colored::Colorize;

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

fn format_log_line(entry: &serde_json::Value) -> String {
    let ts = entry
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let sev = entry
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("DEFAULT");
    let msg = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let colored_sev = match sev {
        "ERROR" | "CRITICAL" => sev.red().bold().to_string(),
        "WARNING" => sev.yellow().to_string(),
        "INFO" => sev.cyan().to_string(),
        "DEBUG" => sev.dimmed().to_string(),
        _ => sev.to_string(),
    };
    format!("{ts} [{colored_sev}] {msg}")
}

pub fn logs(
    app_name: &str,
    tail: u32,
    since: Option<&str>,
    severity: Option<&str>,
    output_path: Option<&Path>,
) {
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

    let result = match client.get_logs(app_id, tail, since, severity) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let logs = match result.get("logs").and_then(|v| v.as_array()) {
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

    if logs.is_empty() {
        if output::is_json_mode() {
            output::success("No logs found.", Some(result));
        } else {
            output::info(
                "No logs found. The app may not have produced any output yet.",
                None,
            );
        }
        return;
    }

    if let Some(path) = output_path {
        let content = if output::is_json_mode() {
            match serde_json::to_string_pretty(&result) {
                Ok(s) => s,
                Err(e) => {
                    output::error(
                        &format!("Failed to serialize logs to JSON: {e}"),
                        "PARSE_ERROR",
                        None,
                    );
                    process::exit(1);
                }
            }
        } else {
            logs.iter()
                .map(|entry| {
                    let ts = entry
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let sev = entry
                        .get("severity")
                        .and_then(|v| v.as_str())
                        .unwrap_or("DEFAULT");
                    let msg = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    format!("{ts} [{sev}] {msg}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        if let Err(e) = std::fs::write(path, &content) {
            output::error(
                &format!("Failed to write logs to {}: {e}", path.display()),
                "FILE_ERROR",
                None,
            );
            process::exit(1);
        }

        let count = logs.len();
        if output::is_json_mode() {
            output::success(
                &format!("Wrote {count} log entries to {}", path.display()),
                Some(result),
            );
        } else {
            output::success(
                &format!("Wrote {count} log entries to {}", path.display()),
                None,
            );
        }
        return;
    }

    if output::is_json_mode() {
        output::success("Logs retrieved.", Some(result));
        return;
    }

    let app_display = result
        .get("app_name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);
    output::info(&format!("Logs for {app_display}:"), None);

    for entry in &logs {
        output::dim_line(&format_log_line(entry));
    }
}
