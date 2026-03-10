use std::process;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use colored::Colorize;

use crate::api_client::FlooClient;
use crate::api_types::Deploy;
use crate::commands::deploy::{poll_deploy, stream_deploy, stream_deploy_json, TERMINAL_STATUSES};
use crate::errors::ErrorCode;
use crate::output;

pub fn list(app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, _app_name) = super::resolve_app_from_config(&client, app);

    let result = match client.list_deploys(&app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if result.deploys.is_empty() {
        if output::is_json_mode() {
            output::success(
                "No deploys found.",
                Some(serde_json::json!({"deploys": []})),
            );
        } else {
            output::info("No deploys found.", None);
        }
        return;
    }

    let rows: Vec<Vec<String>> = result
        .deploys
        .iter()
        .map(|d| {
            let commit = d
                .commit_sha
                .as_deref()
                .map(super::short_sha)
                .unwrap_or("\u{2014}");
            let short_id = truncate_id(&d.id);
            let status = d.status.as_deref().unwrap_or("-");
            let created = d.created_at.as_deref().unwrap_or("-");
            vec![
                short_id,
                colored_status(status),
                d.triggered_by.as_deref().unwrap_or("\u{2014}").to_string(),
                commit.to_string(),
                relative_time(created),
            ]
        })
        .collect();

    output::table(
        &["Deploy ID", "Status", "Triggered By", "Commit", "Created"],
        &rows,
        Some(output::to_value(&result)),
    );
}

/// Check if a log string is meaningful (not empty or placeholder text).
fn is_real_log(logs: &str) -> bool {
    let trimmed = logs.trim();
    !trimmed.is_empty() && trimmed != "[no message content]"
}

/// Print deploy metadata header to stderr.
fn print_deploy_header(deploy: &Deploy) {
    let id = &deploy.id;
    let status = deploy.status.as_deref().unwrap_or("unknown");
    let commit = deploy
        .commit_sha
        .as_deref()
        .map(super::short_sha)
        .unwrap_or("\u{2014}");
    let triggered_by = deploy.triggered_by.as_deref().unwrap_or("\u{2014}");
    let created = deploy.created_at.as_deref().unwrap_or("\u{2014}");

    output::bold_line(&format!("Deploy {id}"));
    output::dim_line(&format!("  Status:  {}", colored_status(status)));
    output::dim_line(&format!("  Commit:  {commit}"));
    output::dim_line(&format!("  By:      {triggered_by}"));
    output::dim_line(&format!("  Created: {created}"));
    output::dim_line("");
}

pub fn logs(deploy_id: &str, app: Option<&str>, follow: bool) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, _app_name) = super::resolve_app_from_config(&client, app);

    let deploy = match client.get_deploy(&app_id, deploy_id) {
        Ok(d) => d,
        Err(e) => {
            let suggestion = match e.code.as_str() {
                "DEPLOY_NOT_FOUND" => Some("Check the deploy ID: floo deploy list --app <name>"),
                _ => None,
            };
            output::error(&e.message, &ErrorCode::from_api(&e.code), suggestion);
            process::exit(1);
        }
    };

    let status = deploy.status.as_deref().unwrap_or("unknown");
    let is_terminal = TERMINAL_STATUSES.contains(&status);

    // JSON mode
    if output::is_json_mode() {
        if follow && !is_terminal {
            // Stream NDJSON for active deploys
            match stream_deploy_json(&client, &app_id, &deploy.id) {
                Ok(d) => {
                    let final_status = d.status.as_deref().unwrap_or("unknown");
                    if final_status == "failed" {
                        process::exit(1);
                    }
                    return;
                }
                Err(e) => {
                    // Fallback: poll and emit final state
                    eprintln!(
                        "Stream unavailable ({}), falling back to polling...",
                        e.code
                    );
                    let final_deploy = poll_deploy(&client, &app_id, &deploy);
                    let is_failed = final_deploy.status.as_deref() == Some("failed");
                    output::success(
                        "Deploy logs retrieved.",
                        Some(serde_json::json!({
                            "deploy_id": final_deploy.id,
                            "status": final_deploy.status,
                            "build_logs": final_deploy.build_logs,
                        })),
                    );
                    if is_failed {
                        process::exit(1);
                    }
                    return;
                }
            }
        }
        let is_failed = deploy.status.as_deref() == Some("failed");
        output::success(
            "Deploy logs retrieved.",
            Some(serde_json::json!({
                "deploy_id": deploy.id,
                "status": deploy.status,
                "build_logs": deploy.build_logs,
            })),
        );
        if is_failed {
            process::exit(1);
        }
        return;
    }

    // Human mode — show metadata header
    print_deploy_header(&deploy);

    if is_terminal {
        // Terminal status: show stored logs or try SSE fallback
        if let Some(logs) = deploy.build_logs.as_deref().filter(|l| is_real_log(l)) {
            for line in logs.lines() {
                output::dim_line(line);
            }
        } else {
            // Try SSE stream as fallback for missing logs
            match stream_deploy(&client, &app_id, &deploy.id) {
                Ok(final_deploy) => {
                    if final_deploy.status.as_deref() == Some("failed") {
                        process::exit(1);
                    }
                }
                Err(e) => {
                    let base_msg = if status == "failed" {
                        "No build logs captured for this deploy."
                    } else {
                        "No build logs available."
                    };
                    output::info(
                        &format!("{base_msg} (log stream failed: {})", e.message),
                        None,
                    );
                }
            }
        }
    } else if follow {
        // Active deploy + --follow: stream live
        let final_deploy = match stream_deploy(&client, &app_id, &deploy.id) {
            Ok(d) => d,
            Err(e) => {
                output::warn(&format!(
                    "Stream unavailable ({}), falling back to polling...",
                    e.code
                ));
                poll_deploy(&client, &app_id, &deploy)
            }
        };
        print_final_status(&final_deploy);
    } else {
        // Active deploy + no follow: show what we have and hint
        if let Some(logs) = deploy.build_logs.as_deref().filter(|l| is_real_log(l)) {
            for line in logs.lines() {
                output::dim_line(line);
            }
        }
        output::info(
            "Deploy is still in progress. Use --follow to stream live.",
            None,
        );
    }
}

// --- helpers ---

fn truncate_id(id: &str) -> String {
    if id.len() > 8 && id.is_ascii() {
        format!("{}...", &id[..8])
    } else {
        id.to_string()
    }
}

fn relative_time(iso_ts: &str) -> String {
    let parsed = chrono::DateTime::parse_from_rfc3339(iso_ts).or_else(|_| {
        // Handle timestamps without timezone (assume UTC)
        chrono::NaiveDateTime::parse_from_str(iso_ts, "%Y-%m-%dT%H:%M:%S%.f")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(iso_ts, "%Y-%m-%dT%H:%M:%S"))
            .map(|naive| naive.and_utc().fixed_offset())
    });

    let dt = match parsed {
        Ok(dt) => dt,
        Err(_) => return iso_ts.to_string(),
    };

    let now = Utc::now();
    let delta = now.signed_duration_since(dt);

    if delta.num_seconds() < 0 {
        return iso_ts.to_string();
    }

    let minutes = delta.num_minutes();
    if minutes < 1 {
        return "<1m ago".to_string();
    }
    if minutes < 60 {
        return format!("{minutes}m ago");
    }

    let hours = delta.num_hours();
    if hours < 24 {
        return format!("{hours}h ago");
    }

    let days = delta.num_days();
    format!("{days}d ago")
}

fn colored_status(status: &str) -> String {
    match status {
        "live" => status.green().bold().to_string(),
        "failed" => status.red().bold().to_string(),
        "building" | "deploying" | "pending" => status.yellow().to_string(),
        _ => status.to_string(),
    }
}

// --- deploy watch ---

const COMMIT_WAIT_TIMEOUT: Duration = Duration::from_secs(120);
const COMMIT_POLL_INTERVAL: Duration = Duration::from_secs(3);

pub fn watch(app: Option<&str>, commit: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let deploy = match commit {
        Some(sha) => find_deploy_by_commit(&client, &app_id, sha),
        None => find_latest_deploy(&client, &app_id),
    };

    let deploy = match deploy {
        Some(d) => d,
        None => {
            output::error(
                "No deploy found.",
                &ErrorCode::DeployNotFound,
                Some("Deploy to the app first: floo deploy"),
            );
            process::exit(1);
        }
    };

    emit_deploy_found(&deploy, &app_name);

    let status = deploy.status.as_deref().unwrap_or("");
    if TERMINAL_STATUSES.contains(&status) {
        print_completed_deploy(&deploy);
        print_final_status(&deploy);
        return;
    }

    // Stream the active deploy
    let deploy_id = deploy.id.clone();
    let final_deploy = if !output::is_json_mode() {
        match stream_deploy(&client, &app_id, &deploy_id) {
            Ok(d) => d,
            Err(e) => {
                eprintln!(
                    "Stream unavailable ({}), falling back to polling...",
                    e.code
                );
                poll_deploy(&client, &app_id, &deploy)
            }
        }
    } else {
        match stream_deploy_json(&client, &app_id, &deploy_id) {
            Ok(d) => {
                // stream_deploy_json already emitted the "done" NDJSON event
                let status = d.status.as_deref().unwrap_or("unknown");
                if status == "failed" {
                    process::exit(1);
                }
                return;
            }
            Err(_) => poll_deploy(&client, &app_id, &deploy),
        }
    };

    print_final_status(&final_deploy);
}

fn find_latest_deploy(client: &FlooClient, app_id: &str) -> Option<Deploy> {
    let result = match client.list_deploys(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };
    result.deploys.into_iter().next()
}

fn find_deploy_by_commit(client: &FlooClient, app_id: &str, sha_prefix: &str) -> Option<Deploy> {
    let start = Instant::now();
    let spinner = if !output::is_json_mode() {
        Some(output::Spinner::new(&format!(
            "Waiting for deploy with commit {sha_prefix}..."
        )))
    } else {
        None
    };

    loop {
        let result = match client.list_deploys(app_id) {
            Ok(r) => r,
            Err(e) => {
                if let Some(s) = spinner.as_ref() {
                    s.finish();
                }
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };

        if let Some(deploy) = result.deploys.into_iter().find(|d| {
            d.commit_sha
                .as_deref()
                .is_some_and(|s| s.starts_with(sha_prefix))
        }) {
            if let Some(s) = spinner.as_ref() {
                s.finish();
            }
            return Some(deploy);
        }

        if start.elapsed() >= COMMIT_WAIT_TIMEOUT {
            if let Some(s) = spinner.as_ref() {
                s.finish();
            }
            output::error(
                &format!(
                    "Timed out waiting for deploy with commit {sha_prefix} (waited {}s).",
                    COMMIT_WAIT_TIMEOUT.as_secs()
                ),
                &ErrorCode::DeployTimeout,
                Some("The deploy may still be processing. Check `floo deploy list --app <name>` or try again."),
            );
            process::exit(1);
        }

        thread::sleep(COMMIT_POLL_INTERVAL);
    }
}

fn emit_deploy_found(deploy: &Deploy, app_name: &str) {
    let deploy_id = &deploy.id;
    let status = deploy.status.as_deref().unwrap_or("unknown");
    let commit = deploy
        .commit_sha
        .as_deref()
        .map(super::short_sha)
        .unwrap_or("\u{2014}");

    if output::is_json_mode() {
        output::print_json(&serde_json::json!({
            "event": "deploy_found",
            "deploy_id": deploy_id,
            "app": app_name,
            "status": status,
            "commit": commit,
        }));
    } else {
        output::info(
            &format!("Watching deploy {deploy_id} ({commit}) \u{2014} {status}"),
            None,
        );
    }
}

fn print_completed_deploy(deploy: &Deploy) {
    if output::is_json_mode() {
        return;
    }
    if let Some(ref logs) = deploy.build_logs {
        if !logs.is_empty() {
            for line in logs.lines() {
                output::dim_line(line);
            }
        }
    }
}

fn print_final_status(deploy: &Deploy) {
    let status = deploy.status.as_deref().unwrap_or("unknown");
    let url = deploy.url.as_deref().unwrap_or("");

    if output::is_json_mode() {
        output::print_json(&serde_json::json!({
            "event": "done",
            "status": status,
            "url": url,
        }));
        if status == "failed" {
            process::exit(1);
        }
    } else if status == "failed" {
        output::error(
            "Deploy failed.",
            &ErrorCode::DeployFailed,
            Some("Check build output above, or run `floo logs` for details."),
        );
        process::exit(1);
    } else if !TERMINAL_STATUSES.contains(&status) {
        output::error(
            &format!("Deploy ended in unexpected state: {status}"),
            &ErrorCode::DeployFailed,
            Some("Check deploy status with `floo deploy list --app <name>`."),
        );
        process::exit(1);
    } else {
        output::success(
            &format!("Deploy {status}: {url}"),
            Some(serde_json::json!({
                "deploy": output::to_value(deploy),
            })),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_time_seconds_ago() {
        let now = Utc::now();
        let ts = now.to_rfc3339();
        assert_eq!(relative_time(&ts), "<1m ago");
    }

    #[test]
    fn test_relative_time_minutes_ago() {
        let now = Utc::now() - chrono::Duration::minutes(5);
        let ts = now.to_rfc3339();
        assert_eq!(relative_time(&ts), "5m ago");
    }

    #[test]
    fn test_relative_time_hours_ago() {
        let now = Utc::now() - chrono::Duration::hours(3);
        let ts = now.to_rfc3339();
        assert_eq!(relative_time(&ts), "3h ago");
    }

    #[test]
    fn test_relative_time_days_ago() {
        let now = Utc::now() - chrono::Duration::days(7);
        let ts = now.to_rfc3339();
        assert_eq!(relative_time(&ts), "7d ago");
    }

    #[test]
    fn test_relative_time_invalid_input() {
        assert_eq!(relative_time("not-a-date"), "not-a-date");
    }

    #[test]
    fn test_relative_time_naive_timestamp() {
        let now = Utc::now() - chrono::Duration::hours(2);
        let ts = now.format("%Y-%m-%dT%H:%M:%S").to_string();
        assert_eq!(relative_time(&ts), "2h ago");
    }

    #[test]
    fn test_colored_status_values() {
        // Verify colored_status returns non-empty strings for all known statuses
        assert!(!colored_status("live").is_empty());
        assert!(!colored_status("failed").is_empty());
        assert!(!colored_status("building").is_empty());
        assert!(!colored_status("deploying").is_empty());
        assert!(!colored_status("pending").is_empty());
        assert!(!colored_status("unknown").is_empty());
    }

    #[test]
    fn test_colored_status_unknown_passthrough() {
        // Unknown status should return the original string (no ANSI if colors disabled)
        let result = colored_status("custom-status");
        assert!(result.contains("custom-status"));
    }

    #[test]
    fn test_truncate_id_long() {
        assert_eq!(truncate_id("abcdefghijklmnop"), "abcdefgh...");
    }

    #[test]
    fn test_truncate_id_short() {
        assert_eq!(truncate_id("abcd"), "abcd");
    }

    #[test]
    fn test_truncate_id_exactly_eight() {
        assert_eq!(truncate_id("abcdefgh"), "abcdefgh");
    }

    #[test]
    fn test_truncate_id_non_ascii_not_sliced() {
        // Multi-byte chars: slicing at byte 8 could panic, so non-ASCII IDs are returned whole
        let id = "\u{1F600}\u{1F600}\u{1F600}"; // 12 bytes, 3 chars
        assert_eq!(truncate_id(id), id);
    }

    #[test]
    fn test_is_real_log_empty() {
        assert!(!is_real_log(""));
    }

    #[test]
    fn test_is_real_log_placeholder() {
        assert!(!is_real_log("[no message content]"));
    }

    #[test]
    fn test_is_real_log_real_content() {
        assert!(is_real_log("Step 1: Building..."));
    }

    #[test]
    fn test_is_real_log_placeholder_with_whitespace() {
        assert!(!is_real_log("  [no message content]  \n"));
    }

    #[test]
    fn test_is_real_log_whitespace_only() {
        assert!(!is_real_log("   \n  "));
    }

    #[test]
    fn test_relative_time_boundary_59_minutes() {
        let ts = (Utc::now() - chrono::Duration::minutes(59)).to_rfc3339();
        assert_eq!(relative_time(&ts), "59m ago");
    }

    #[test]
    fn test_relative_time_boundary_60_minutes() {
        let ts = (Utc::now() - chrono::Duration::minutes(60)).to_rfc3339();
        assert_eq!(relative_time(&ts), "1h ago");
    }
}
