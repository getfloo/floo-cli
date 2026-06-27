use std::process;
use std::thread;
use std::time::{Duration, Instant};

use crate::api_client::FlooClient;
use crate::api_types::{CreatePreviewDeployRequest, Deploy, LogEntry, PreviewEnvironment};
use crate::errors::ErrorCode;
use crate::output;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT: Duration = Duration::from_secs(600);
const TERMINAL_STATUSES: &[&str] = &["live", "failed", "cancelled", "superseded"];

pub fn up(
    app_flag: Option<&str>,
    branch: &str,
    wait: bool,
    runtime: &str,
    commit_sha: Option<&str>,
    ref_name: Option<&str>,
) {
    let branch = branch.trim();
    if branch.is_empty() {
        output::error(
            "--branch cannot be empty.",
            &ErrorCode::InvalidFormat,
            Some("Pass the remote GitHub branch to deploy, e.g. --branch feat/my-change."),
        );
        process::exit(1);
    }

    if output::is_dry_run_mode() {
        output::dry_run_preview(
            &format!(
                "Would create a preview sandbox from remote GitHub branch '{branch}'. Local dirty files are not uploaded."
            ),
            serde_json::json!({
                "action": "preview_up",
                "app": app_flag,
                "branch": branch,
                "runtime": runtime,
                "commit_sha": commit_sha,
                "ref": ref_name,
                "environment": "preview",
                "github_source_only": true,
                "dev_prod_untouched": true,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);

    if !output::is_json_mode() {
        output::info(
            &format!("Creating preview for {app_name} from branch {branch}..."),
            None,
        );
    }

    let request = CreatePreviewDeployRequest {
        runtime,
        environment: "preview",
        branch,
        commit_sha,
        ref_name,
    };
    let mut deploy = match client.create_preview_deploy(&app_id, &request) {
        Ok(deploy) => deploy,
        Err(e) => {
            output::error(
                &e.message,
                &ErrorCode::from_api(&e.code),
                preview_api_suggestion(&e.code),
            );
            process::exit(1);
        }
    };

    if wait {
        deploy = poll_deploy(&client, &app_id, &deploy);
    }

    let preview = deploy
        .preview_slug
        .as_deref()
        .and_then(|slug| client.get_preview(&app_id, slug).ok());
    emit_preview_command_result(
        "Preview deploy created.",
        &app_id,
        &app_name,
        &deploy,
        preview.as_ref(),
    );

    if wait && deploy.status.as_deref() != Some("live") {
        process::exit(1);
    }
}

pub fn list(app_flag: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let listing = match client.list_previews(&app_id) {
        Ok(listing) => listing,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            "Previews retrieved.",
            Some(serde_json::json!({
                "app": app_json(&app_id, &app_name),
                "previews": listing.previews.iter().map(preview_json).collect::<Vec<_>>(),
                "total": listing.total,
                "dev_prod_untouched": true,
            })),
        );
        return;
    }

    if listing.previews.is_empty() {
        output::info(&format!("No previews found for {app_name}."), None);
        return;
    }
    let rows: Vec<Vec<String>> = listing
        .previews
        .iter()
        .map(|preview| {
            vec![
                preview.slug.clone(),
                preview
                    .source_branch
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                preview
                    .latest_deploy_status
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                preview.url.clone().unwrap_or_else(|| "-".to_string()),
                preview
                    .expires_at
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();
    output::table(
        &["Preview", "Branch", "Status", "URL", "Expires"],
        &rows,
        None,
    );
}

pub fn status(app_flag: Option<&str>, preview_identifier: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let preview = resolve_preview(&client, &app_id, &app_name, preview_identifier);

    if output::is_json_mode() {
        output::success(
            "Preview status retrieved.",
            Some(serde_json::json!({
                "app": app_json(&app_id, &app_name),
                "preview": preview_json(&preview),
                "source_branch": preview.source_branch,
                "deploy_id": preview.latest_deploy_id,
                "status": preview.latest_deploy_status,
                "url": preview.url,
                "expires_at": preview.expires_at,
                "database_branches": output::to_value(&preview.database_branches),
                "dev_prod_untouched": true,
            })),
        );
        return;
    }

    output::info(&format!("Preview {} for {app_name}", preview.slug), None);
    render_preview(&preview);
    output::info(
        &format!("Next: floo previews logs {} --app {app_name}", preview.slug),
        None,
    );
}

pub fn delete(app_flag: Option<&str>, preview_identifier: &str, yes: bool) {
    use crate::confirm::{confirm_tier2, ConfirmOutcome, RiskMetadata, Tier};

    if output::is_dry_run_mode() {
        let preview = normalize_preview_identifier(preview_identifier)
            .unwrap_or_else(|| preview_identifier.trim().to_string());
        let risk: RiskMetadata = Tier::Two.into();
        output::dry_run_preview(
            &format!(
                "Would delete preview sandbox '{preview}'. Cloud Run services, preview-owned resources, routes, and env vars would be torn down; dev and prod are untouched."
            ),
            serde_json::json!({
                "action": "preview_delete",
                "app": app_flag,
                "preview": preview,
                "destructive": risk.destructive,
                "data_loss": risk.data_loss,
                "tier": risk.tier,
                "scope": "preview",
                "dev_prod_untouched": true,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let preview = resolve_preview(&client, &app_id, &app_name, preview_identifier);

    match confirm_tier2(
        "Delete preview sandbox",
        &format!("{} on {app_name} (dev/prod untouched)", preview.slug),
        yes,
    ) {
        ConfirmOutcome::Proceed => {}
        ConfirmOutcome::Aborted => {
            if output::is_json_mode() {
                output::success("Cancelled.", Some(serde_json::json!({"cancelled": true})));
            } else {
                output::info("Cancelled.", None);
            }
            process::exit(0);
        }
        ConfirmOutcome::Refused { suggestion } => {
            crate::confirm::exit_refused(
                &format!(
                    "Refusing to delete preview '{}' on {app_name} without explicit confirmation; dev/prod untouched.",
                    preview.slug
                ),
                &suggestion,
            );
        }
    }

    if let Err(e) = client.delete_preview(&app_id, &preview.slug) {
        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
        process::exit(1);
    }

    if output::is_json_mode() {
        output::success(
            "Preview deleted.",
            Some(serde_json::json!({
                "app": app_json(&app_id, &app_name),
                "preview": preview_json(&preview),
                "deleted": true,
                "scope": "preview",
                "dev_prod_untouched": true,
            })),
        );
    } else {
        output::success(
            &format!(
                "Deleted preview {} for {app_name}. Dev and prod were untouched.",
                preview.slug
            ),
            None,
        );
    }
}

pub fn logs(app_flag: Option<&str>, preview_identifier: &str, follow: bool, tail: u32) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let preview = resolve_preview(&client, &app_id, &app_name, preview_identifier);
    let Some(deploy_id) = preview.latest_deploy_id.as_deref() else {
        output::error(
            &format!("Preview '{}' has no deploy yet.", preview.slug),
            &ErrorCode::DeployNotFound,
            Some("Run `floo previews status` to inspect the preview lifecycle."),
        );
        process::exit(1);
    };

    if follow {
        let streamed = if output::is_json_mode() {
            crate::commands::deploy::stream_deploy_json(&client, &app_id, deploy_id)
        } else {
            crate::commands::deploy::stream_deploy(&client, &app_id, deploy_id)
        };
        match streamed {
            Ok(_) => return,
            Err(e) if e.code == "NOT_FOUND" || e.code == "STREAM_ERROR" => {}
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        }
    }

    let response = match client.get_logs(
        &app_id,
        tail,
        None,
        None,
        None,
        None,
        None,
        Some(deploy_id),
        Some("preview"),
        None,
    ) {
        Ok(response) => response,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            "Preview logs retrieved.",
            Some(serde_json::json!({
                "app": app_json(&app_id, &app_name),
                "preview": preview_json(&preview),
                "deploy_id": deploy_id,
                "logs": output::to_value(&response.logs),
                "total": response.total,
                "dev_prod_untouched": true,
            })),
        );
        return;
    }

    if response.logs.is_empty() {
        output::info("No logs found for this preview deploy.", None);
        return;
    }
    for entry in response.logs {
        output::info(&format_log_line(&entry), None);
    }
}

fn poll_deploy(client: &FlooClient, app_id: &str, initial: &Deploy) -> Deploy {
    let started = Instant::now();
    let mut deploy = initial.clone();
    while !TERMINAL_STATUSES.contains(&deploy.status.as_deref().unwrap_or("")) {
        if !output::is_json_mode() {
            output::info(
                &format!(
                    "Deploy {}...",
                    deploy.status.as_deref().unwrap_or("pending")
                ),
                None,
            );
        }
        if started.elapsed() >= POLL_TIMEOUT {
            output::error(
                "Preview deploy timed out after 10 minutes.",
                &ErrorCode::DeployTimeout,
                Some("The deploy may still complete. Run `floo previews status <preview>`."),
            );
            process::exit(1);
        }
        thread::sleep(POLL_INTERVAL);
        deploy = match client.get_deploy(app_id, &deploy.id) {
            Ok(deploy) => deploy,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };
    }
    deploy
}

fn emit_preview_command_result(
    message: &str,
    app_id: &str,
    app_name: &str,
    deploy: &Deploy,
    preview: Option<&PreviewEnvironment>,
) {
    if output::is_json_mode() {
        output::success(
            message,
            Some(serde_json::json!({
                "app": app_json(app_id, app_name),
                "preview": preview.map(preview_json).unwrap_or_else(|| serde_json::json!({
                    "slug": deploy.preview_slug,
                    "environment_name": "preview",
                    "source_branch": deploy.source_branch,
                    "url": deploy.url,
                })),
                "source_branch": deploy.source_branch,
                "deploy_id": deploy.id,
                "status": deploy.status,
                "url": deploy.url,
                "expires_at": preview.and_then(|p| p.expires_at.clone()),
                "database_branches": preview
                    .map(|p| output::to_value(&p.database_branches))
                    .unwrap_or_else(|| serde_json::json!([])),
                "dev_prod_untouched": true,
            })),
        );
        return;
    }

    let status = deploy.status.as_deref().unwrap_or("unknown");
    let slug = deploy.preview_slug.as_deref().unwrap_or("(pending)");
    output::success(&format!("Preview {slug} is {status}."), None);
    if let Some(url) = deploy
        .url
        .as_deref()
        .or_else(|| preview.and_then(|p| p.url.as_deref()))
    {
        output::info(&format!("URL: {url}"), None);
    }
    output::info(
        &format!("Next: floo previews status {slug} --app {app_name}"),
        None,
    );
}

fn resolve_preview(
    client: &FlooClient,
    app_id: &str,
    app_name: &str,
    preview_identifier: &str,
) -> PreviewEnvironment {
    if let Some(slug) = normalize_preview_identifier(preview_identifier) {
        if let Ok(preview) = client.get_preview(app_id, &slug) {
            return preview;
        }
    }

    let listing = match client.list_previews(app_id) {
        Ok(response) => response,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };
    let matches: Vec<PreviewEnvironment> = listing
        .previews
        .into_iter()
        .filter(|preview| {
            preview.slug == preview_identifier
                || preview.source_branch.as_deref() == Some(preview_identifier)
                || preview.url.as_deref() == Some(preview_identifier)
                || preview
                    .pr_number
                    .map(|pr| format!("#{pr}") == preview_identifier)
                    .unwrap_or(false)
        })
        .collect();

    match matches.as_slice() {
        [preview] => preview.clone(),
        [] => {
            output::error(
                &format!("Preview '{preview_identifier}' not found on {app_name}."),
                &ErrorCode::Other("PREVIEW_NOT_FOUND".to_string()),
                Some("Pass a preview slug, source branch, preview URL, or unambiguous #PR."),
            );
            process::exit(1);
        }
        _ => {
            output::error(
                &format!("Preview identifier '{preview_identifier}' is ambiguous on {app_name}."),
                &ErrorCode::Other("AMBIGUOUS_PREVIEW_IDENTIFIER".to_string()),
                Some("Pass the exact preview slug from `floo previews list`."),
            );
            process::exit(1);
        }
    }
}

fn normalize_preview_identifier(identifier: &str) -> Option<String> {
    let trimmed = identifier.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    if let Some(prefix_start) = host.find("-preview-") {
        let after_prefix = &host[prefix_start + "-preview-".len()..];
        let label = after_prefix.split('.').next().unwrap_or(after_prefix);
        return Some(label.to_string());
    }
    if trimmed.contains("://")
        || trimmed.contains(".")
        || trimmed.contains('/')
        || trimmed.starts_with('#')
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn app_json(app_id: &str, app_name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": app_id,
        "name": app_name,
    })
}

fn preview_json(preview: &PreviewEnvironment) -> serde_json::Value {
    serde_json::json!({
        "id": preview.id,
        "slug": preview.slug,
        "environment_name": "preview",
        "source_branch": preview.source_branch,
        "pr_number": preview.pr_number,
        "url": preview.url,
        "latest_deploy_id": preview.latest_deploy_id,
        "latest_deploy_status": preview.latest_deploy_status,
        "latest_commit_sha": preview.latest_commit_sha,
        "ttl_hours": preview.ttl_hours,
        "expires_at": preview.expires_at,
        "resources": output::to_value(&preview.resources),
        "database_branches": output::to_value(&preview.database_branches),
    })
}

fn render_preview(preview: &PreviewEnvironment) {
    output::info(
        &format!(
            "  Branch:  {}",
            preview.source_branch.as_deref().unwrap_or("-")
        ),
        None,
    );
    output::info(
        &format!(
            "  Status:  {}",
            preview.latest_deploy_status.as_deref().unwrap_or("-")
        ),
        None,
    );
    output::info(
        &format!("  URL:     {}", preview.url.as_deref().unwrap_or("-")),
        None,
    );
    output::info(
        &format!(
            "  Expires: {}",
            preview.expires_at.as_deref().unwrap_or("-")
        ),
        None,
    );
}

fn format_log_line(entry: &LogEntry) -> String {
    let ts = entry.timestamp.as_deref().unwrap_or("");
    let sev = entry.severity.as_deref().unwrap_or("DEFAULT");
    let msg = entry.message.as_deref().unwrap_or("");
    let service = entry
        .service_name
        .as_deref()
        .map(|name| format!(" [{name}]"))
        .unwrap_or_default();
    format!("{ts}{service} [{sev}] {msg}")
}

fn preview_api_suggestion(code: &str) -> Option<&'static str> {
    match code {
        "PREVIEW_MANAGED_SERVICE_ISOLATION_UNAVAILABLE" => Some(
            "Preview managed-service isolation could not be provisioned. Fix the attached managed service state, then retry.",
        ),
        "INVALID_PREVIEW_BRANCH" => Some("Push a valid remote GitHub branch and pass it with --branch."),
        _ => None,
    }
}
