use std::process;

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
                .map(|s| if s.len() > 7 { &s[..7] } else { s })
                .unwrap_or("\u{2014}");
            vec![
                d.id.clone(),
                d.status.as_deref().unwrap_or("-").to_string(),
                d.triggered_by.as_deref().unwrap_or("\u{2014}").to_string(),
                commit.to_string(),
                d.created_at.as_deref().unwrap_or("-").to_string(),
            ]
        })
        .collect();

    output::table(
        &["Deploy ID", "Status", "Triggered By", "Commit", "Created"],
        &rows,
        Some(output::to_value(&result)),
    );
}

pub fn logs(deploy_id: &str, app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, _app_name) = super::resolve_app_from_config(&client, app);

    let deploy = match client.get_deploy(&app_id, deploy_id) {
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

    if output::is_json_mode() {
        output::success(
            "Deploy logs retrieved.",
            Some(serde_json::json!({
                "deploy_id": deploy.id,
                "status": deploy.status,
                "build_logs": deploy.build_logs,
            })),
        );
        return;
    }

    match &deploy.build_logs {
        Some(logs) if !logs.is_empty() => output::info(logs, None),
        _ => {
            let status = deploy.status.as_deref().unwrap_or("unknown");
            let msg = match status {
                "pending" | "building" => {
                    "Build logs not yet available (deploy is still in progress)."
                }
                "failed" => "No build logs captured for this deploy.",
                _ => "No build logs available.",
            };
            output::info(msg, None);
        }
    }
}
