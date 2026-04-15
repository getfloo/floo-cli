use std::process;

use crate::errors::ErrorCode;
use crate::output;

pub fn events(app_flag: Option<&str>, status_filter: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _app_name) = super::resolve_app_from_config(&client, app_flag);

    let result = match client.reparo_events(&app_id, status_filter) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success("Reparo events.", Some(result));
        return;
    }

    let events_arr = result.get("events").and_then(|v| v.as_array());
    let Some(events) = events_arr else {
        output::info("No Reparo events.", None);
        return;
    };

    if events.is_empty() {
        output::info("No Reparo events.", None);
        return;
    }

    let rows: Vec<Vec<String>> = events
        .iter()
        .map(|e| {
            let id = e
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let status = e
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let trigger = e
                .get("trigger")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let created_at = e
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            vec![id, status, trigger, created_at]
        })
        .collect();

    output::table(&["ID", "Status", "Trigger", "Created"], &rows, None);
}
