use std::process;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use colored::Colorize;

use crate::api_client::FlooClient;
use crate::api_types::{CoalescedCommit, Deploy};
use crate::commands::deploy::{
    poll_deploy, render_diagnostics, settle_to_terminal, stream_deploy, stream_deploy_json,
};
use crate::deploy_status;
use crate::errors::ErrorCode;
use crate::output;

fn deploy_list_payload(result: &crate::api_types::ListDeploysResponse) -> serde_json::Value {
    let deploys: Vec<serde_json::Value> = result
        .deploys
        .iter()
        .map(|d| {
            serde_json::json!({
                "id": &d.id,
                "status": &d.status,
                "url": &d.url,
                "created_at": &d.created_at,
                "started_at": &d.started_at,
                "finished_at": &d.finished_at,
                "duration_ms": &d.duration_ms,
                "failure_reason": failure_reason(d),
                "failing_stage": failure_stage(d),
                "triggered_by": &d.triggered_by,
                "commit_sha": &d.commit_sha,
                "env": &d.env,
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
            let env = d.env.as_deref().unwrap_or("\u{2014}");
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
    let env = deploy.env.as_deref().unwrap_or("\u{2014}");
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
                        Some("Deploy the app first: floo deploy"),
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
    let is_terminal = deploy_status::is_terminal(status);

    // JSON mode
    if output::is_json_mode() {
        if follow && !is_terminal {
            // Stream NDJSON for active deploys
            match stream_deploy_json(&client, &app_id, &deploy.id) {
                Ok(d) => {
                    let final_status = d.status.as_deref().unwrap_or("unknown");
                    if deploy_status::is_failure(final_status) {
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
                    let is_failed =
                        deploy_status::is_failure(final_deploy.status.as_deref().unwrap_or(""));
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
        let is_failed = deploy_status::is_failure(deploy.status.as_deref().unwrap_or(""));
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
                    let final_deploy = settle_to_terminal(&client, &app_id, final_deploy);
                    if deploy_status::is_failure(final_deploy.status.as_deref().unwrap_or("")) {
                        process::exit(1);
                    }
                }
                Err(e) => {
                    let base_msg = if deploy_status::is_failure(status) {
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
            Ok(d) => settle_to_terminal(&client, &app_id, d),
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
        // superseded (a newer deploy replaced this) and cancelled (target env torn
        // down before the deploy ran) are both moot terminals — dimmed, not error.
        "superseded" | "cancelled" => status.dimmed().to_string(),
        "building" | "deploying" | "pending" => status.yellow().to_string(),
        _ => status.to_string(),
    }
}

// --- deploy watch ---

/// How long to wait for a pushed commit's deploy to appear once nothing else is
/// deploying. A push that lands while another deploy runs (on a new floo-hosted
/// project, the starter's first deploy) is queued, and the API starts it within a
/// minute of that deploy finishing, so time spent behind a running deploy does not
/// count against this.
const COMMIT_WAIT_TIMEOUT: Duration = Duration::from_secs(120);
/// Upper bound on the whole wait, however long earlier deploys keep running.
const COMMIT_WAIT_MAX: Duration = Duration::from_secs(20 * 60);
const COMMIT_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// True when a pushed commit has waited too long for its deploy to appear.
/// `idle` is the time since a deploy that holds the commit back was last seen.
fn commit_wait_expired(waited: Duration, idle: Duration) -> bool {
    idle >= COMMIT_WAIT_TIMEOUT || waited >= COMMIT_WAIT_MAX
}

/// A running dev deploy holds a newer push back until it finishes.
fn holds_back_push(deploy: &Deploy) -> bool {
    let running = deploy
        .status
        .as_deref()
        .is_some_and(|status| !deploy_status::is_terminal(status));
    running && deploy.env.as_deref() != Some("prod") && deploy.preview_slug.is_none()
}

pub fn watch(app: Option<&str>, commit: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let matched = match commit {
        Some(sha) => find_deploy_by_commit(&client, &app_id, sha),
        None => find_latest_deploy(&client, &app_id).map(|deploy| (deploy, None)),
    };

    let (deploy, coalesced_commit) = match matched {
        Some(found) => found,
        None => {
            output::error(
                "No deploy found.",
                &ErrorCode::DeployNotFound,
                Some("Deploy the app first: floo deploy"),
            );
            process::exit(1);
        }
    };

    if let Some(lineage) = coalesced_commit.as_ref() {
        emit_commit_coalesced(lineage, &deploy);
    }
    emit_deploy_found(&deploy, &app_name);

    let status = deploy.status.as_deref().unwrap_or("");
    if deploy_status::is_terminal(status) {
        print_completed_deploy(&deploy);
        print_final_status(&deploy);
        return;
    }

    // Stream the active deploy
    let deploy_id = deploy.id.clone();
    let final_deploy = if !output::is_json_mode() {
        match stream_deploy(&client, &app_id, &deploy_id) {
            // settle_to_terminal owns the stream-ends-non-terminal race for
            // EVERY stream consumer now — this inline fallback moved there.
            Ok(d) => settle_to_terminal(&client, &app_id, d),
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
                if deploy_status::is_failure(status) {
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

fn find_deploy_by_commit(
    client: &FlooClient,
    app_id: &str,
    sha_prefix: &str,
) -> Option<(Deploy, Option<CoalescedCommit>)> {
    let start = Instant::now();
    let waiting_message = format!("Waiting for deploy with commit {sha_prefix}...");
    let mut spinner = if !output::is_json_mode() {
        Some(output::Spinner::new(&waiting_message))
    } else {
        None
    };
    let mut last_held_back: Option<Instant> = None;
    let mut announced_blocker: Option<String> = None;

    loop {
        let mut cursor: Option<String> = None;
        let mut coalesced_match: Option<(Deploy, CoalescedCommit)> = None;
        let mut blocker: Option<Deploy> = None;

        loop {
            let result = match client.list_deploys_paginated(app_id, 100, cursor.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    if let Some(s) = spinner.as_ref() {
                        s.finish();
                    }
                    output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                    process::exit(1);
                }
            };

            for deploy in result.deploys {
                if blocker.is_none() && holds_back_push(&deploy) {
                    blocker = Some(deploy.clone());
                }
                if deploy
                    .commit_sha
                    .as_deref()
                    .is_some_and(|sha| sha.starts_with(sha_prefix))
                {
                    if let Some(s) = spinner.as_ref() {
                        s.finish();
                    }
                    return Some((deploy, None));
                }

                if coalesced_match.is_none() {
                    if let Some(lineage) = deploy
                        .coalesced_commits
                        .iter()
                        .find(|lineage| lineage.requested_commit_sha.starts_with(sha_prefix))
                    {
                        coalesced_match = Some((deploy.clone(), lineage.clone()));
                    }
                }
            }

            if !result.has_more {
                break;
            }
            let Some(next_cursor) = result.next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }

        if let Some((deploy, lineage)) = coalesced_match {
            if let Some(s) = spinner.as_ref() {
                s.finish();
            }
            return Some((deploy, Some(lineage)));
        }

        if let Some(running) = blocker {
            last_held_back = Some(Instant::now());
            if announced_blocker.as_deref() != Some(running.id.as_str()) {
                if let Some(s) = spinner.take() {
                    s.finish();
                }
                announce_held_back(&running, sha_prefix);
                spinner = (!output::is_json_mode()).then(|| output::Spinner::new(&waiting_message));
                announced_blocker = Some(running.id);
            }
        }

        let idle = last_held_back.map_or_else(|| start.elapsed(), |seen| seen.elapsed());
        if commit_wait_expired(start.elapsed(), idle) {
            if let Some(s) = spinner.as_ref() {
                s.finish();
            }
            output::error(
                &format!(
                    "Timed out waiting for deploy with commit {sha_prefix} (waited {}s).",
                    start.elapsed().as_secs()
                ),
                &ErrorCode::DeployTimeout,
                Some(&format!(
                    "The push may still deploy. Run `floo deploys watch --commit {sha_prefix}` to keep waiting, or `floo status --commit {sha_prefix}` to see why it has not deployed."
                )),
            );
            process::exit(1);
        }

        thread::sleep(COMMIT_POLL_INTERVAL);
    }
}

fn announce_held_back(running: &Deploy, sha_prefix: &str) {
    let running_commit = running
        .commit_sha
        .as_deref()
        .map(super::short_sha)
        .unwrap_or("\u{2014}");
    if output::is_json_mode() {
        output::print_json(&serde_json::json!({
            "event": "queued_behind_deploy",
            "deploy_id": running.id,
            "deploy_commit": running_commit,
            "requested_commit": sha_prefix,
        }));
    } else {
        output::info(
            &format!(
                "Deploy {} ({running_commit}) is still running. Commit {} deploys as soon as it finishes.",
                running.id,
                super::short_sha(sha_prefix),
            ),
            None,
        );
    }
}

fn emit_commit_coalesced(lineage: &CoalescedCommit, deploy: &Deploy) {
    let effective_commit = deploy.commit_sha.as_deref().unwrap_or("unknown");
    if output::is_json_mode() {
        output::print_json(&serde_json::json!({
            "event": "commit_coalesced",
            "lineage_status": "superseded",
            "requested_commit": lineage.requested_commit_sha,
            "effective_commit": effective_commit,
            "effective_deploy_id": deploy.id,
        }));
    } else {
        output::info(
            &format!(
                "Commit {} was coalesced into descendant {} — following deploy {}.",
                super::short_sha(&lineage.requested_commit_sha),
                super::short_sha(effective_commit),
                deploy.id,
            ),
            None,
        );
    }
}

fn emit_deploy_found(deploy: &Deploy, app_name: &str) {
    let deploy_id = &deploy.id;
    let status = deploy.status.as_deref().unwrap_or("unknown");
    let env = deploy.env.as_deref().unwrap_or("unknown");
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
            "env": env,
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
    render_diagnostics(&deploy.diagnostics);
    let status = deploy.status.as_deref().unwrap_or("unknown");
    let url = deploy.url.as_deref().unwrap_or("");

    if output::is_json_mode() {
        output::print_json(&serde_json::json!({
            "event": "done",
            "diagnostics": deploy.diagnostics,
            "status": status,
            "url": url,
        }));
        if deploy_status::is_failure(status) {
            process::exit(1);
        }
        return;
    }

    // Human mode: report the terminal disposition. Matching on `classify` makes a
    // newly-added terminal status a compile error here instead of silently falling
    // through to the generic-success arm.
    match deploy_status::classify(status) {
        Some(deploy_status::Terminal::Failed) => {
            output::error(
                "Deploy failed.",
                &ErrorCode::DeployFailed,
                Some("Check build output above, or run `floo logs` for details."),
            );
            process::exit(1);
        }
        Some(deploy_status::Terminal::Superseded) => {
            output::success(
                "Deploy superseded by a newer deploy.",
                Some(serde_json::json!({ "deploy": output::to_value(deploy) })),
            );
        }
        Some(deploy_status::Terminal::Cancelled) => {
            output::success(
                "Deploy cancelled: its target environment was removed before it ran.",
                Some(serde_json::json!({ "deploy": output::to_value(deploy) })),
            );
        }
        Some(deploy_status::Terminal::Live) => {
            output::success(
                &format!("Deploy {status}: {url}"),
                Some(serde_json::json!({ "deploy": output::to_value(deploy) })),
            );
        }
        None => {
            output::error(
                &format!("Deploy ended in unexpected state: {status}"),
                &ErrorCode::DeployFailed,
                Some("Check deploy status with `floo deploys list --app <name>`."),
            );
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deploy_with(status: &str, env: Option<&str>, preview_slug: Option<&str>) -> Deploy {
        serde_json::from_value(serde_json::json!({
            "id": "deploy-1",
            "status": status,
            "url": null,
            "build_logs": null,
            "created_at": null,
            "triggered_by": null,
            "commit_sha": "abc1234",
            "env": env,
            "preview_slug": preview_slug,
        }))
        .unwrap()
    }

    #[test]
    fn commit_wait_counts_only_time_nothing_holds_the_push_back() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        let minute = Duration::from_secs(60);
        // Nothing running: the old 120 s limit.
        assert!(!commit_wait_expired(minute, minute));
        assert!(commit_wait_expired(2 * minute, 2 * minute));
        // A starter deploy ran for 4 minutes and finished 30 s ago: keep waiting.
        assert!(!commit_wait_expired(4 * minute + minute / 2, minute / 2));
        // Two idle minutes after it finished: give up.
        assert!(commit_wait_expired(6 * minute, 2 * minute));
        // Never past the hard cap, even behind a deploy that keeps running.
        assert!(commit_wait_expired(COMMIT_WAIT_MAX, Duration::ZERO));
    }

    #[test]
    fn only_running_dev_deploys_hold_a_push_back() {
        output::set_json_mode(false);
        output::set_dry_run_mode(false);
        assert!(holds_back_push(&deploy_with("building", Some("dev"), None)));
        assert!(holds_back_push(&deploy_with("pending", None, None)));
        assert!(!holds_back_push(&deploy_with("live", Some("dev"), None)));
        assert!(!holds_back_push(&deploy_with(
            "deploying",
            Some("prod"),
            None
        )));
        assert!(!holds_back_push(&deploy_with(
            "building",
            Some("preview"),
            Some("feat-x")
        )));
    }

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
