use std::process;

use serde_json::Value;

use crate::errors::ErrorCode;
use crate::output;

pub fn list(app_flag: Option<&str>, status_filter: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _app_name) = super::resolve_app_from_config(&client, app_flag);

    let result = match client.agent_events(&app_id, status_filter) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

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
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _app_name) = super::resolve_app_from_config(&client, app_flag);

    match client.start_repair(&app_id, event_id) {
        Ok(event) => output::success(
            "Repair queued. floo opens a draft pull request if it finds a fix; follow it with `floo events list`.",
            Some(event),
        ),
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
