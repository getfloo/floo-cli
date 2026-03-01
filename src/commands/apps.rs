use std::path::Path;
use std::process;

use crate::archive::create_archive;
use crate::detection::detect;
use crate::output;
use crate::project_config::{self, AppAccessMode};

pub fn list(page: u32, per_page: u32) {
    super::require_auth();
    let client = super::init_client(None);
    let result = match client.list_apps(page, per_page) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let apps = result
        .get("apps")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let total = match result.get("total").and_then(|v| v.as_u64()) {
        Some(t) => t as u32,
        None => {
            eprintln!("Warning: API response missing 'total' field; pagination may be inaccurate.");
            apps.len() as u32
        }
    };

    if apps.is_empty() {
        if !output::is_json_mode() {
            output::info("No apps yet. Deploy one with floo deploy.", None);
        } else {
            output::success(
                "No apps.",
                Some(
                    serde_json::json!({"apps": [], "total": total, "page": page, "per_page": per_page}),
                ),
            );
        }
        return;
    }

    let rows: Vec<Vec<String>> = apps
        .iter()
        .map(|a| {
            vec![
                a.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                a.get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                a.get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("\u{2014}")
                    .to_string(),
                a.get("runtime")
                    .and_then(|v| v.as_str())
                    .unwrap_or("\u{2014}")
                    .to_string(),
                a.get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
            ]
        })
        .collect();

    output::table(
        &["Name", "Status", "URL", "Runtime", "Created"],
        &rows,
        Some(serde_json::json!({"apps": apps, "total": total, "page": page, "per_page": per_page})),
    );

    if !output::is_json_mode() {
        if let Some(shown) = page.checked_mul(per_page) {
            if total > shown {
                let remaining = total - shown;
                output::dim_line(&format!(
                    "{remaining} more app{} not shown. Use --page {} to see next page.",
                    if remaining == 1 { "" } else { "s" },
                    page + 1
                ));
            }
        }
    }
}

pub fn status(app_name: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let app_data = super::resolve_app_or_exit(&client, app_name);

    if output::is_json_mode() {
        let name = app_data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(app_name);
        output::success(&format!("App {name}"), Some(app_data));
    } else {
        let name = app_data.get("name").and_then(|v| v.as_str()).unwrap_or("-");
        let st = app_data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let url = app_data
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("\u{2014}");
        let runtime = app_data
            .get("runtime")
            .and_then(|v| v.as_str())
            .unwrap_or("\u{2014}");
        let id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("-");
        let created = app_data
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("-");

        output::info(name, None);
        output::info(&format!("  Status:   {st}"), None);
        output::info(&format!("  URL:      {url}"), None);
        output::info(&format!("  Runtime:  {runtime}"), None);
        output::info(&format!("  ID:       {id}"), None);
        output::info(&format!("  Created:  {created}"), None);
    }
}

pub fn delete(app_name: &str, force: bool) {
    super::require_auth();
    let client = super::init_client(None);
    let app_data = super::resolve_app_or_exit(&client, app_name);

    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    if !force && !output::confirm(&format!("Delete app '{name}'? This cannot be undone.")) {
        if !output::is_json_mode() {
            output::info("Cancelled.", None);
        }
        process::exit(0);
    }

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");

    if let Err(e) = client.delete_app(app_id) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }

    output::success(
        &format!("Deleted app '{name}'."),
        Some(serde_json::json!({"id": app_id})),
    );
}

pub fn connect(
    repo: &str,
    installation_id: u64,
    app_name: &str,
    branch: Option<&str>,
    skip_env_check: bool,
    no_deploy: bool,
) {
    super::require_auth();
    let client = super::init_client(None);
    let app_data = super::resolve_app_or_exit(&client, app_name);

    let app_id = super::expect_str_field(&app_data, "id").to_string();
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name)
        .to_string();

    // Try to load project config from cwd to import env vars and trigger deploy
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            "FILE_ERROR",
            None,
        );
        process::exit(1);
    });
    // Project config is optional for connect — missing config is fine, all other
    // errors (LEGACY_CONFIG, INVALID_PROJECT_CONFIG) are also suppressed so the
    // connect itself still succeeds; users can run `floo env import` separately.
    let resolved = project_config::resolve_app_context(&cwd, Some(app_name)).ok();

    // Step 1: Import env vars from local env_file before connecting
    if let Some(ref r) = resolved {
        import_env_vars_for_connect(&client, &app_id, r);
    }

    // Step 2: Connect to GitHub
    let result = match client.github_connect(&app_id, repo, installation_id, branch, skip_env_check)
    {
        Ok(r) => r,
        Err(e) => {
            let suggestion = match e.code.as_str() {
                "GITHUB_ALREADY_CONNECTED" => {
                    Some("Disconnect first: floo apps disconnect --app <name>")
                }
                "GITHUB_REPO_NOT_ACCESSIBLE" => {
                    Some("Ensure the GitHub App is installed on the repo's organization.")
                }
                _ => None,
            };
            output::error(&e.message, &e.code, suggestion);
            process::exit(1);
        }
    };

    let connected_branch = result
        .get("default_branch")
        .and_then(|v| v.as_str())
        .unwrap_or("main")
        .to_string();

    output::success(
        &format!("Connected {name} to {repo} (branch: {connected_branch})"),
        Some(result),
    );

    // Step 3: Trigger initial deploy unless --no-deploy
    if !no_deploy {
        if let Some(ref r) = resolved {
            trigger_initial_deploy(&client, &app_id, &cwd, r);
        } else if !output::is_json_mode() {
            output::info(
                "No project config found. Run `floo deploy` to trigger the first deploy.",
                None,
            );
        }
    }
}

fn import_env_vars_for_connect(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    resolved: &project_config::ResolvedApp,
) {
    super::deploy::sync_env_vars_if_needed(client, app_id, resolved, true);
}

fn trigger_initial_deploy(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    project_path: &Path,
    resolved: &project_config::ResolvedApp,
) {
    if !output::is_json_mode() {
        output::info("Deploying...", None);
    }

    let detection = detect(project_path);

    let services = match project_config::discover_services(resolved) {
        Ok(svcs) => svcs,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let spinner = output::Spinner::new("Packaging source...");
    let archive_path = match create_archive(project_path) {
        Ok(p) => {
            spinner.finish();
            p
        }
        Err(e) => {
            spinner.finish();
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let access_mode: Option<AppAccessMode> = resolved
        .app_config
        .as_ref()
        .and_then(|c| c.app.access_mode)
        .or_else(|| {
            resolved
                .service_config
                .as_ref()
                .and_then(|c| c.app.access_mode)
        });

    let spinner = output::Spinner::new("Uploading...");
    let mut deploy_data = match client.create_deploy(
        app_id,
        &archive_path,
        &detection.runtime,
        detection.framework.as_deref(),
        Some(&services),
        access_mode.as_ref().map(|m| m.as_str()),
    ) {
        Ok(d) => {
            spinner.finish();
            d
        }
        Err(e) => {
            spinner.finish();
            super::deploy::cleanup(&archive_path);
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    super::deploy::cleanup(&archive_path);

    let initial_status = deploy_data
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !super::deploy::TERMINAL_STATUSES.contains(&initial_status) {
        let deploy_id = match deploy_data.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                output::error(
                    "Unexpected API response: deploy is missing required 'id'.",
                    "INVALID_RESPONSE",
                    Some("This may indicate a CLI/API mismatch. Check for updates with `floo update`."),
                );
                process::exit(1);
            }
        };

        if !output::is_json_mode() {
            match super::deploy::stream_deploy(client, app_id, &deploy_id) {
                Ok(d) => deploy_data = d,
                Err(_) => deploy_data = super::deploy::poll_deploy(client, app_id, &deploy_data),
            }
        } else {
            match super::deploy::stream_deploy_json(client, app_id, &deploy_id) {
                Ok(d) => deploy_data = d,
                Err(_) => deploy_data = super::deploy::poll_deploy(client, app_id, &deploy_data),
            }
        }
    }

    let final_status = deploy_data
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if final_status == "failed" {
        output::error_with_data(
            "Deploy failed.",
            "DEPLOY_FAILED",
            Some("Check build output above, or run `floo logs` for details."),
            Some(deploy_data),
        );
        process::exit(1);
    }

    let url = deploy_data
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    output::success(&format!("Deployed to {url}"), Some(deploy_data));
}

pub fn disconnect(app_name: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let app_data = super::resolve_app_or_exit(&client, app_name);

    let app_id = super::expect_str_field(&app_data, "id");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    if let Err(e) = client.github_disconnect(app_id) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }

    output::success(
        &format!("Disconnected {name} from GitHub."),
        Some(serde_json::json!({"app": name})),
    );
}
