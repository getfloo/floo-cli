use std::path::Path;
use std::process;

use crate::detection::detect;
use crate::errors::ErrorCode;
use crate::output;
use crate::project_config::{self, AppAccessMode};

pub fn connect(
    repo: &str,
    app: Option<&str>,
    branch: Option<&str>,
    skip_env_check: bool,
    no_deploy: bool,
) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, name) = super::resolve_app_from_config(&client, app);

    // Try to load project config from cwd to import env vars and trigger deploy
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    });
    // Project config is optional for connect — missing config is fine, all other
    // errors (LEGACY_CONFIG, INVALID_PROJECT_CONFIG) are also suppressed so the
    // connect itself still succeeds; users can run `floo env import` separately.
    let resolved = project_config::resolve_app_context(&cwd, Some(&name)).ok();

    // Step 1: Import env vars from local env_file before connecting
    if let Some(ref r) = resolved {
        import_env_vars_for_connect(&client, &app_id, r);
    }

    // Step 2: Connect to GitHub (installation_id auto-resolved by the API)
    let result = match client.github_connect(&app_id, repo, branch, skip_env_check) {
        Ok(r) => r,
        Err(e) if e.code == "GITHUB_APP_NOT_INSTALLED" => {
            // App not installed — run browser install flow or exit in JSON mode
            let install_url = e
                .extra
                .as_ref()
                .and_then(|v| v.get("install_url"))
                .and_then(|v| v.as_str())
                .unwrap_or("https://github.com/apps/getfloo/installations/new");

            if output::is_json_mode() {
                output::error_with_data(
                    &e.message,
                    &ErrorCode::from_api(&e.code),
                    None,
                    Some(serde_json::json!({
                        "error": "github_app_not_installed",
                        "install_url": install_url,
                    })),
                );
                process::exit(1);
            }

            // Interactive: run browser install flow
            let owner = repo.split('/').next().unwrap_or(repo);
            output::warn(&format!("Floo GitHub App not installed on \"{owner}\""));

            run_installation_flow(&client, install_url);

            // Retry connect — API will auto-resolve the newly installed ID
            match client.github_connect(&app_id, repo, branch, skip_env_check) {
                Ok(r) => r,
                Err(e2) => {
                    output::error(&e2.message, &ErrorCode::from_api(&e2.code), None);
                    process::exit(1);
                }
            }
        }
        Err(e) if e.code == "GITHUB_REPO_NOT_IN_INSTALLATION" => {
            // App installed on org but repo not in scope — open settings
            let settings_url = e
                .extra
                .as_ref()
                .and_then(|v| v.get("settings_url"))
                .and_then(|v| v.as_str());

            if output::is_json_mode() {
                output::error_with_data(
                    &e.message,
                    &ErrorCode::from_api(&e.code),
                    None,
                    Some(serde_json::json!({
                        "error": "github_repo_not_in_installation",
                        "settings_url": settings_url,
                    })),
                );
                process::exit(1);
            }

            let owner = repo.split('/').next().unwrap_or(repo);
            output::warn(&format!(
                "Floo GitHub App is installed on \"{owner}\" but does not have access to \"{repo}\"."
            ));

            if let Some(url) = settings_url {
                output::info(
                    "Opening installation settings to add this repository...",
                    None,
                );
                if let Err(e) = open::that(url) {
                    output::warn(&format!("Could not open browser: {e}"));
                    output::warn(&format!("Open this URL manually: {url}"));
                }
            } else {
                output::warn(&format!(
                    "Visit https://github.com/organizations/{owner}/settings/installations to manage repository access."
                ));
            }

            output::info("After granting access, re-run this command.", None);
            process::exit(1);
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

    let connected_branch = result.default_branch.as_deref().unwrap_or("main");

    output::success(
        &format!("Connected {name} to {repo} (branch: {connected_branch})"),
        Some(output::to_value(&result)),
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
                output::info(&format!("{name} GitHub connection"), None);
                output::info(&format!("  Repo:      {}", conn.repo_full_name), None);
                output::info(&format!("  Branch:    {}", conn.default_branch), None);
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

    let spinner = output::Spinner::new("Deploying...");
    let mut deploy_data = match client.create_deploy(
        app_id,
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
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let initial_status = deploy_data.status.as_deref().unwrap_or("");

    if !super::deploy::TERMINAL_STATUSES.contains(&initial_status) {
        if deploy_data.id.is_empty() {
            output::error(
                "Unexpected API response: deploy is missing required 'id'.",
                &ErrorCode::InvalidResponse,
                Some("This may indicate a CLI/API mismatch. Check for updates with `floo update`."),
            );
            process::exit(1);
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
        output::error_with_data(
            "Deploy failed.",
            &ErrorCode::DeployFailed,
            Some("Check build output above, or run `floo logs` for details."),
            Some(output::to_value(&deploy_data)),
        );
        process::exit(1);
    }

    let url = deploy_data.url.as_deref().unwrap_or("");

    output::success(
        &format!("Deployed to {url}"),
        Some(output::to_value(&deploy_data)),
    );
}
