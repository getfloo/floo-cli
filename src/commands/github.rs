use std::path::Path;
use std::process;

use crate::detection::detect;
use crate::errors::ErrorCode;
use crate::output;
use crate::project_config;

pub fn connect(
    repo: &str,
    app: Option<&str>,
    branch: Option<&str>,
    skip_env_check: bool,
    no_deploy: bool,
) {
    super::require_auth();
    let client = super::init_client(None);

    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    });

    // Resolve app — skip config file reads when --app is provided
    let (app_data, resolved) = if let Some(app_flag) = app {
        // --app provided: look up directly, no local config needed
        let app_data = match crate::resolve::resolve_app(&client, app_flag) {
            Ok(a) => a,
            Err(e) if e.code == "APP_NOT_FOUND" => {
                // Create new app — try local detection for runtime, fall back to "unknown"
                let runtime = detect(&cwd).runtime;
                let spinner = output::Spinner::new(&format!("Creating app {app_flag}..."));
                match client.create_app(app_flag, Some(&runtime)) {
                    Ok(a) => {
                        spinner.finish();
                        a
                    }
                    Err(e) => {
                        spinner.finish();
                        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                        process::exit(1);
                    }
                }
            }
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };
        // Try to load local config for env var import (optional — not required)
        let resolved =
            project_config::resolve_app_context(&cwd, Some(&app_data.name)).ok();
        (app_data, resolved)
    } else {
        // No --app: must resolve from local config
        let resolved_ctx = match project_config::resolve_app_context(&cwd, None) {
            Ok(r) => r,
            Err(e) => {
                output::error(&e.message, &e.code, e.suggestion.as_deref());
                process::exit(1);
            }
        };
        let app_name = resolved_ctx.app_name.clone();

        let app_data = match crate::resolve::resolve_app(&client, &app_name) {
            Ok(a) => a,
            Err(e) if e.code == "APP_NOT_FOUND" => {
                let detection = detect(&cwd);
                let spinner = output::Spinner::new(&format!("Creating app {app_name}..."));
                match client.create_app(&app_name, Some(&detection.runtime)) {
                    Ok(a) => {
                        spinner.finish();
                        a
                    }
                    Err(e) => {
                        spinner.finish();
                        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                        process::exit(1);
                    }
                }
            }
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };

        let resolved =
            project_config::resolve_app_context(&cwd, Some(&app_data.name)).ok();
        (app_data, resolved)
    };

    let app_id = app_data.id.clone();
    let name = app_data.name.clone();

    // Phase 1: Import env vars from local env_file before connecting
    if let Some(ref r) = resolved {
        import_env_vars_for_connect(&client, &app_id, r);
    }

    // Phase 2: Connect to GitHub (handles installation + repo access)
    let result = match client.github_connect(&app_id, repo, branch, skip_env_check) {
        Ok(r) => r,
        Err(e) if e.code == "GITHUB_APP_NOT_INSTALLED" => {
            let install_url = e
                .extra
                .as_ref()
                .and_then(|v| v.get("install_url"))
                .and_then(|v| v.as_str())
                .unwrap_or("https://github.com/apps/getfloo/installations/new");

            let owner = repo.split('/').next().unwrap_or(repo);
            if !output::is_json_mode() {
                output::warn(&format!("Floo GitHub App not installed on \"{owner}\""));
            }

            run_installation_flow(&client, install_url);

            match client.github_connect(&app_id, repo, branch, skip_env_check) {
                Ok(r) => r,
                Err(e2) => {
                    output::error(&e2.message, &ErrorCode::from_api(&e2.code), None);
                    process::exit(1);
                }
            }
        }
        Err(e) if e.code == "GITHUB_REPO_NOT_IN_INSTALLATION" => {
            let settings_url = e
                .extra
                .as_ref()
                .and_then(|v| v.get("settings_url"))
                .and_then(|v| v.as_str());

            let owner = repo.split('/').next().unwrap_or(repo);
            let fallback_url =
                format!("https://github.com/organizations/{owner}/settings/installations");
            let url = settings_url.unwrap_or(&fallback_url);

            if !output::is_json_mode() {
                output::warn(&format!(
                    "Floo GitHub App does not have access to \"{repo}\"."
                ));
                output::info("Opening GitHub settings to grant access...", None);
            }

            if let Err(e) = open::that(url) {
                if !output::is_json_mode() {
                    output::warn(&format!("Could not open browser: {e}"));
                    output::warn(&format!("Open this URL manually: {url}"));
                }
            }

            poll_repo_access(&client, repo, url);

            // Repo is now accessible — connect
            match client.github_connect(&app_id, repo, branch, skip_env_check) {
                Ok(r) => r,
                Err(e2) => {
                    output::error(&e2.message, &ErrorCode::from_api(&e2.code), None);
                    process::exit(1);
                }
            }
        }
        Err(e) => {
            let suggestion = match e.code.as_str() {
                "GITHUB_ALREADY_CONNECTED" => {
                    Some("Disconnect first: floo apps github disconnect --app <name>")
                }
                "GITHUB_REPO_NOT_ACCESSIBLE" => {
                    Some("Ensure the GitHub App is installed on the repo's organization.")
                }
                _ => None,
            };
            output::error(&e.message, &ErrorCode::from_api(&e.code), suggestion);
            process::exit(1);
        }
    };
    let connected_branch = result.default_branch.as_deref().unwrap_or("(unknown)");

    // Phase 4: Deploy and wait (unless --no-deploy)
    if no_deploy {
        output::success(
            &format!("Connected {name} to {repo} (branch: {connected_branch})"),
            Some(serde_json::json!({
                "connected": true,
                "app": name,
                "repo": repo,
                "branch": connected_branch,
                "deployed": false,
            })),
        );
        return;
    }

    let deploy_result = run_initial_deploy(&client, &app_id, &cwd);

    // Phase 5: One success/failure message
    match deploy_result {
        DeployOutcome::Live { url, deploy } => {
            output::success(
                &format!("Connected {name} to {repo} — deployed and live at {url}"),
                Some(serde_json::json!({
                    "connected": true,
                    "app": name,
                    "repo": repo,
                    "branch": connected_branch,
                    "deployed": true,
                    "deploy_status": "live",
                    "url": url,
                    "deploy": deploy,
                })),
            );
        }
        DeployOutcome::Failed { deploy } => {
            output::error_with_data(
                &format!("Connected {name} to {repo} but deploy failed."),
                &ErrorCode::DeployFailed,
                Some("Run `floo redeploy` to retry."),
                Some(serde_json::json!({
                    "connected": true,
                    "app": name,
                    "repo": repo,
                    "branch": connected_branch,
                    "deployed": false,
                    "deploy_status": "failed",
                    "deploy": deploy,
                })),
            );
            process::exit(1);
        }
    }
}

pub fn disconnect(app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, name) = super::resolve_app_from_config(&client, app);

    if let Err(e) = client.github_disconnect(&app_id) {
        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
        process::exit(1);
    }

    output::success(
        &format!("Disconnected {name} from GitHub."),
        Some(serde_json::json!({"app": name})),
    );
}

pub fn status(app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, name) = super::resolve_app_from_config(&client, app);

    match client.github_status(&app_id) {
        Ok(conn) => {
            if output::is_json_mode() {
                output::success(
                    &format!("{name} GitHub connection"),
                    Some(output::to_value(&conn)),
                );
            } else {
                let repo = conn
                    .repo_full_name
                    .as_deref()
                    .or_else(|| {
                        conn.services
                            .first()
                            .and_then(|s| s.repo_full_name.as_deref())
                    })
                    .unwrap_or("(not set)");
                let branch = conn
                    .default_branch
                    .as_deref()
                    .or_else(|| {
                        conn.services
                            .first()
                            .and_then(|s| s.default_branch.as_deref())
                    })
                    .unwrap_or("(unknown)");
                output::info(&format!("{name} GitHub connection"), None);
                output::info(&format!("  Repo:      {repo}"), None);
                output::info(&format!("  Branch:    {branch}"), None);
                output::info(&format!("  Connected: {}", conn.connected_at), None);
            }
        }
        Err(e) if e.code == "GITHUB_NOT_CONNECTED" => {
            if output::is_json_mode() {
                output::success(
                    "Not connected",
                    Some(serde_json::json!({"connected": false})),
                );
            } else {
                output::info(&format!("{name} is not connected to GitHub."), None);
            }
        }
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    }
}

/// Poll the lightweight check-repo-access endpoint until the repo is accessible.
fn poll_repo_access(client: &crate::api_client::FlooClient, repo: &str, settings_url: &str) {
    let spinner = output::Spinner::new(&format!(
        "Waiting for repo access (grant at {settings_url})..."
    ));
    let poll_interval = std::time::Duration::from_secs(1);
    let timeout = std::time::Duration::from_secs(180);
    let start = std::time::Instant::now();

    loop {
        std::thread::sleep(poll_interval);

        if start.elapsed() > timeout {
            spinner.finish();
            output::error(
                "Timed out waiting for repository access.",
                &ErrorCode::Other("REPO_ACCESS_TIMEOUT".into()),
                Some(&format!(
                    "Grant access at {settings_url} then re-run: \
                     floo apps github connect {repo}"
                )),
            );
            process::exit(1);
        }

        match client.github_check_repo_access(repo) {
            Ok(resp) => {
                if resp.get("accessible").and_then(|v| v.as_bool()) == Some(true) {
                    spinner.finish();
                    return;
                }
            }
            Err(e) => {
                let is_transient = e.status_code == 0 || e.status_code >= 500;
                if !is_transient {
                    spinner.finish();
                    output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                    process::exit(1);
                }
            }
        }
    }
}

fn run_installation_flow(client: &crate::api_client::FlooClient, install_url: &str) {
    // Begin the setup session (stores pending state in Redis)
    if let Err(e) = client.github_setup_begin() {
        // Permanent errors (4xx) mean the flow is doomed — abort early
        if e.status_code > 0 && e.status_code < 500 {
            output::error(
                &format!("Failed to start setup session: {}", e.message),
                &ErrorCode::from_api(&e.code),
                Some("Check your authentication with: floo auth whoami"),
            );
            process::exit(1);
        }
        output::warn(&format!(
            "Setup session may not have been created: {}. Continuing...",
            e.message
        ));
    }

    // Open browser for installation
    if !output::is_json_mode() {
        output::info("Opening browser to install...", None);
    }
    if let Err(e) = open::that(install_url) {
        output::warn(&format!("Could not open browser: {e}"));
        output::warn(&format!("Open this URL manually: {install_url}"));
    }

    // Poll for installation completion (3s interval, 5 min timeout)
    let spinner = output::Spinner::new("Waiting for installation...");
    let poll_interval = std::time::Duration::from_secs(3);
    let timeout = std::time::Duration::from_secs(300);
    let start = std::time::Instant::now();

    loop {
        std::thread::sleep(poll_interval);

        if start.elapsed() > timeout {
            spinner.finish();
            output::error(
                "Timed out waiting for GitHub App installation.",
                &ErrorCode::Other("SETUP_TIMEOUT".into()),
                Some("Install the app manually, then re-run: floo apps github connect <owner/repo> --app <name>"),
            );
            process::exit(1);
        }

        match client.github_setup_poll() {
            Ok(resp) => {
                let status = resp
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("none");
                if status == "ready" {
                    spinner.finish();
                    if resp
                        .get("installation_id")
                        .and_then(|v| v.as_u64())
                        .is_none()
                    {
                        output::error(
                            "GitHub App was installed but the server did not return an installation ID.",
                            &ErrorCode::InvalidResponse,
                            Some("Try running the command again — the installation should be detected automatically."),
                        );
                        process::exit(1);
                    }
                    return;
                }
            }
            Err(e) => {
                // Only tolerate transient failures (network issues, 5xx).
                // Permanent errors (4xx) should abort immediately.
                let is_transient = e.status_code == 0 || e.status_code >= 500;
                if !is_transient {
                    spinner.finish();
                    output::error(
                        &format!("Poll failed: {}", e.message),
                        &ErrorCode::from_api(&e.code),
                        None,
                    );
                    process::exit(1);
                }
            }
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

enum DeployOutcome {
    Live {
        url: String,
        deploy: serde_json::Value,
    },
    Failed {
        deploy: serde_json::Value,
    },
}

fn run_initial_deploy(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    project_path: &Path,
) -> DeployOutcome {
    let detection = detect(project_path);

    let spinner = output::Spinner::new("Deploying...");
    let mut deploy_data = match client.create_deploy(
        app_id,
        &detection.runtime,
        detection.framework.as_deref(),
        None, // API discovers services from GitHub tarball
        None, // access_mode
        None, // agent_mode
        None, // auth_redirect_uris
        None, // reparo_config
        None, // cron_jobs
        None, // github_config
    ) {
        Ok(d) => {
            spinner.finish();
            d
        }
        Err(e) => {
            spinner.finish();
            return DeployOutcome::Failed {
                deploy: serde_json::json!({"error": e.message}),
            };
        }
    };

    let initial_status = deploy_data.status.as_deref().unwrap_or("");

    if !super::deploy::TERMINAL_STATUSES.contains(&initial_status) {
        if deploy_data.id.is_empty() {
            return DeployOutcome::Failed {
                deploy: serde_json::json!({"error": "Deploy missing 'id' in response"}),
            };
        }
        let deploy_id = deploy_data.id.clone();

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

    let final_status = deploy_data.status.as_deref().unwrap_or("");

    if final_status == "failed" {
        DeployOutcome::Failed {
            deploy: output::to_value(&deploy_data),
        }
    } else if final_status == "live" {
        let url = deploy_data.url.as_deref().unwrap_or("").to_string();
        DeployOutcome::Live {
            url,
            deploy: output::to_value(&deploy_data),
        }
    } else {
        // Ambiguous status (timeout, cancelled, unknown) — report as failed
        output::warn(&format!("Deploy ended with unexpected status: {}", final_status));
        DeployOutcome::Failed {
            deploy: output::to_value(&deploy_data),
        }
    }
}
