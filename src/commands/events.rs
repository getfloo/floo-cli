use std::process;

use serde_json::Value;

use crate::api_client::FlooClient;
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;

pub fn list(app_flag: Option<&str>, status_filter: Option<&str>) {
    let result = call(app_flag, |client, app_id| {
        client.agent_events(app_id, status_filter)
    });

    if output::is_json_mode() {
        output::success("Agent events.", Some(result));
        return;
    }

    let events_arr = result.get("events").and_then(|v| v.as_array());
    let Some(events) = events_arr else {
        output::info("No agent events.", None);
        return;
    };

    if events.is_empty() {
        output::info("No agent events.", None);
        return;
    }

    let rows: Vec<Vec<String>> = events
        .iter()
        .map(|e| {
            vec![
                field(e, "id"),
                field(e, "status"),
                repair(e),
                field(e, "repair_pr_url"),
                field(e, "created_at"),
            ]
        })
        .collect();

    output::table(
        &["ID", "Status", "Repair", "Pull request", "Created"],
        &rows,
        None,
    );
}

/// Start a repair run for one event. The run starts within a minute; its
/// progress shows in `floo events list`.
pub fn start_repair(app_flag: Option<&str>, event_id: &str) {
    let event = call(app_flag, |client, app_id| {
        client.start_repair(app_id, event_id)
    });
    output::success(
        "Run queued. If the agent finds a fix, floo opens a draft pull request (repair mode) or records it for `floo events diff` (recommend mode); follow it with `floo events list`.",
        Some(event),
    );
}

/// Print a fix's changed files and patches, the agent's evidence, and the commit
/// to approve; a recommendation has no commit, since floo applied nothing.
pub fn diff(app_flag: Option<&str>, event_id: &str) {
    let diff = call(app_flag, |client, app_id| {
        client.repair_diff(app_id, event_id)
    });
    if output::is_json_mode() {
        output::success("Repair changes.", Some(diff));
        return;
    }
    match diff.get("head_sha").and_then(Value::as_str) {
        Some(sha) => output::info(
            &format!("Commit {sha}. Approve it with: floo events approve {event_id} --commit {sha}"),
            None,
        ),
        None => output::info(
            "A recommended fix: floo didn't apply or test it. Apply it yourself, then check it with the requests below.",
            None,
        ),
    }
    print_evidence(&diff);
    let files = diff.get("files").and_then(Value::as_array);
    let listed = files.map_or(0, Vec::len);
    let changed = diff
        .get("changed_files")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if changed > listed as u64 {
        output::info(
            &format!(
                "GitHub lists {listed} of the {changed} files this fix changes; the rest are not shown."
            ),
            None,
        );
    }
    for file in files.into_iter().flatten() {
        let name = match file.get("previous_filename").and_then(Value::as_str) {
            Some(old) => format!("{old} → {}", field(file, "filename")),
            None => field(file, "filename"),
        };
        output::info(&format!("\n{name} ({})", field(file, "status")), None);
        match file.get("patch").and_then(Value::as_str) {
            Some(patch) => output::info(patch, None),
            None => output::info("No line-by-line view of this file.", None),
        }
    }
}

/// The agent's diagnosis, and the requests that reproduce the error.
fn print_evidence(diff: &Value) {
    if let Some(diagnosis) = diff.get("diagnosis").and_then(Value::as_str) {
        output::info(&format!("\nWhat the agent found:\n{diagnosis}"), None);
    }
    let requests = diff.get("reproduction").and_then(Value::as_array);
    if let Some(requests) = requests.filter(|r| !r.is_empty()) {
        output::info(
            &format!(
                "\nRequests that reproduce the error, to the {} service:",
                field(diff, "reproduction_service")
            ),
            None,
        );
        for request in requests {
            output::info(
                &format!("  {} {}", field(request, "method"), field(request, "path")),
                None,
            );
        }
    }
}

/// Approve a repair's preview at the reviewed commit.
pub fn approve(app_flag: Option<&str>, event_id: &str, commit: &str) {
    let event = call(app_flag, |client, app_id| {
        client.approve_repair(app_id, event_id, commit)
    });
    output::success(
        "Preview approved. floo deploys it, checks that the error is gone, and marks the fix verified.",
        Some(event),
    );
}

/// Merge a verified repair's fix.
pub fn merge(app_flag: Option<&str>, event_id: &str) {
    let event = call(app_flag, |client, app_id| {
        client.merge_repair(app_id, event_id)
    });
    output::success(
        "Fix merged. It deploys like any push to your default branch.",
        Some(event),
    );
}

/// Run one call for the resolved app, exiting with the API's error on failure.
fn call(
    app_flag: Option<&str>,
    request: impl FnOnce(&FlooClient, &str) -> Result<Value, FlooApiError>,
) -> Value {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _app_name) = super::resolve_app_from_config(&client, app_flag);
    match request(&client, &app_id) {
        Ok(value) => value,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    }
}

fn field(event: &Value, key: &str) -> String {
    event
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("-")
        .to_string()
}

/// The run's phase, with its failure reason when it failed.
fn repair(event: &Value) -> String {
    match (
        event.get("repair_phase").and_then(Value::as_str),
        event.get("repair_failure_reason").and_then(Value::as_str),
    ) {
        (Some(phase), Some(reason)) => format!("{phase}: {reason}"),
        (Some(phase), None) => phase.to_string(),
        (None, _) => "-".to_string(),
    }
}
