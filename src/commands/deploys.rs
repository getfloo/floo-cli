use std::process;
use std::thread;
use std::time::{Duration, Instant};

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
                "DEPLOY_NOT_FOUND" => Some("Check the deploy ID: floo deploy list --app <name>"),
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
