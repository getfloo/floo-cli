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

/// Compact, agent-safe deploy status summary.
///
/// Composed entirely from existing API endpoints (`get_app` + the
/// latest entry in `list_deploys` / `get_deploy`). Emits derived phase
/// booleans the user can branch on without parsing build logs:
///
/// - `image_built` — the deploy moved past the build phase
/// - `service_ready` — Cloud Run is serving the new revision
/// - `host_bound` — the gateway URL is wired and the deploy is LIVE
///
/// Always emits the same compact shape; never includes build logs or
/// other audit payload that could carry secret env values. Closes
/// feedback `5c7621fd` (floo-artifact, 2026-04-30): "add a safe
/// status/debug command that summarizes deploy state without dumping
/// build logs, Cloud Run audit payloads, or secret env values."
pub fn status(app: Option<&str>, deploy_id: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let app_info = match client.get_app(&app_id) {
        Ok(a) => a,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let deploy = match deploy_id {
        Some(id) => {
            let full_id = resolve_deploy_id(&client, &app_id, id);
            match client.get_deploy(&app_id, &full_id) {
                Ok(d) => d,
                Err(e) => {
                    output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                    process::exit(1);
                }
            }
        }
        None => match client.list_deploys(&app_id) {
            Ok(list) => match list.deploys.into_iter().next() {
                Some(d) => d,
                None => {
                    output::error(
                        "No deploys found for this app yet.",
                        &ErrorCode::DeployNotFound,
                        Some("Run `floo deploy .` or push to the connected GitHub branch."),
                    );
                    process::exit(1);
                }
            },
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        },
    };

    let deploy_status = deploy.status.as_deref().unwrap_or("unknown");
    let app_status = app_info.status.as_deref().unwrap_or("unknown");
    let gateway_url = app_info.url.as_deref().or(deploy.url.as_deref());
    let commit_short = deploy.commit_sha.as_deref().map(super::short_sha);

    let image_built = matches!(
        deploy_status,
        "deploying" | "configuring_routing" | "live" | "superseded"
    );
    let service_ready = matches!(deploy_status, "live");
    // host_bound is the strict signal: the gateway URL is wired AND the
    // deploy is the one currently serving. A deploy that "looks live"
    // because the direct Cloud Run URL serves new code but the gateway
    // 502s is exactly the floo-artifact 2026-04-30 fed461a6 shape — so
    // host_bound=false on that state is the right answer for an agent.
    let host_bound = service_ready && gateway_url.is_some() && app_status == "live";

    let next_command = derive_next_command(deploy_status, host_bound);

    let summary = serde_json::json!({
        "app": app_name,
        "deploy_id": deploy.id,
        "commit": commit_short,
        "status": deploy_status,
        "app_status": app_status,
        "status_semantics": {
            "status": "deploy_attempt",
            "app_status": "currently_serving_app",
        },
        "url": gateway_url,
        "image_built": image_built,
        "service_ready": service_ready,
        "host_bound": host_bound,
        "failure_reason": failure_reason(&deploy),
        "failing_stage": failure_stage(&deploy),
        "created_at": deploy.created_at,
        "started_at": deploy.started_at,
        "finished_at": deploy.finished_at,
        "duration_ms": deploy.duration_ms,
        "next_command": next_command,
    });

    if output::is_json_mode() {
        output::success("Deploy status retrieved.", Some(summary));
        return;
    }

    let header = format!("{} ({})", app_name, deploy.id);
    output::bold_line(&header);
    output::dim_line(&format!(
        "  status:        {} (deploy attempt)",
        colored_status(deploy_status)
    ));
    output::dim_line(&format!("  app_status:    {} (current app)", app_status));
    output::dim_line(&format!(
        "  commit:        {}",
        commit_short.unwrap_or("\u{2014}")
    ));
    output::dim_line(&format!(
        "  url:           {}",
        gateway_url.unwrap_or("\u{2014}")
    ));
    output::dim_line(&format!("  image_built:   {}", image_built));
    output::dim_line(&format!("  service_ready: {}", service_ready));
    output::dim_line(&format!("  host_bound:    {}", host_bound));
    output::dim_line(&format!(
        "  duration:      {}",
        format_duration_ms(deploy.duration_ms)
    ));
    if let Some(reason) = failure_reason(&deploy) {
        let stage = failure_stage(&deploy).unwrap_or("unknown");
        output::dim_line(&format!("  failure:       {stage}: {reason}"));
    }
    output::dim_line(&format!("  next_command:  {}", next_command));
}

/// Suggest the next command an agent or operator should run, based on
/// the current state. Agent-safe: never names a destructive command
/// without explicit caller confirmation.
fn derive_next_command(deploy_status: &str, host_bound: bool) -> &'static str {
    match deploy_status {
        "pending" | "scheduled" | "building" | "deploying" | "configuring_routing" => {
            "floo deploys watch"
        }
        "failed" => "floo deploys logs",
        "live" if host_bound => "floo logs",
        "live" => "floo apps status",
        _ => "floo apps status",
    }
}

fn deploy_list_payload(result: &crate::api_types::ListDeploysResponse) -> serde_json::Value {
    let deploys: Vec<serde_json::Value> = result
        .deploys
        .iter()
        .map(|d| {
            serde_json::json!({
                "id": &d.id,
                "status": &d.status,
                "url": &d.url,
                "runtime": &d.runtime,
                "created_at": &d.created_at,
                "started_at": &d.started_at,
                "finished_at": &d.finished_at,
                "duration_ms": &d.duration_ms,
                "failure_reason": failure_reason(d),
                "failing_stage": failure_stage(d),
                "triggered_by": &d.triggered_by,
                "commit_sha": &d.commit_sha,
                "environment_name": &d.environment_name,
            })
        })
        .collect();
    serde_json::json!({
        "deploys": deploys,
        "total": result.total,
        "page": result.page,
        "per_page": result.per_page,
        "limit": result.limit,
        "next_cursor": result.next_cursor,
        "has_more": result.has_more,
    })
}

pub fn list(app: Option<&str>, limit: u32, cursor: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, _app_name) = super::resolve_app_from_config(&client, app);

    let result = match client.list_deploys_paginated(&app_id, limit, cursor) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if result.deploys.is_empty() {
        if output::is_json_mode() {
            output::success("No deploys found.", Some(deploy_list_payload(&result)));
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
            let env = d.environment_name.as_deref().unwrap_or("\u{2014}");
            let created = d.created_at.as_deref().unwrap_or("-");
            let duration = format_duration_ms(d.duration_ms);
            let failure = failure_reason(d)
                .map(|reason| {
                    let stage = failure_stage(d).unwrap_or("unknown");
                    truncate_text(&format!("{stage}: {reason}"), 64)
                })
                .unwrap_or_else(|| "\u{2014}".to_string());
            vec![
                short_id,
                colored_status(status),
                env.to_string(),
                d.triggered_by.as_deref().unwrap_or("\u{2014}").to_string(),
                commit.to_string(),
                duration,
                relative_time(created),
                failure,
            ]
        })
        .collect();

    output::table(
        &[
            "Deploy ID",
            "Status",
            "Env",
            "Triggered By",
            "Commit",
            "Duration",
            "Created",
            "Failure",
        ],
        &rows,
        Some(deploy_list_payload(&result)),
    );
    if let Some(next_cursor) = result.next_cursor.as_deref() {
        let app_arg = app.map(|name| format!(" --app {name}")).unwrap_or_default();
        output::dim_line(&format!(
            "More deploys available. Next: floo deploys list{app_arg} --limit {} --cursor {next_cursor}",
            result.limit.unwrap_or(limit)
        ));
    }
}

/// Check if a log string is meaningful (not empty or placeholder text).
fn is_real_log(logs: &str) -> bool {
    let trimmed = logs.trim();
    !trimmed.is_empty() && trimmed != "[no message content]"
}

fn failure_stage(deploy: &Deploy) -> Option<&str> {
    deploy
        .failing_stage
        .as_deref()
        .or(deploy.failure_stage.as_deref())
        .or(deploy
            .failure_root_cause
            .as_ref()
            .and_then(|cause| cause.stage.as_deref()))
        .or(deploy.failure_step.as_deref())
}

fn failure_reason(deploy: &Deploy) -> Option<&str> {
    deploy
        .failure_reason
        .as_deref()
        .or(deploy
            .failure_root_cause
            .as_ref()
            .and_then(|cause| cause.reason.as_deref()))
        .or(deploy.failure_message.as_deref())
}

fn format_duration_ms(duration_ms: Option<i64>) -> String {
    let Some(duration_ms) = duration_ms else {
        return "\u{2014}".to_string();
    };
    if duration_ms < 1000 {
        return format!("{duration_ms}ms");
    }
    let total_seconds = duration_ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('\u{2026}');
    truncated
}

/// Print deploy metadata header to stderr.
fn print_deploy_header(deploy: &Deploy) {
    let id = &deploy.id;
    let status = deploy.status.as_deref().unwrap_or("unknown");
    let env = deploy.environment_name.as_deref().unwrap_or("\u{2014}");
    let commit = deploy
        .commit_sha
        .as_deref()
        .map(super::short_sha)
        .unwrap_or("\u{2014}");
    let triggered_by = deploy.triggered_by.as_deref().unwrap_or("\u{2014}");
    let created = deploy.created_at.as_deref().unwrap_or("\u{2014}");

    output::bold_line(&format!("Deploy {id}"));
    output::dim_line(&format!("  Status:  {}", colored_status(status)));
    output::dim_line(&format!("  Env:     {env}"));
    output::dim_line(&format!("  Commit:  {commit}"));
    output::dim_line(&format!("  By:      {triggered_by}"));
    output::dim_line(&format!("  Created: {created}"));
    output::dim_line(&format!(
        "  Duration: {}",
        format_duration_ms(deploy.duration_ms)
    ));
    if let Some(reason) = failure_reason(deploy) {
        let stage = failure_stage(deploy).unwrap_or("unknown");
        output::dim_line(&format!("  Failure: {stage}: {reason}"));
    }
    output::dim_line("");
}

pub fn logs(deploy_id: Option<&str>, app: Option<&str>, follow: bool) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, _app_name) = super::resolve_app_from_config(&client, app);

    let resolved_deploy_id: String = match deploy_id {
        Some(id) => resolve_deploy_id(&client, &app_id, id),
        None => {
            let result = match client.list_deploys(&app_id) {
                Ok(r) => r,
                Err(e) => {
                    output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                    process::exit(1);
                }
            };
            match result.deploys.into_iter().next() {
                Some(d) => d.id,
                None => {
                    output::error(
                        "No deploys found for this app.",
                        &ErrorCode::DeployNotFound,
                        Some("Connect a repo first: floo apps github connect <owner/repo>"),
                    );
                    process::exit(1);
                }
            }
        }
    };

    let deploy = match client.get_deploy(&app_id, &resolved_deploy_id) {
        Ok(d) => d,
        Err(e) => {
            let suggestion = match e.code.as_str() {
                "DEPLOY_NOT_FOUND" => Some("Check the deploy ID: floo deploys list --app <name>"),
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

/// Returns true when `id` is a full UUID (32 hex chars or 36 with dashes).
/// We want to skip the prefix-resolution round trip in that case (the API
/// validates the shape). Anything shorter is treated as a prefix and
/// matched against the deploy list.
fn is_full_uuid(id: &str) -> bool {
    let len = id.len();
    if len != 32 && len != 36 {
        return false;
    }
    id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn strip_truncation_marker(id: &str) -> &str {
    id.strip_suffix("...").unwrap_or(id)
}

/// True for hex-shaped strings (0-9a-f, optional dashes), i.e. shapes that
/// could be a UUID prefix. We only attempt prefix-resolution against the
/// deploy list for these. Anything outside this character set bypasses the
/// resolver and is passed straight to the API, which keeps the door open for
/// future non-UUID identifiers and means fixture IDs like `deploy-456` in
/// tests do not trigger an extra mock requirement.
fn looks_like_uuid_prefix(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Resolve a partial deploy id (git-style prefix) to a full deploy id by
/// listing the app's deploys and matching by prefix. A full UUID is
/// returned unchanged (cheap, avoids the list call). Errors and exits the
/// process with a friendly message when the prefix matches zero or
/// multiple deploys, replacing the raw FastAPI validation array that
/// users were seeing when they copy-pasted the truncated id from the
/// table view.
pub(crate) fn resolve_deploy_id(client: &FlooClient, app_id: &str, partial: &str) -> String {
    let partial = strip_truncation_marker(partial);

    if partial.is_empty() {
        output::error(
            "Empty deploy ID.",
            &ErrorCode::DeployNotFound,
            Some("Pass a deploy ID. Run `floo deploys list --app <name>` to see them."),
        );
        process::exit(1);
    }

    if is_full_uuid(partial) {
        return partial.to_string();
    }

    // Non-UUID-shaped strings (e.g. test fixture IDs) bypass prefix
    // resolution; we pass them through and let the API decide.
    if !looks_like_uuid_prefix(partial) {
        return partial.to_string();
    }

    let deploys = match client.list_deploys(app_id) {
        Ok(r) => r.deploys,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let matches: Vec<&Deploy> = deploys
        .iter()
        .filter(|d| d.id.starts_with(partial))
        .collect();

    match matches.as_slice() {
        [single] => single.id.clone(),
        [] => {
            output::error(
                &format!("No deploy found matching '{partial}'."),
                &ErrorCode::DeployNotFound,
                Some("Run `floo deploys list --app <name>` to see full deploy IDs."),
            );
            process::exit(1);
        }
        many => {
            let preview: Vec<&str> = many.iter().take(3).map(|d| d.id.as_str()).collect();
            output::error(
                &format!(
                    "Deploy ID '{partial}' is ambiguous ({} matches: {}{}).",
                    many.len(),
                    preview.join(", "),
                    if many.len() > 3 { ", ..." } else { "" },
                ),
                &ErrorCode::DeployNotFound,
                Some("Use more characters of the deploy ID, or run `floo deploys list --app <name>`."),
            );
            process::exit(1);
        }
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
        "superseded" => status.dimmed().to_string(),
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
                Some("Connect a repo first: floo apps github connect <owner/repo>"),
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
        let streamed = match stream_deploy(&client, &app_id, &deploy_id) {
            Ok(d) => d,
            Err(e) => {
                eprintln!(
                    "Stream unavailable ({}), falling back to polling...",
                    e.code
                );
                poll_deploy(&client, &app_id, &deploy)
            }
        };
        // SSE stream may end before the deploy reaches a terminal state (e.g. connection
        // drop, server restart). If so, fall back to polling to wait for completion.
        let streamed_status = streamed.status.as_deref().unwrap_or("");
        if !TERMINAL_STATUSES.contains(&streamed_status) {
            eprintln!("Stream ended in non-terminal state ({streamed_status}), falling back to polling...");
            poll_deploy(&client, &app_id, &streamed)
        } else {
            streamed
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
                Some("The deploy may still be processing. Check `floo deploys list --app <name>` or try again."),
            );
            process::exit(1);
        }

        thread::sleep(COMMIT_POLL_INTERVAL);
    }
}

fn emit_deploy_found(deploy: &Deploy, app_name: &str) {
    let deploy_id = &deploy.id;
    let status = deploy.status.as_deref().unwrap_or("unknown");
    let env = deploy.environment_name.as_deref().unwrap_or("unknown");
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
            "environment": env,
            "commit": commit,
        }));
    } else {
        output::info(
            &format!("Watching deploy {deploy_id} ({commit}) \u{2014} {status} ({env})"),
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
    } else if status == "superseded" {
        output::success(
            "Deploy superseded by a newer deploy.",
            Some(serde_json::json!({
                "deploy": output::to_value(deploy),
            })),
        );
    } else if !TERMINAL_STATUSES.contains(&status) {
        output::error(
            &format!("Deploy ended in unexpected state: {status}"),
            &ErrorCode::DeployFailed,
            Some("Check deploy status with `floo deploys list --app <name>`."),
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
        assert!(!colored_status("superseded").is_empty());
        assert!(!colored_status("building").is_empty());
        assert!(!colored_status("deploying").is_empty());
        assert!(!colored_status("pending").is_empty());
        assert!(!colored_status("unknown").is_empty());
    }

    #[test]
    fn test_superseded_is_terminal_status() {
        // TERMINAL_STATUSES must include "superseded" so poll/stream loops exit
        // when the platform marks a deploy as superseded. Before this was added,
        // `floo deploys watch` would spin for POLL_TIMEOUT (10 min) on superseded
        // deploys. See feedback 1748af72 (2026-04-24).
        assert!(TERMINAL_STATUSES.contains(&"superseded"));
        assert!(TERMINAL_STATUSES.contains(&"live"));
        assert!(TERMINAL_STATUSES.contains(&"failed"));
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

    #[test]
    fn test_is_full_uuid_dashed() {
        assert!(is_full_uuid("fed461a6-7c12-4abc-89de-0123456789ab"));
    }

    #[test]
    fn test_is_full_uuid_undashed() {
        assert!(is_full_uuid("fed461a67c124abc89de0123456789ab"));
    }

    #[test]
    fn test_is_full_uuid_rejects_prefix() {
        assert!(!is_full_uuid("fed461a6"));
    }

    #[test]
    fn test_is_full_uuid_rejects_with_truncation_marker() {
        assert!(!is_full_uuid("fed461a6..."));
    }

    #[test]
    fn test_is_full_uuid_rejects_non_hex() {
        assert!(!is_full_uuid("zed461a6-7c12-4abc-89de-0123456789ab"));
    }

    #[test]
    fn test_strip_truncation_marker_present() {
        assert_eq!(strip_truncation_marker("fed461a6..."), "fed461a6");
    }

    #[test]
    fn test_strip_truncation_marker_absent() {
        assert_eq!(strip_truncation_marker("fed461a6"), "fed461a6");
    }

    #[test]
    fn test_looks_like_uuid_prefix_hex() {
        assert!(looks_like_uuid_prefix("fed461a6"));
    }

    #[test]
    fn test_looks_like_uuid_prefix_with_dash() {
        assert!(looks_like_uuid_prefix("fed461a6-7c12"));
    }

    #[test]
    fn test_looks_like_uuid_prefix_rejects_letters_outside_hex() {
        // 'p', 'l', 'o', 'y' are not hex digits, so this is not a UUID prefix
        assert!(!looks_like_uuid_prefix("deploy-456"));
    }

    #[test]
    fn test_looks_like_uuid_prefix_rejects_empty() {
        assert!(!looks_like_uuid_prefix(""));
    }
}
