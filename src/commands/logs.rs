use std::path::Path;
use std::process;

use colored::Colorize;

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

fn colorize_severity(severity: &str) -> String {
    match severity {
        "ERROR" | "CRITICAL" => severity.red().bold().to_string(),
        "WARNING" => severity.yellow().to_string(),
        "INFO" => severity.cyan().to_string(),
        "DEBUG" => severity.dimmed().to_string(),
        _ => severity.to_string(),
    }
}

pub fn logs(
    app_name: &str,
    tail: u32,
    since: Option<&str>,
    severity: Option<&str>,
    output_path: Option<&Path>,
) {
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

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");

    let result = match client.get_logs(app_id, tail, since, severity) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let logs = result
        .get("logs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

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
            serde_json::to_string_pretty(&result).unwrap_or_default()
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
        let ts = entry
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let sev = entry
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("DEFAULT");
        let msg = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
        eprintln!("  {ts} [{}] {msg}", colorize_severity(sev));
    }
}
