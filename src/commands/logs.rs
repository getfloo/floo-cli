use std::path::Path;
use std::process;
use std::thread;
use std::time::Duration;

use colored::Colorize;

use crate::errors::ErrorCode;
use crate::output;
use crate::project_config::{self, AppSource};
use crate::resolve::resolve_app;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_TRANSIENT_RETRIES: u32 = 3;

struct LogsFilter<'a> {
    since: Option<&'a str>,
    severity: Option<&'a str>,
    services: &'a [String],
    search: Option<&'a str>,
}

fn extract_field<'a>(entry: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    entry.get(key).and_then(|v| v.as_str())
}

fn format_log_line(entry: &serde_json::Value, show_service_prefix: bool) -> String {
    let ts = extract_field(entry, "timestamp").unwrap_or("");
    let sev = extract_field(entry, "severity").unwrap_or("DEFAULT");
    let msg = extract_field(entry, "message").unwrap_or("");
    let colored_sev = match sev {
        "ERROR" | "CRITICAL" => sev.red().bold().to_string(),
        "WARNING" => sev.yellow().to_string(),
        "INFO" => sev.cyan().to_string(),
        "DEBUG" => sev.dimmed().to_string(),
        _ => sev.to_string(),
    };

    if show_service_prefix {
        let service = extract_field(entry, "service_name").unwrap_or("unknown");
        let colored_service = format!("[{}]", service).blue().bold().to_string();
        format!("{colored_service} {ts} [{colored_sev}] {msg}")
    } else {
        format!("{ts} [{colored_sev}] {msg}")
    }
}

fn format_log_line_plain(entry: &serde_json::Value, show_service_prefix: bool) -> String {
    let ts = extract_field(entry, "timestamp").unwrap_or("");
    let sev = extract_field(entry, "severity").unwrap_or("DEFAULT");
    let msg = extract_field(entry, "message").unwrap_or("");

    if show_service_prefix {
        let svc = extract_field(entry, "service_name").unwrap_or("unknown");
        format!("[{svc}] {ts} [{sev}] {msg}")
    } else {
        format!("{ts} [{sev}] {msg}")
    }
}

fn apply_search_filter(entries: Vec<serde_json::Value>, search: &str) -> Vec<serde_json::Value> {
    let needle = search.to_lowercase();
    entries
        .into_iter()
        .filter(|entry| {
            extract_field(entry, "message")
                .map(|msg| msg.to_lowercase().contains(&needle))
                .unwrap_or(false)
        })
        .collect()
}

fn fetch_logs(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    tail: u32,
    filter: &LogsFilter,
) -> Vec<serde_json::Value> {
    if filter.services.len() <= 1 {
        let service = filter.services.first().map(|s| s.as_str());
        let result = match client.get_logs(app_id, tail, filter.since, filter.severity, service) {
            Ok(r) => r,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };
        parse_logs_array(&result)
    } else {
        let mut all_logs = Vec::new();
        for svc in filter.services {
            let result =
                match client.get_logs(app_id, tail, filter.since, filter.severity, Some(svc)) {
                    Ok(r) => r,
                    Err(e) => {
                        output::error(
                            &format!("Failed to fetch logs for service '{}': {}", svc, e.message),
                            &ErrorCode::from_api(&e.code),
                            None,
                        );
                        process::exit(1);
                    }
                };
            all_logs.extend(parse_logs_array(&result));
        }
        all_logs.sort_by(|a, b| {
            let ts_a = extract_field(a, "timestamp").unwrap_or("");
            let ts_b = extract_field(b, "timestamp").unwrap_or("");
            ts_a.cmp(ts_b)
        });
        all_logs
    }
}

fn try_fetch_logs(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    tail: u32,
    filter: &LogsFilter,
) -> Result<Vec<serde_json::Value>, crate::errors::FlooApiError> {
    if filter.services.len() <= 1 {
        let service = filter.services.first().map(|s| s.as_str());
        let result = client.get_logs(app_id, tail, filter.since, filter.severity, service)?;
        Ok(parse_logs_array(&result))
    } else {
        let mut all_logs = Vec::new();
        for svc in filter.services {
            let result = client.get_logs(app_id, tail, filter.since, filter.severity, Some(svc))?;
            all_logs.extend(parse_logs_array(&result));
        }
        all_logs.sort_by(|a, b| {
            let ts_a = extract_field(a, "timestamp").unwrap_or("");
            let ts_b = extract_field(b, "timestamp").unwrap_or("");
            ts_a.cmp(ts_b)
        });
        Ok(all_logs)
    }
}

fn parse_logs_array(result: &serde_json::Value) -> Vec<serde_json::Value> {
    match result.get("logs").and_then(|v| v.as_array()) {
        Some(arr) => arr.clone(),
        None => {
            output::error(
                "Unexpected response format from the API.",
                &ErrorCode::ParseError,
                Some("Try updating the CLI: curl -fsSL https://getfloo.com/install.sh | bash"),
            );
            process::exit(1);
        }
    }
}

fn print_context_header(app_name: &str, source_label: &str, filter: &LogsFilter) {
    eprintln!();
    eprintln!("  {} {} (from {})", "App:".bold(), app_name, source_label);
    if filter.services.is_empty() {
        eprintln!("  {} all", "Services:".bold());
    } else {
        eprintln!("  {} {}", "Services:".bold(), filter.services.join(", "));
    }
    if let Some(s) = filter.since {
        eprintln!("  {} {}", "Since:".bold(), s);
    }
    if let Some(q) = filter.search {
        eprintln!("  {} \"{}\"", "Search:".bold(), q);
    }
    eprintln!("  {}", "\u{2500}".repeat(40).dimmed());
    eprintln!();
}

fn source_label(source: &AppSource, config_dir: &Path) -> String {
    match source {
        AppSource::Flag => "--app flag".to_string(),
        AppSource::ServiceFile => {
            format!(
                "{} in {}",
                project_config::SERVICE_CONFIG_FILE,
                config_dir.display()
            )
        }
        AppSource::AppFile => {
            format!(
                "{} in {}",
                project_config::APP_CONFIG_FILE,
                config_dir.display()
            )
        }
    }
}

fn update_last_timestamp(last: &mut Option<String>, entry: &serde_json::Value) {
    if let Some(ts) = extract_field(entry, "timestamp") {
        let is_newer = match last.as_deref() {
            Some(prev) => ts > prev,
            None => true,
        };
        if is_newer {
            *last = Some(ts.to_string());
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn logs(
    app_flag: Option<&str>,
    tail: u32,
    since: Option<&str>,
    severity: Option<&str>,
    services: &[String],
    search: Option<&str>,
    live: bool,
    output_path: Option<&Path>,
) {
    super::require_auth();
    let client = super::init_client(None);

    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    });

    let resolved = match project_config::resolve_app_context(&cwd, app_flag) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let app_name = &resolved.app_name;
    let src_label = source_label(&resolved.source, &resolved.config_dir);

    let app_data = match resolve_app(&client, app_name) {
        Ok(a) => a,
        Err(e) => {
            if e.code == "APP_NOT_FOUND" {
                output::error(
                    &format!("App '{app_name}' not found."),
                    &ErrorCode::AppNotFound,
                    Some("Check the app name or ID and try again."),
                );
            } else {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            }
            process::exit(1);
        }
    };

    let app_id = super::expect_str_field(&app_data, "id");
    let show_service_prefix = services.len() != 1;

    let filter = LogsFilter {
        since,
        severity,
        services,
        search,
    };

    if !output::is_json_mode() {
        print_context_header(app_name, &src_label, &filter);
    }

    if live {
        live_logs(&client, app_id, tail, &filter, show_service_prefix);
    } else {
        batch_logs(
            &client,
            app_id,
            app_name,
            tail,
            &filter,
            show_service_prefix,
            output_path,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn batch_logs(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    app_name: &str,
    tail: u32,
    filter: &LogsFilter,
    show_service_prefix: bool,
    output_path: Option<&Path>,
) {
    let mut logs = fetch_logs(client, app_id, tail, filter);

    if let Some(q) = filter.search {
        logs = apply_search_filter(logs, q);
    }

    if logs.is_empty() {
        if output::is_json_mode() {
            output::success(
                "No logs found.",
                Some(serde_json::json!({
                    "logs": [],
                    "total": 0,
                    "app_name": app_name,
                })),
            );
        } else {
            output::info(
                "No logs found. The app may not have produced any output yet.",
                None,
            );
        }
        return;
    }

    if let Some(path) = output_path {
        write_logs_to_file(path, &logs, app_name, show_service_prefix);
        return;
    }

    if output::is_json_mode() {
        let total = logs.len();
        output::success(
            "Logs retrieved.",
            Some(serde_json::json!({
                "logs": logs,
                "total": total,
                "app_name": app_name,
            })),
        );
        return;
    }

    for entry in &logs {
        output::dim_line(&format_log_line(entry, show_service_prefix));
    }
}

fn write_logs_to_file(
    path: &Path,
    logs: &[serde_json::Value],
    app_name: &str,
    show_service_prefix: bool,
) {
    let content = if output::is_json_mode() {
        let payload = serde_json::json!({
            "logs": logs,
            "total": logs.len(),
            "app_name": app_name,
        });
        match serde_json::to_string_pretty(&payload) {
            Ok(s) => s,
            Err(e) => {
                output::error(
                    &format!("Failed to serialize logs to JSON: {e}"),
                    &ErrorCode::ParseError,
                    None,
                );
                process::exit(1);
            }
        }
    } else {
        logs.iter()
            .map(|entry| format_log_line_plain(entry, show_service_prefix))
            .collect::<Vec<_>>()
            .join("\n")
    };

    if let Err(e) = std::fs::write(path, &content) {
        output::error(
            &format!("Failed to write logs to {}: {e}", path.display()),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    }

    let count = logs.len();
    if output::is_json_mode() {
        output::success(
            &format!("Wrote {count} log entries to {}", path.display()),
            Some(serde_json::json!({
                "logs": logs,
                "total": count,
                "app_name": app_name,
            })),
        );
    } else {
        output::success(
            &format!("Wrote {count} log entries to {}", path.display()),
            None,
        );
    }
}

fn live_logs(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    tail: u32,
    filter: &LogsFilter,
    show_service_prefix: bool,
) {
    let is_json = output::is_json_mode();

    let mut logs = fetch_logs(client, app_id, tail, filter);
    if let Some(q) = filter.search {
        logs = apply_search_filter(logs, q);
    }

    let mut last_timestamp: Option<String> = None;

    for entry in &logs {
        if is_json {
            output::print_json(entry);
        } else {
            output::dim_line(&format_log_line(entry, show_service_prefix));
        }
        update_last_timestamp(&mut last_timestamp, entry);
    }

    if !is_json && logs.is_empty() {
        output::info("Waiting for new logs...", None);
    }

    let mut consecutive_errors: u32 = 0;

    loop {
        thread::sleep(POLL_INTERVAL);

        let since_filter = last_timestamp.as_deref().or(filter.since);

        let poll_filter = LogsFilter {
            since: since_filter,
            severity: filter.severity,
            services: filter.services,
            search: None,
        };

        let poll_result = try_fetch_logs(client, app_id, tail, &poll_filter);
        let mut new_logs = match poll_result {
            Ok(logs) => {
                consecutive_errors = 0;
                logs
            }
            Err(e) => {
                consecutive_errors += 1;
                if consecutive_errors >= MAX_TRANSIENT_RETRIES {
                    output::error(
                        &format!(
                            "Live polling failed after {} consecutive errors: {}",
                            MAX_TRANSIENT_RETRIES, e.message
                        ),
                        &ErrorCode::from_api(&e.code),
                        Some("Check your network connection and try again."),
                    );
                    process::exit(1);
                }
                if !is_json {
                    eprintln!(
                        "  {} transient error (retry {}/{}): {}",
                        "!".yellow().bold(),
                        consecutive_errors,
                        MAX_TRANSIENT_RETRIES,
                        e.message
                    );
                }
                continue;
            }
        };

        if let Some(q) = filter.search {
            new_logs = apply_search_filter(new_logs, q);
        }

        let new_entries: Vec<_> = match &last_timestamp {
            None => new_logs,
            Some(last_ts) => new_logs
                .into_iter()
                .filter(|entry| {
                    extract_field(entry, "timestamp")
                        .map(|ts| ts > last_ts.as_str())
                        .unwrap_or(false)
                })
                .collect(),
        };

        for entry in &new_entries {
            if is_json {
                output::print_json(entry);
            } else {
                output::dim_line(&format_log_line(entry, show_service_prefix));
            }
            update_last_timestamp(&mut last_timestamp, entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_apply_search_filter_matches_case_insensitive() {
        let entries = vec![
            json!({"message": "Server started", "timestamp": "t1", "severity": "INFO"}),
            json!({"message": "Error: connection failed", "timestamp": "t2", "severity": "ERROR"}),
            json!({"message": "Request handled", "timestamp": "t3", "severity": "INFO"}),
        ];

        let result = apply_search_filter(entries, "error");
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].get("message").unwrap().as_str().unwrap(),
            "Error: connection failed"
        );
    }

    #[test]
    fn test_apply_search_filter_empty_query_matches_all() {
        let entries = vec![json!({"message": "Hello", "timestamp": "t1", "severity": "INFO"})];

        let result = apply_search_filter(entries, "");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_apply_search_filter_no_match() {
        let entries =
            vec![json!({"message": "Hello world", "timestamp": "t1", "severity": "INFO"})];

        let result = apply_search_filter(entries, "zzz_nonexistent");
        assert!(result.is_empty());
    }

    #[test]
    fn test_apply_search_filter_missing_message_field() {
        let entries = vec![json!({"timestamp": "t1", "severity": "INFO"})];

        let result = apply_search_filter(entries, "anything");
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_log_line_without_prefix() {
        let entry = json!({
            "timestamp": "2024-01-01T00:00:00Z",
            "severity": "INFO",
            "message": "Server started",
            "service_name": "api"
        });

        let line = format_log_line(&entry, false);
        assert!(line.contains("2024-01-01T00:00:00Z"));
        assert!(line.contains("Server started"));
        assert!(!line.contains("[api]"));
    }

    #[test]
    fn test_format_log_line_with_prefix() {
        let entry = json!({
            "timestamp": "2024-01-01T00:00:00Z",
            "severity": "INFO",
            "message": "Server started",
            "service_name": "api"
        });

        let line = format_log_line(&entry, true);
        assert!(line.contains("2024-01-01T00:00:00Z"));
        assert!(line.contains("Server started"));
        assert!(line.contains("[api]"));
    }

    #[test]
    fn test_format_log_line_with_prefix_missing_service_name() {
        let entry = json!({
            "timestamp": "2024-01-01T00:00:00Z",
            "severity": "ERROR",
            "message": "something broke"
        });

        let line = format_log_line(&entry, true);
        assert!(line.contains("[unknown]"));
        assert!(line.contains("something broke"));
    }
}
