use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::Duration;

use colored::Colorize;

use crate::api_types::{LogEntry, LogsResponse, RequestLogEntry};
use crate::errors::ErrorCode;
use crate::output;
use crate::project_config::{self, AppSource};
use crate::resolve::resolve_app;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_TRANSIENT_RETRIES: u32 = 3;
/// Server-side cap on `limit` for both /logs and /requests (FastAPI `le=500`).
/// Clamp client-side so `--tail 9999` shows the most recent 500 with a clear
/// note instead of leaking a raw Pydantic validation error (#1155 finding #5).
const MAX_LOG_LINES: u32 = 500;

/// Clamp `--tail` to the server maximum, emitting a one-line note when it caps.
fn clamp_tail(tail: u32) -> u32 {
    if tail > MAX_LOG_LINES {
        if !output::is_json_mode() {
            output::info(
                &format!(
                    "--tail {tail} exceeds the maximum; showing the most recent {MAX_LOG_LINES} lines."
                ),
                None,
            );
        }
        MAX_LOG_LINES
    } else {
        tail
    }
}

pub struct LogsArgs {
    pub app_flag: Option<String>,
    pub tail: u32,
    pub since: Option<String>,
    pub severity: Option<String>,
    pub services: Vec<String>,
    pub cron: Option<String>,
    pub search: Option<String>,
    pub deployment: Option<String>,
    pub cursor: Option<String>,
    pub live: bool,
    pub output_path: Option<PathBuf>,
    pub env: Option<String>,
}

struct LogsFilter<'a> {
    since: Option<&'a str>,
    severity: Option<&'a str>,
    services: &'a [String],
    cron: Option<&'a str>,
    search: Option<&'a str>,
    deployment: Option<&'a str>,
    environment: Option<&'a str>,
    cursor: Option<&'a str>,
}

/// The attribution tag for a log line: cron jobs render as `cron:<name>`,
/// service entries as the service name. The API now always resolves a name
/// (single-service apps attribute to the app itself), so `unknown` is only a
/// last-resort guard for an entry that carries neither identity.
fn attribution_label(entry: &LogEntry) -> Option<String> {
    if let Some(cron) = entry.cron_job_name.as_deref() {
        return Some(format!("cron:{cron}"));
    }
    entry.service_name.clone()
}

/// Show the per-line `[attribution]` prefix only when the rendered set spans
/// more than one distinct service/cron. A single-service app (one label for
/// every line) renders clean, untagged lines instead of a redundant — and
/// previously `[unknown]` — prefix; a multi-service or service+cron mix tags
/// each line so the operator can tell them apart.
fn should_show_prefix(entries: &[LogEntry]) -> bool {
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        if let Some(label) = attribution_label(entry) {
            seen.insert(label);
            if seen.len() > 1 {
                return true;
            }
        }
    }
    false
}

fn format_log_line(entry: &LogEntry, show_service_prefix: bool) -> String {
    let ts = entry.timestamp.as_deref().unwrap_or("");
    let sev = entry.severity.as_deref().unwrap_or("DEFAULT");
    let msg = entry.message.as_deref().unwrap_or("");
    let colored_sev = match sev {
        "ERROR" | "CRITICAL" => sev.red().bold().to_string(),
        "WARNING" => sev.yellow().to_string(),
        "INFO" => sev.cyan().to_string(),
        "DEBUG" => sev.dimmed().to_string(),
        _ => sev.to_string(),
    };

    if show_service_prefix {
        let label = attribution_label(entry).unwrap_or_else(|| "unknown".to_string());
        let colored_label = format!("[{label}]").blue().bold().to_string();
        format!("{colored_label} {ts} [{colored_sev}] {msg}")
    } else {
        format!("{ts} [{colored_sev}] {msg}")
    }
}

fn format_log_line_plain(entry: &LogEntry, show_service_prefix: bool) -> String {
    let ts = entry.timestamp.as_deref().unwrap_or("");
    let sev = entry.severity.as_deref().unwrap_or("DEFAULT");
    let msg = entry.message.as_deref().unwrap_or("");

    if show_service_prefix {
        let label = attribution_label(entry).unwrap_or_else(|| "unknown".to_string());
        format!("[{label}] {ts} [{sev}] {msg}")
    } else {
        format!("{ts} [{sev}] {msg}")
    }
}

fn fetch_logs(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    tail: u32,
    filter: &LogsFilter,
) -> LogsResponse {
    if filter.services.len() <= 1 {
        let service = filter.services.first().map(|s| s.as_str());
        match client.get_logs(
            app_id,
            tail,
            filter.since,
            filter.severity,
            service,
            filter.cron,
            filter.search,
            filter.deployment,
            filter.environment,
            filter.cursor,
        ) {
            Ok(r) => r,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        }
    } else {
        let mut all_logs = Vec::new();
        for svc in filter.services {
            let result = match client.get_logs(
                app_id,
                tail,
                filter.since,
                filter.severity,
                Some(svc),
                None,
                filter.search,
                filter.deployment,
                filter.environment,
                filter.cursor,
            ) {
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
            all_logs.extend(result.logs);
        }
        all_logs.sort_by(|a, b| {
            let ts_a = a.timestamp.as_deref().unwrap_or("");
            let ts_b = b.timestamp.as_deref().unwrap_or("");
            ts_a.cmp(ts_b)
        });
        LogsResponse {
            total: Some(all_logs.len() as i32),
            app_name: None,
            limit: Some(tail),
            next_cursor: None,
            has_more: false,
            logs: all_logs,
        }
    }
}

fn try_fetch_logs(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    tail: u32,
    filter: &LogsFilter,
) -> Result<Vec<LogEntry>, crate::errors::FlooApiError> {
    if filter.services.len() <= 1 {
        let service = filter.services.first().map(|s| s.as_str());
        let result = client.get_logs(
            app_id,
            tail,
            filter.since,
            filter.severity,
            service,
            filter.cron,
            filter.search,
            filter.deployment,
            filter.environment,
            filter.cursor,
        )?;
        Ok(result.logs)
    } else {
        let mut all_logs = Vec::new();
        for svc in filter.services {
            let result = client.get_logs(
                app_id,
                tail,
                filter.since,
                filter.severity,
                Some(svc),
                None,
                filter.search,
                filter.deployment,
                filter.environment,
                filter.cursor,
            )?;
            all_logs.extend(result.logs);
        }
        all_logs.sort_by(|a, b| {
            let ts_a = a.timestamp.as_deref().unwrap_or("");
            let ts_b = b.timestamp.as_deref().unwrap_or("");
            ts_a.cmp(ts_b)
        });
        Ok(all_logs)
    }
}

fn print_context_header(app_name: &str, source_label: &str, filter: &LogsFilter) {
    eprintln!();
    eprintln!("  {} {} (from {})", "App:".bold(), app_name, source_label);
    if let Some(cron) = filter.cron {
        eprintln!("  {} {}", "Cron:".bold(), cron);
    } else if filter.services.is_empty() {
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
    if let Some(deployment) = filter.deployment {
        eprintln!("  {} {}", "Deployment:".bold(), deployment);
    }
    if filter.cursor.is_some() {
        eprintln!("  {} set", "Cursor:".bold());
    }
    if let Some(environment) = filter.environment {
        eprintln!("  {} {}", "Environment:".bold(), environment);
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

fn update_last_timestamp(last: &mut Option<String>, entry: &LogEntry) {
    if let Some(ts) = entry.timestamp.as_deref() {
        let is_newer = match last.as_deref() {
            Some(prev) => ts > prev,
            None => true,
        };
        if is_newer {
            *last = Some(ts.to_string());
        }
    }
}

pub fn logs(args: LogsArgs) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_data, src_label) = if let Some(ref app_flag) = args.app_flag {
        // --app provided: skip disk reads entirely
        let app = match resolve_app(&client, app_flag) {
            Ok(a) => a,
            Err(e) => {
                // 404 == app not found; gate on status, not a code string.
                if e.is_not_found() {
                    output::error(
                        &format!("App '{app_flag}' not found."),
                        &ErrorCode::AppNotFound,
                        Some("Check the app name or ID and try again."),
                    );
                } else {
                    output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                }
                process::exit(1);
            }
        };
        let label = format!("(--app {})", app.name);
        (app, label)
    } else {
        // No --app: resolve from local config files
        let cwd = super::read_cwd_or_exit();

        let resolved = match project_config::resolve_app_context(&cwd, None) {
            Ok(r) => r,
            Err(e) => {
                output::error(&e.message, &e.code, e.suggestion.as_deref());
                process::exit(1);
            }
        };

        let label = source_label(&resolved.source, &resolved.config_dir);
        let app = match resolve_app(&client, &resolved.app_name) {
            Ok(a) => a,
            Err(e) => {
                // 404 == app not found; gate on status, not a code string.
                if e.is_not_found() {
                    output::error(
                        &format!("App '{}' not found.", resolved.app_name),
                        &ErrorCode::AppNotFound,
                        Some("Check the app name or ID and try again."),
                    );
                } else {
                    output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                }
                process::exit(1);
            }
        };
        (app, label)
    };

    let app_name = &app_data.name;
    let app_id = &app_data.id;
    let tail = clamp_tail(args.tail);

    let filter = LogsFilter {
        since: args.since.as_deref(),
        severity: args.severity.as_deref(),
        services: &args.services,
        cron: args.cron.as_deref(),
        search: args.search.as_deref(),
        deployment: args.deployment.as_deref(),
        environment: args.env.as_deref(),
        cursor: args.cursor.as_deref(),
    };

    if !output::is_json_mode() {
        print_context_header(app_name, &src_label, &filter);
    }

    if args.live {
        live_logs(&client, app_id, tail, &filter);
    } else {
        batch_logs(
            &client,
            app_id,
            app_name,
            tail,
            &filter,
            args.output_path.as_deref(),
        );
    }
}

fn batch_logs(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    app_name: &str,
    tail: u32,
    filter: &LogsFilter,
    output_path: Option<&Path>,
) {
    let logs = fetch_logs(client, app_id, tail, filter);
    let mut response = logs;
    if response.app_name.is_none() {
        response.app_name = Some(app_name.to_string());
    }
    if response.limit.is_none() {
        response.limit = Some(tail);
    }

    let show_service_prefix = should_show_prefix(&response.logs);

    if response.logs.is_empty() {
        if output::is_json_mode() {
            output::success("No logs found.", Some(output::to_value(&response)));
        } else {
            output::info(
                "No logs found. The app may not have produced any output yet.",
                None,
            );
        }
        return;
    }

    if let Some(path) = output_path {
        write_logs_to_file(path, &response, show_service_prefix);
        return;
    }

    if output::is_json_mode() {
        output::success("Logs retrieved.", Some(output::to_value(&response)));
        return;
    }

    for entry in &response.logs {
        output::dim_line(&format_log_line(entry, show_service_prefix));
    }
}

fn write_logs_to_file(path: &Path, response: &LogsResponse, show_service_prefix: bool) {
    let content = if output::is_json_mode() {
        match serde_json::to_string_pretty(response) {
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
        response
            .logs
            .iter()
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

    let count = response.logs.len();
    if output::is_json_mode() {
        output::success(
            &format!("Wrote {count} log entries to {}", path.display()),
            Some(output::to_value(response)),
        );
    } else {
        output::success(
            &format!("Wrote {count} log entries to {}", path.display()),
            None,
        );
    }
}

fn live_logs(client: &crate::api_client::FlooClient, app_id: &str, tail: u32, filter: &LogsFilter) {
    let is_json = output::is_json_mode();

    let logs_response = fetch_logs(client, app_id, tail, filter);

    // Decide the prefix once from the initial page so the stream stays
    // visually stable: a single-service app never sprouts a prefix mid-stream,
    // a multi-service/service+cron mix keeps it for every line.
    let show_service_prefix = should_show_prefix(&logs_response.logs);

    let mut last_timestamp: Option<String> = None;

    for entry in &logs_response.logs {
        if is_json {
            output::print_json(&output::to_value(entry));
        } else {
            output::dim_line(&format_log_line(entry, show_service_prefix));
        }
        update_last_timestamp(&mut last_timestamp, entry);
    }

    if !is_json && logs_response.logs.is_empty() {
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
            cron: filter.cron,
            search: filter.search,
            deployment: filter.deployment,
            environment: filter.environment,
            cursor: None,
        };

        let poll_result = try_fetch_logs(client, app_id, tail, &poll_filter);
        let new_logs = match poll_result {
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

        let new_entries: Vec<_> = match &last_timestamp {
            None => new_logs,
            Some(last_ts) => new_logs
                .into_iter()
                .filter(|entry| {
                    entry
                        .timestamp
                        .as_deref()
                        .map(|ts| ts > last_ts.as_str())
                        .unwrap_or(false)
                })
                .collect(),
        };

        for entry in &new_entries {
            if is_json {
                output::print_json(&output::to_value(entry));
            } else {
                output::dim_line(&format_log_line(entry, show_service_prefix));
            }
            update_last_timestamp(&mut last_timestamp, entry);
        }
    }
}

// ─── floo logs --requests ──────────────────────────────────────────────────

pub struct RequestLogsArgs {
    pub app_flag: Option<String>,
    pub tail: u32,
    pub since: Option<String>,
    /// Stream new requests as they arrive (poll every 2s). Mirrors `--live`
    /// for app logs; `--follow` is accepted as an alias on the parent flag.
    pub live: bool,
}

fn format_request_line(entry: &RequestLogEntry) -> String {
    let status_str = entry.status_code.to_string();
    let colored_status = match entry.status_code / 100 {
        2 => status_str.green().to_string(),
        3 => status_str.cyan().to_string(),
        4 => status_str.yellow().to_string(),
        5 => status_str.red().bold().to_string(),
        _ => status_str,
    };
    let latency = if entry.latency_ms < 1000 {
        format!("{}ms", entry.latency_ms)
    } else {
        format!("{:.2}s", entry.latency_ms as f64 / 1000.0)
    };
    let url = match (&entry.host, &entry.path) {
        (Some(h), Some(p)) => format!("https://{h}{p}"),
        (Some(h), None) => format!("https://{h}"),
        (None, Some(p)) => p.clone(),
        (None, None) => "-".to_string(),
    };
    format!(
        "{} {} {} {} ({})",
        entry.timestamp,
        entry.method.bold(),
        colored_status,
        url,
        latency,
    )
}

pub fn request_logs(args: RequestLogsArgs) {
    super::require_auth();
    let client = super::init_client(None);

    let app_data = if let Some(ref app_flag) = args.app_flag {
        match resolve_app(&client, app_flag) {
            Ok(a) => a,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        }
    } else {
        let cwd = super::read_cwd_or_exit();
        let resolved = match project_config::resolve_app_context(&cwd, None) {
            Ok(r) => r,
            Err(e) => {
                output::error(&e.message, &e.code, e.suggestion.as_deref());
                process::exit(1);
            }
        };
        match resolve_app(&client, &resolved.app_name) {
            Ok(a) => a,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        }
    };

    let tail = clamp_tail(args.tail);

    if args.live {
        live_request_logs(
            &client,
            &app_data.id,
            &app_data.name,
            tail,
            args.since.as_deref(),
        );
        return;
    }

    let result = match client.get_request_logs(&app_data.id, tail, args.since.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            output::error(
                &format!("Failed to fetch request logs: {e}"),
                &ErrorCode::from_api("REQUESTS_FETCH_FAILED"),
                None,
            );
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        let message = if result.requests.is_empty() {
            "No requests found."
        } else {
            "Request logs retrieved."
        };
        output::success(message, Some(output::to_value(&result)));
        return;
    }

    if result.requests.is_empty() {
        output::dim_line(&format!(
            "No requests in the last 7 days for {}.",
            app_data.name
        ));
        output::info(
            "Gateway request logs are retained ~7 days. For longer windows use \
             `floo analytics` (aggregated totals up to 90 days).",
            None,
        );
        return;
    }

    // Newest-first from the API — reverse so the console reads top-to-bottom
    // oldest-to-newest, like `tail -f`.
    for entry in result.requests.iter().rev() {
        output::dim_line(&format_request_line(entry));
    }
    // The two surfaces read different stores (raw 7-day requests vs aggregated
    // 90-day analytics), so they legitimately differ for older traffic. Make
    // the horizon explicit rather than leaving "analytics shows 145, requests
    // shows fewer" a silent contradiction (#1155 finding #3).
    output::info(
        "Showing raw gateway requests from the last ~7 days. `floo analytics` \
         aggregates totals over a longer window.",
        None,
    );
}

fn live_request_logs(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    app_name: &str,
    tail: u32,
    initial_since: Option<&str>,
) {
    let is_json = output::is_json_mode();

    let initial = match client.get_request_logs(app_id, tail, initial_since) {
        Ok(r) => r,
        Err(e) => {
            output::error(
                &format!("Failed to fetch request logs: {e}"),
                &ErrorCode::from_api("REQUESTS_FETCH_FAILED"),
                None,
            );
            process::exit(1);
        }
    };

    let mut last_timestamp: Option<String> = None;
    // The API returns newest-first; reverse so the console reads
    // oldest-to-newest like `tail -f`.
    for entry in initial.requests.iter().rev() {
        emit_request_entry(entry, is_json);
        update_last_request_timestamp(&mut last_timestamp, entry);
    }

    if !is_json && initial.requests.is_empty() {
        output::dim_line(&format!("No requests yet for {app_name}. Waiting…"));
    }

    let mut consecutive_errors: u32 = 0;

    loop {
        thread::sleep(POLL_INTERVAL);

        // Once we've seen any request, we anchor on its timestamp; before
        // then, fall back to the user-provided --since window so the first
        // few polls don't re-stream the entire backlog.
        let since_filter = last_timestamp.as_deref().or(initial_since);

        let poll_result = client.get_request_logs(app_id, tail, since_filter);
        let new_logs = match poll_result {
            Ok(r) => {
                consecutive_errors = 0;
                r.requests
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

        // Strict-greater filter so a request that exactly matches the last
        // anchor isn't re-emitted. The API's `since` is inclusive
        // (>= since_dt) and timestamp resolution is microseconds, so without
        // this filter every poll cycle could re-print the most recent line.
        let new_entries: Vec<&RequestLogEntry> = match &last_timestamp {
            None => new_logs.iter().rev().collect(),
            Some(last_ts) => new_logs
                .iter()
                .rev()
                .filter(|entry| entry.timestamp.as_str() > last_ts.as_str())
                .collect(),
        };

        for entry in &new_entries {
            emit_request_entry(entry, is_json);
            update_last_request_timestamp(&mut last_timestamp, entry);
        }
    }
}

fn emit_request_entry(entry: &RequestLogEntry, is_json: bool) {
    if is_json {
        output::print_json(&output::to_value(entry));
    } else {
        output::dim_line(&format_request_line(entry));
    }
}

fn update_last_request_timestamp(last: &mut Option<String>, entry: &RequestLogEntry) {
    let ts = entry.timestamp.as_str();
    let is_newer = match last.as_deref() {
        Some(prev) => ts > prev,
        None => true,
    };
    if is_newer {
        *last = Some(ts.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_log_with_service(ts: &str, sev: &str, msg: &str, svc: &str) -> LogEntry {
        LogEntry {
            timestamp: Some(ts.to_string()),
            severity: Some(sev.to_string()),
            message: Some(msg.to_string()),
            deployment_id: None,
            request_id: None,
            labels: None,
            service_name: Some(svc.to_string()),
            cron_job_name: None,
            deploy_context: None,
            severity_class: None,
            lifecycle_noise: None,
        }
    }

    #[test]
    fn test_format_log_line_without_prefix() {
        let entry = make_log_with_service("2024-01-01T00:00:00Z", "INFO", "Server started", "api");

        let line = format_log_line(&entry, false);
        assert!(line.contains("2024-01-01T00:00:00Z"));
        assert!(line.contains("Server started"));
        assert!(!line.contains("[api]"));
    }

    #[test]
    fn test_format_log_line_with_prefix() {
        let entry = make_log_with_service("2024-01-01T00:00:00Z", "INFO", "Server started", "api");

        let line = format_log_line(&entry, true);
        assert!(line.contains("2024-01-01T00:00:00Z"));
        assert!(line.contains("Server started"));
        assert!(line.contains("[api]"));
    }

    #[test]
    fn test_format_log_line_with_prefix_missing_service_name() {
        let entry = LogEntry {
            timestamp: Some("2024-01-01T00:00:00Z".to_string()),
            severity: Some("ERROR".to_string()),
            message: Some("something broke".to_string()),
            deployment_id: None,
            request_id: None,
            labels: None,
            service_name: None,
            cron_job_name: None,
            deploy_context: None,
            severity_class: None,
            lifecycle_noise: None,
        };

        let line = format_log_line(&entry, true);
        assert!(line.contains("[unknown]"));
        assert!(line.contains("something broke"));
    }

    fn make_log_with_cron(ts: &str, sev: &str, msg: &str, cron: &str) -> LogEntry {
        LogEntry {
            timestamp: Some(ts.to_string()),
            severity: Some(sev.to_string()),
            message: Some(msg.to_string()),
            deployment_id: None,
            request_id: None,
            labels: None,
            service_name: None,
            cron_job_name: Some(cron.to_string()),
            deploy_context: None,
            severity_class: None,
            lifecycle_noise: None,
        }
    }

    #[test]
    fn test_attribution_label_prefers_cron() {
        let cron = make_log_with_cron("t", "INFO", "tick", "nightly-report");
        assert_eq!(
            attribution_label(&cron).as_deref(),
            Some("cron:nightly-report")
        );
        let svc = make_log_with_service("t", "INFO", "req", "web");
        assert_eq!(attribution_label(&svc).as_deref(), Some("web"));
    }

    #[test]
    fn test_format_log_line_renders_cron_prefix() {
        let entry = make_log_with_cron("2024-01-01T00:00:00Z", "INFO", "cron ran", "nightly");
        let line = format_log_line(&entry, true);
        assert!(line.contains("[cron:nightly]"));
        assert!(line.contains("cron ran"));
    }

    #[test]
    fn test_should_show_prefix_single_service_is_clean() {
        // A single-service app: every line attributes to the same name (the
        // app), so no redundant per-line prefix — and never [unknown].
        let entries = vec![
            make_log_with_service("t1", "INFO", "a", "my-app"),
            make_log_with_service("t2", "INFO", "b", "my-app"),
        ];
        assert!(!should_show_prefix(&entries));
    }

    #[test]
    fn test_should_show_prefix_multi_service_tags_lines() {
        let entries = vec![
            make_log_with_service("t1", "INFO", "a", "web"),
            make_log_with_service("t2", "INFO", "b", "worker"),
        ];
        assert!(should_show_prefix(&entries));
    }

    #[test]
    fn test_should_show_prefix_service_plus_cron_mix() {
        let entries = vec![
            make_log_with_service("t1", "INFO", "a", "my-app"),
            make_log_with_cron("t2", "INFO", "b", "nightly"),
        ];
        assert!(should_show_prefix(&entries));
    }

    #[test]
    fn test_should_show_prefix_empty_is_false() {
        assert!(!should_show_prefix(&[]));
    }

    #[test]
    fn test_clamp_tail_caps_at_server_max() {
        assert_eq!(clamp_tail(MAX_LOG_LINES + 1), MAX_LOG_LINES);
        assert_eq!(clamp_tail(9999), MAX_LOG_LINES);
        assert_eq!(clamp_tail(MAX_LOG_LINES), MAX_LOG_LINES);
        assert_eq!(clamp_tail(100), 100);
        assert_eq!(clamp_tail(1), 1);
    }

    fn make_request_entry(ts: &str) -> RequestLogEntry {
        RequestLogEntry {
            timestamp: ts.to_string(),
            method: "GET".to_string(),
            path: Some("/".to_string()),
            host: Some("my-app.on.getfloo.com".to_string()),
            status_code: 200,
            latency_ms: 12,
            access_mode: "public".to_string(),
            user_identity: None,
        }
    }

    #[test]
    fn test_update_last_request_timestamp_advances_on_newer_entry() {
        let mut last: Option<String> = None;
        update_last_request_timestamp(&mut last, &make_request_entry("2026-04-30T15:42:21Z"));
        assert_eq!(last.as_deref(), Some("2026-04-30T15:42:21Z"));

        update_last_request_timestamp(&mut last, &make_request_entry("2026-04-30T15:42:22Z"));
        assert_eq!(last.as_deref(), Some("2026-04-30T15:42:22Z"));
    }

    #[test]
    fn test_update_last_request_timestamp_keeps_newer_when_older_arrives() {
        // Live polling rides on the API's timestamp >= since semantics, so
        // an older row can show up alongside the new ones; the anchor must
        // not move backwards or we'd re-emit the same line forever.
        let mut last: Option<String> = Some("2026-04-30T15:42:22Z".to_string());
        update_last_request_timestamp(&mut last, &make_request_entry("2026-04-30T15:42:21Z"));
        assert_eq!(last.as_deref(), Some("2026-04-30T15:42:22Z"));
    }
}
