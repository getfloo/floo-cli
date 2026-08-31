use std::path::Path;
use std::process;
use std::time::{Duration, Instant};

use crate::api_types::{GitHubInstallationCandidate, GitHubSetupStatus};
use crate::deploy_status::{self, Terminal};
use crate::detection::detect;
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;
use crate::project_config;

const GITHUB_WAIT_TIMEOUT: Duration = Duration::from_secs(900);
const INSTALLATION_POLL_INTERVAL: Duration = Duration::from_secs(3);
const REPO_ACCESS_POLL_INTERVAL: Duration = Duration::from_secs(2);
const APPROVAL_HINT_AFTER: Duration = Duration::from_secs(60);
const DEFAULT_INSTALL_URL: &str = "https://github.com/apps/getfloo/installations/new";

/// An app **this invocation created**, eligible to be undone if connect never
/// links it to GitHub.
///
/// Only apps created here are tracked. An app the user already had is never
/// removed: it can hold services, env vars, deploys, and team access that this
/// command did not create and has no business destroying.
struct CreatedApp {
    id: String,
    name: String,
}

/// A failure that has not been reported to the user yet.
///
/// Helpers return this instead of calling `process::exit` directly so the
/// command can undo the app it created *before* the error is emitted.
/// Reporting first would either lose the cleanup entirely (exit never
/// returns) or print a second JSON object after the error, breaking the
/// one-object stdout contract agents parse.
struct ConnectFailure {
    message: String,
    code: ErrorCode,
    suggestion: Option<String>,
}

impl ConnectFailure {
    fn new(message: impl Into<String>, code: ErrorCode, suggestion: Option<String>) -> Self {
        Self {
            message: message.into(),
            code,
            suggestion,
        }
    }

    fn from_api(e: &FlooApiError, suggestion: Option<String>) -> Self {
        Self::new(e.message.clone(), ErrorCode::from_api(&e.code), suggestion)
    }
}

/// Where a human grants the floo GitHub App access to one repository.
///
/// GitHub scopes an installation to an *account*, so the page that grants
/// `owner/repo` is the installation on `owner` — never one on some other
/// account the operator happens to have. Personal accounts and organizations
/// keep that page at different paths, and sending someone to the wrong one
/// ends the instruction at a 404.
fn installation_settings_url(owner: &str, installation_id: Option<i64>) -> String {
    match installation_id {
        Some(id) => format!("https://github.com/settings/installations/{id}"),
        None => format!("https://github.com/apps/getfloo/installations/new/permissions?suggested_target_id={owner}"),
    }
}

/// The single explanation of how to connect the floo GitHub App to a repo.
///
/// Every message names four things in order: what is wrong, the exact URL that
/// fixes it, what to do on that page — the account **and** the repository —
/// and the command to re-run. An agent driving this CLI has no browser and no
/// prior context, so a message that omits any of the four leaves it guessing.
/// Omitting the account is what turned a correct "not installed" report into
/// four identical retries: the App was installed, on a different account, and
/// nothing said that installing elsewhere grants nothing here.
fn grant_access_instructions(
    repo: &str,
    owner: &str,
    grant_url: &str,
    rerun_command: &str,
    installed_elsewhere: bool,
) -> String {
    let mut steps = String::new();
    steps.push_str(&format!(
        "The floo GitHub App must be installed on the account \"{owner}\" and granted access to \"{repo}\".\n"
    ));
    if installed_elsewhere {
        steps.push_str(&format!(
            "Installing it on any other account or organization does NOT grant access to \"{repo}\" \
             — GitHub scopes an installation to one account, and \"{repo}\" belongs to \"{owner}\".\n"
        ));
    }
    steps.push_str(
        "A human must do this in a browser; it cannot be done from the CLI or by an agent.\n",
    );
    steps.push_str(&format!("  1. Open: {grant_url}\n"));
    steps.push_str(&format!(
        "  2. Choose the account \"{owner}\" (not a different organization).\n"
    ));
    steps.push_str(&format!(
        "  3. Under \"Repository access\", select \"{repo}\". If \"Only select repositories\" \
         is set, \"{repo}\" must appear in that list.\n"
    ));
    steps.push_str("  4. Save / Install.\n");
    steps.push_str(&format!("  5. Re-run: {rerun_command}"));
    steps
}

/// Undo an app this run created, then report `failure` and exit non-zero.
///
/// A connect that never linked the repo used to leave the app behind, so the
/// operator had to delete it by hand before retrying: the second attempt hit
/// APP_NAME_TAKEN on creation and stopped. Cleaning up here makes a failed
/// connect retryable as-is.
fn abort_connect(
    client: &crate::api_client::FlooClient,
    created_app: Option<&CreatedApp>,
    failure: ConnectFailure,
) -> ! {
    let mut suggestion = failure.suggestion;

    if let Some(created) = created_app {
        match client.delete_app(&created.id) {
            Ok(()) => {
                if !output::is_json_mode() {
                    output::info(
                        &format!(
                            "Removed the app \"{}\" floo created for this attempt.",
                            created.name
                        ),
                        None,
                    );
                }
            }
            Err(e) => {
                // Folded into the suggestion rather than printed separately:
                // in JSON mode the error below is the only object on stdout,
                // and a stranded app the operator does not know about is worse
                // than a long suggestion.
                let note = format!(
                    "floo also could not remove the app \"{}\" it created for this attempt ({}). \
                     Delete it before retrying: floo apps delete {}",
                    created.name, e.message, created.name
                );
                if !output::is_json_mode() {
                    output::warn(&note);
                }
                suggestion = Some(match suggestion {
                    Some(existing) => format!("{existing}\n{note}"),
                    None => note,
                });
            }
        }
    }

    output::error(&failure.message, &failure.code, suggestion.as_deref());
    process::exit(1);
}

pub fn connect(
    repo: &str,
    app: Option<&str>,
    branch: Option<&str>,
    skip_env_check: bool,
    no_deploy: bool,
    no_browser: bool,
) {
    super::require_auth();
    let client = super::init_client(None);

    // In JSON mode, never open a browser — agents can't use one.
    let no_browser = no_browser || output::is_json_mode();
    let rerun_command =
        connect_rerun_command(repo, app, branch, skip_env_check, no_deploy, no_browser);

    let cwd = super::read_cwd_or_exit();

    // Apps this run creates are undone if connect never links the repo.
    let mut created_app: Option<CreatedApp> = None;

    // Resolve app — skip config file reads when --app is provided
    let (app_data, resolved) = if let Some(app_flag) = app {
        // --app provided: look up directly, no local config needed
        let app_data = match crate::resolve::resolve_app(&client, app_flag) {
            Ok(a) => a,
            // 404 == app doesn't exist yet → create it (status, not code string).
            Err(e) if e.is_not_found() => {
                // Create new app — try local detection for runtime, fall back to "unknown"
                let runtime = detect(&cwd).runtime;
                let spinner = output::Spinner::new(&format!("Creating app {app_flag}..."));
                match client.create_app(app_flag, Some(&runtime)) {
                    Ok(a) => {
                        spinner.finish();
                        created_app = Some(CreatedApp {
                            id: a.id.clone(),
                            name: a.name.clone(),
                        });
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
        let resolved = project_config::resolve_app_context(&cwd, Some(&app_data.name)).ok();
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
            // 404 == app doesn't exist yet → create it (status, not code string).
            Err(e) if e.is_not_found() => {
                let detection = detect(&cwd);
                let spinner = output::Spinner::new(&format!("Creating app {app_name}..."));
                match client.create_app(&app_name, Some(&detection.runtime)) {
                    Ok(a) => {
                        spinner.finish();
                        created_app = Some(CreatedApp {
                            id: a.id.clone(),
                            name: a.name.clone(),
                        });
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

        let resolved = project_config::resolve_app_context(&cwd, Some(&app_data.name)).ok();
        (app_data, resolved)
    };

    let app_id = app_data.id.clone();
    let name = app_data.name.clone();

    // Phase 1: Import env vars from local env_file before connecting
    if let Some(ref r) = resolved {
        if let Err(failure) = import_env_vars_for_connect(&client, &app_id, r) {
            abort_connect(&client, created_app.as_ref(), failure);
        }
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
                .unwrap_or(DEFAULT_INSTALL_URL);

            let owner = repo.split('/').next().unwrap_or(repo);

            if no_browser {
                // --no-browser cannot complete the grant, so the error IS the
                // deliverable: it must be a complete instruction a human can
                // follow without re-deriving anything from the CLI's state.
                let setup_url = manual_setup_url(&client, install_url);
                abort_connect(
                    &client,
                    created_app.as_ref(),
                    ConnectFailure::new(
                        format!(
                            "The floo GitHub App is not installed on the GitHub account \"{owner}\", \
                             so floo cannot read \"{repo}\"."
                        ),
                        ErrorCode::from_api("GITHUB_APP_NOT_INSTALLED"),
                        Some(grant_access_instructions(
                            repo,
                            owner,
                            &setup_url,
                            &rerun_command,
                            true,
                        )),
                    ),
                );
            }

            if !output::is_json_mode() {
                output::warn(&format!(
                    "The floo GitHub App is not installed on the GitHub account \"{owner}\"."
                ));
                output::info(
                    &grant_access_instructions(repo, owner, install_url, &rerun_command, true),
                    None,
                );
            }

            if let Err(failure) =
                run_installation_flow(&client, install_url, &rerun_command, Some(owner))
            {
                abort_connect(&client, created_app.as_ref(), failure);
            }

            match client.github_connect(&app_id, repo, branch, skip_env_check) {
                Ok(r) => r,
                Err(e2) => abort_connect(
                    &client,
                    created_app.as_ref(),
                    ConnectFailure::from_api(&e2, None),
                ),
            }
        }
        Err(e) if e.code == "GITHUB_REPO_NOT_IN_INSTALLATION" => {
            let install_url = e
                .extra
                .as_ref()
                .and_then(|v| v.get("install_url"))
                .and_then(|v| v.as_str())
                .unwrap_or(DEFAULT_INSTALL_URL);
            let settings_url = e
                .extra
                .as_ref()
                .and_then(|v| v.get("settings_url"))
                .and_then(|v| v.as_str());

            let owner = repo.split('/').next().unwrap_or(repo);
            // The API sends an account-correct settings_url. The fallback is
            // only for an API too old to send one, and must not assume an
            // organization: the /organizations/ path 404s for a personal
            // account, ending the instruction at a broken link.
            let fallback_url = installation_settings_url(owner, None);
            let url = settings_url.unwrap_or(&fallback_url);

            if no_browser {
                abort_connect(
                    &client,
                    created_app.as_ref(),
                    ConnectFailure::new(
                        format!(
                            "The floo GitHub App is installed on \"{owner}\" but does not have \
                             access to \"{repo}\"."
                        ),
                        ErrorCode::from_api("GITHUB_REPO_NOT_IN_INSTALLATION"),
                        Some(grant_access_instructions(
                            repo,
                            owner,
                            url,
                            &rerun_command,
                            false,
                        )),
                    ),
                );
            }

            if !output::is_json_mode() {
                output::warn(&format!(
                    "The floo GitHub App is installed on \"{owner}\" but does not have access to \"{repo}\"."
                ));
                output::info(
                    &grant_access_instructions(repo, owner, url, &rerun_command, false),
                    None,
                );
            }

            if let Err(failure) =
                run_installation_flow(&client, install_url, &rerun_command, Some(owner))
            {
                abort_connect(&client, created_app.as_ref(), failure);
            }
            if let Err(failure) = poll_repo_access(&client, repo, url, &rerun_command) {
                abort_connect(&client, created_app.as_ref(), failure);
            }

            // Repo is now accessible — connect
            match client.github_connect(&app_id, repo, branch, skip_env_check) {
                Ok(r) => r,
                Err(e2) => abort_connect(
                    &client,
                    created_app.as_ref(),
                    ConnectFailure::from_api(&e2, None),
                ),
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
                // Installed on GitHub, unlinked in floo. Without this the error
                // named no command at all and the only exit anyone found was
                // uninstalling the App (getfloo/floo#2189).
                "GITHUB_INSTALLATION_NOT_AUTHORIZED" => Some(
                    "The App is installed on GitHub but not linked to this floo org. \
                     Link it: floo apps github setup",
                ),
                _ => None,
            };
            abort_connect(
                &client,
                created_app.as_ref(),
                ConnectFailure::from_api(&e, suggestion.map(str::to_string)),
            );
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
        DeployOutcome::Superseded { deploy } => {
            output::success(
                &format!("Connected {name} to {repo} — deploy superseded by a newer deploy."),
                Some(serde_json::json!({
                    "connected": true,
                    "app": name,
                    "repo": repo,
                    "branch": connected_branch,
                    "deployed": false,
                    "deploy_status": "superseded",
                    "deploy": deploy,
                })),
            );
        }
        DeployOutcome::Cancelled { deploy } => {
            output::success(
                &format!(
                    "Connected {name} to {repo}, but the deploy was cancelled \
                     (its target environment was removed before it ran)."
                ),
                Some(serde_json::json!({
                    "connected": true,
                    "app": name,
                    "repo": repo,
                    "branch": connected_branch,
                    "deployed": false,
                    "deploy_status": "cancelled",
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

/// Mint a fresh GitHub App setup link regardless of install state.
///
/// `connect` mints one only on the `GITHUB_APP_NOT_INSTALLED` and
/// `GITHUB_REPO_NOT_IN_INSTALLATION` branches. Once the App IS installed but
/// floo holds no binding for the org, every connect returns
/// `GITHUB_INSTALLATION_NOT_AUTHORIZED` and nothing re-mints the handshake —
/// the dead end in getfloo/floo#2189, whose only exit was uninstalling the App
/// from the GitHub org. This command is that exit.
pub fn setup(no_browser: bool) {
    super::require_auth();
    let client = super::init_client(None);

    // In JSON mode, never open a browser — agents can't use one.
    let no_browser = no_browser || output::is_json_mode();

    if no_browser {
        // Minted here only on this branch: run_installation_flow begins its own
        // session, so hoisting this above the `if` would burn two setup tokens
        // per interactive run and leave the first orphaned in Redis.
        let setup_url = match begin_setup(&client, DEFAULT_INSTALL_URL) {
            Ok(url) => url,
            Err(failure) => {
                // No app is created on this path, so there is nothing to undo.
                abort_connect(&client, None, failure);
            }
        };
        // The URL goes in the message, not only the JSON payload: human mode
        // prints the message and drops the data, so a payload-only link is an
        // instruction to open nothing.
        output::success(
            &format!(
                "Open this link to link the floo GitHub App to your org:\n  {setup_url}\n\n\
                 Then run: floo apps github connect <owner>/<repo>"
            ),
            Some(serde_json::json!({
                "setup_url": setup_url,
                "next": "floo apps github connect <owner>/<repo>",
            })),
        );
        return;
    }

    // `setup` creates no app, so a failure here has nothing to undo — but it
    // must still abort. Falling through would report a link that never landed.
    if let Err(failure) =
        run_installation_flow(&client, DEFAULT_INSTALL_URL, "floo apps github setup", None)
    {
        abort_connect(&client, None, failure);
    }

    output::success(
        "Linked the floo GitHub App to your org.",
        Some(serde_json::json!({
            "linked": true,
            "next": "floo apps github connect <owner>/<repo>",
        })),
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
fn poll_repo_access(
    client: &crate::api_client::FlooClient,
    repo: &str,
    settings_url: &str,
    rerun_command: &str,
) -> Result<(), ConnectFailure> {
    let mut spinner = output::Spinner::new(&format!(
        "Waiting for repo access (grant at {settings_url})..."
    ));
    let start = Instant::now();
    let mut approval_hint_shown = false;

    loop {
        std::thread::sleep(REPO_ACCESS_POLL_INTERVAL);

        if !approval_hint_shown && start.elapsed() >= APPROVAL_HINT_AFTER {
            spinner.finish();
            if !output::is_json_mode() {
                output::info(
                    "Still waiting for repo access. GitHub may still be applying the installation's repository permissions.",
                    None,
                );
            }
            spinner = output::Spinner::new("Waiting for repo access...");
            approval_hint_shown = true;
        }

        if start.elapsed() > GITHUB_WAIT_TIMEOUT {
            spinner.finish();
            return Err(ConnectFailure::new(
                "Timed out waiting for repository access.",
                ErrorCode::Other("REPO_ACCESS_TIMEOUT".into()),
                Some(repo_access_timeout_suggestion(settings_url, rerun_command)),
            ));
        }

        match client.github_check_repo_access(repo) {
            Ok(resp) => {
                if resp.get("accessible").and_then(|v| v.as_bool()) == Some(true) {
                    spinner.finish();
                    return Ok(());
                }
            }
            Err(e) => {
                let is_transient = e.status_code == 0 || e.status_code >= 500;
                if !is_transient {
                    spinner.finish();
                    return Err(ConnectFailure::from_api(&e, None));
                }
            }
        }
    }
}

/// Bind the one installation that matches the repository's owner.
///
/// `awaiting_selection` exists because binding every reachable installation
/// would grant the org repositories nobody asked for. That ambiguity is real
/// for a bare `github setup`, but not for `github connect owner/repo`: the
/// owner names the account, and exactly one candidate can serve it. When no
/// candidate matches, the operator has installed the App somewhere that cannot
/// reach this repo — which is the single most common way this flow fails, and
/// is worth saying in full rather than reporting as ambiguity.
fn select_installation(
    client: &crate::api_client::FlooClient,
    candidates: Vec<GitHubInstallationCandidate>,
    repo_owner: Option<&str>,
    rerun_command: &str,
) -> Result<(), ConnectFailure> {
    let known: Vec<String> = candidates
        .iter()
        .filter_map(|c| c.owner_login.clone())
        .collect();
    let known_list = if known.is_empty() {
        "(none reported)".to_string()
    } else {
        known.join(", ")
    };

    let Some(owner) = repo_owner else {
        return Err(ConnectFailure::new(
            "Your GitHub account can reach more than one floo App installation, so floo cannot \
             tell which one to use.",
            ErrorCode::Other("GITHUB_INSTALLATION_AMBIGUOUS".into()),
            Some(format!(
                "Installations available: {known_list}.\n\
                 Re-run this against the repository you want so floo can pick the matching \
                 installation: floo apps github connect <owner>/<repo>"
            )),
        ));
    };

    let matched = candidates.iter().find(|c| {
        c.owner_login
            .as_deref()
            .is_some_and(|login| login.eq_ignore_ascii_case(owner))
    });

    let Some(candidate) = matched else {
        return Err(ConnectFailure::new(
            format!(
                "The floo GitHub App is not installed on the GitHub account \"{owner}\". \
                 It is installed on: {known_list}."
            ),
            ErrorCode::from_api("GITHUB_APP_NOT_INSTALLED"),
            Some(format!(
                "An installation on another account grants floo nothing for a repository owned \
                 by \"{owner}\". Install the floo GitHub App on \"{owner}\" itself: {}\n\
                 Then re-run: {rerun_command}",
                installation_settings_url(owner, None)
            )),
        ));
    };

    if !output::is_json_mode() {
        output::info(
            &format!(
                "Using the floo GitHub App installation on \"{}\".",
                candidate.owner_login.as_deref().unwrap_or(owner)
            ),
            None,
        );
    }

    client
        .github_setup_select(candidate.installation_id)
        .map(|_| ())
        .map_err(|e| {
            ConnectFailure::from_api(&e, Some(format!("Re-run once resolved: {rerun_command}")))
        })
}

fn run_installation_flow(
    client: &crate::api_client::FlooClient,
    install_url: &str,
    rerun_command: &str,
    repo_owner: Option<&str>,
) -> Result<(), ConnectFailure> {
    // Begin the setup session (stores pending state in Redis)
    let setup_url = begin_setup(client, install_url)?;

    // Open browser for installation
    if !output::is_json_mode() {
        output::info("Opening browser to install...", None);
    }
    if let Err(e) = open::that(&setup_url) {
        output::warn(&format!("Could not open browser: {e}"));
        output::warn(&format!("Open this URL manually: {setup_url}"));
    }

    let mut spinner = output::Spinner::new("Waiting for GitHub installation...");
    let start = Instant::now();
    let mut approval_notice_shown = false;

    loop {
        std::thread::sleep(INSTALLATION_POLL_INTERVAL);

        if start.elapsed() > GITHUB_WAIT_TIMEOUT {
            spinner.finish();
            return Err(ConnectFailure::new(
                "Timed out waiting for GitHub App installation.",
                ErrorCode::Other("SETUP_TIMEOUT".into()),
                Some(setup_timeout_suggestion(rerun_command)),
            ));
        }

        match client.github_setup_poll() {
            Ok(resp) => match resp.status {
                GitHubSetupStatus::Ready => {
                    spinner.finish();
                    if resp.installation_id.is_none() {
                        return Err(ConnectFailure::new(
                            "GitHub App was installed but the server did not return an installation ID.",
                            ErrorCode::InvalidResponse,
                            Some(
                                "Try running the command again. The installation should be detected automatically."
                                    .to_string(),
                            ),
                        ));
                    }
                    return Ok(());
                }
                GitHubSetupStatus::AwaitingOrgApproval => {
                    if !approval_notice_shown {
                        spinner.finish();
                        if !output::is_json_mode() {
                            output::info(
                                "GitHub recorded the install request. Waiting for org admin approval.",
                                None,
                            );
                        }
                        spinner = output::Spinner::new(setup_spinner_message(
                            &GitHubSetupStatus::AwaitingOrgApproval,
                        ));
                        approval_notice_shown = true;
                    }
                }
                GitHubSetupStatus::AwaitingSelection => {
                    spinner.finish();
                    // The authorizing identity can reach several installations,
                    // so the API refuses to guess. When this flow was started
                    // for a specific repo we are not guessing: that repo's
                    // owner names exactly one installation, and picking it is
                    // the answer to the question the API asked.
                    match select_installation(client, resp.candidates, repo_owner, rerun_command) {
                        Ok(()) => {
                            spinner = output::Spinner::new("Waiting for GitHub installation...");
                        }
                        Err(failure) => return Err(failure),
                    }
                }
                GitHubSetupStatus::AwaitingInstallation => {}
                GitHubSetupStatus::None => {
                    spinner.finish();
                    return Err(ConnectFailure::new(
                        "GitHub setup session disappeared while waiting for installation.",
                        ErrorCode::Other("SETUP_TIMEOUT".into()),
                        Some(setup_session_lost_suggestion(rerun_command)),
                    ));
                }
                GitHubSetupStatus::Unknown => {
                    spinner.finish();
                    return Err(ConnectFailure::new(
                        "This floo CLI does not understand the GitHub setup state the API reported.",
                        ErrorCode::InvalidResponse,
                        Some(
                            "The API has a setup state this CLI predates. Update the CLI and \
                             re-run: floo update"
                                .to_string(),
                        ),
                    ));
                }
            },
            Err(e) => {
                // Only tolerate transient failures (network issues, 5xx).
                // Permanent errors (4xx) should abort immediately.
                let is_transient = e.status_code == 0 || e.status_code >= 500;
                if !is_transient {
                    spinner.finish();
                    return Err(ConnectFailure::new(
                        format!("Poll failed: {}", e.message),
                        ErrorCode::from_api(&e.code),
                        None,
                    ));
                }
            }
        }
    }
}

fn import_env_vars_for_connect(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    resolved: &project_config::ResolvedApp,
) -> Result<(), ConnectFailure> {
    super::deploy::sync_env_vars_if_needed(client, app_id, resolved, true)
        .map_err(|message| ConnectFailure::new(message, ErrorCode::InvalidPath, None))
}

fn setup_spinner_message(status: &GitHubSetupStatus) -> &'static str {
    match status {
        GitHubSetupStatus::AwaitingOrgApproval => "Waiting for org admin approval...",
        GitHubSetupStatus::AwaitingInstallation => "Waiting for GitHub installation...",
        GitHubSetupStatus::Ready => "GitHub installation ready.",
        GitHubSetupStatus::None => "GitHub setup session missing.",
        GitHubSetupStatus::AwaitingSelection => "Selecting the GitHub App installation...",
        GitHubSetupStatus::Unknown => "Waiting for GitHub installation...",
    }
}

fn connect_rerun_command(
    repo: &str,
    app: Option<&str>,
    branch: Option<&str>,
    skip_env_check: bool,
    no_deploy: bool,
    no_browser: bool,
) -> String {
    let mut parts = vec!["floo".to_string()];
    if output::is_json_mode() {
        parts.push("--json".to_string());
    }
    parts.extend([
        "apps".to_string(),
        "github".to_string(),
        "connect".to_string(),
        repo.to_string(),
    ]);
    if let Some(app_name) = app {
        parts.push("--app".to_string());
        parts.push(app_name.to_string());
    }
    if let Some(branch_name) = branch {
        parts.push("--branch".to_string());
        parts.push(branch_name.to_string());
    }
    if skip_env_check {
        parts.push("--skip-env-check".to_string());
    }
    if no_deploy {
        parts.push("--no-deploy".to_string());
    }
    if no_browser {
        parts.push("--no-browser".to_string());
    }
    parts.join(" ")
}

fn setup_timeout_suggestion(rerun_command: &str) -> String {
    format!(
        "If your GitHub org needs approval, leave the request in place and re-run after it lands: \
         {rerun_command}"
    )
}

fn setup_session_lost_suggestion(rerun_command: &str) -> String {
    format!("Start the GitHub setup flow again: {rerun_command}")
}

fn repo_access_timeout_suggestion(settings_url: &str, rerun_command: &str) -> String {
    format!(
        "Grant access at {settings_url}. If your org needs approval, re-run after it lands: \
         {rerun_command}"
    )
}

fn begin_setup(
    client: &crate::api_client::FlooClient,
    fallback_url: &str,
) -> Result<String, ConnectFailure> {
    match client.github_setup_begin() {
        Ok(resp) => Ok(resp
            .get("install_url")
            .and_then(|v| v.as_str())
            .unwrap_or(fallback_url)
            .to_string()),
        Err(e) => handle_setup_begin_error(client, e, fallback_url),
    }
}

fn handle_setup_begin_error(
    client: &crate::api_client::FlooClient,
    e: FlooApiError,
    fallback_url: &str,
) -> Result<String, ConnectFailure> {
    let is_transient = e.status_code == 0 || e.status_code >= 500;
    // Permanent errors (4xx) mean the flow is doomed — abort early.
    if !is_transient {
        return Err(ConnectFailure::new(
            format!("Failed to start setup session: {}", e.message),
            ErrorCode::from_api(&e.code),
            Some("Check your authentication with: floo auth whoami".to_string()),
        ));
    }

    let setup_session_exists = matches!(
        client.github_setup_poll(),
        Ok(resp) if resp.status != GitHubSetupStatus::None
    );
    if !setup_session_exists {
        return Err(ConnectFailure::new(
            format!("Failed to start setup session: {}", e.message),
            ErrorCode::from_api(&e.code),
            Some(
                "Retry the command. If this keeps happening, check API health with `floo auth whoami`."
                    .to_string(),
            ),
        ));
    }

    output::warn(&format!(
        "Setup session start was inconclusive: {}. Continuing because an active setup session still exists...",
        e.message
    ));
    Ok(fallback_url.to_string())
}

fn manual_setup_url(client: &crate::api_client::FlooClient, fallback_url: &str) -> String {
    match client.github_setup_begin() {
        Ok(resp) => resp
            .get("install_url")
            .and_then(|v| v.as_str())
            .unwrap_or(fallback_url)
            .to_string(),
        Err(_) => fallback_url.to_string(),
    }
}

enum DeployOutcome {
    Live {
        url: String,
        deploy: serde_json::Value,
    },
    Superseded {
        deploy: serde_json::Value,
    },
    Cancelled {
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
        None,  // API discovers services from GitHub tarball
        None,  // access_mode
        None,  // agent_mode
        None,  // auth_redirect_uris
        None,  // cron_jobs
        None,  // github_config
        false, // skip_migrations — initial deploy from `floo apps github connect` always runs migrations
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

    if !deploy_status::is_terminal(initial_status) {
        if deploy_data.id.is_empty() {
            return DeployOutcome::Failed {
                deploy: serde_json::json!({"error": "Deploy missing 'id' in response"}),
            };
        }
        let deploy_id = deploy_data.id.clone();

        if !output::is_json_mode() {
            match super::deploy::stream_deploy(client, app_id, &deploy_id) {
                // Unsettled, this site INVERTED the #208 race: classify(None)
                // on a raced "deploying" reported a healthy first deploy as
                // FAILED with a  hint — the onboarding surface.
                Ok(d) => deploy_data = super::deploy::settle_to_terminal(client, app_id, d),
                Err(_) => deploy_data = super::deploy::poll_deploy(client, app_id, &deploy_data),
            }
        } else {
            // stream_deploy_json settles internally before emitting done (#208).
            match super::deploy::stream_deploy_json(client, app_id, &deploy_id) {
                Ok(d) => deploy_data = d,
                Err(_) => deploy_data = super::deploy::poll_deploy(client, app_id, &deploy_data),
            }
        }
    }

    let final_status = deploy_data.status.as_deref().unwrap_or("");

    // Matching on `classify` makes a newly-added terminal status a compile error
    // here instead of silently falling into the `None` → failed arm.
    match deploy_status::classify(final_status) {
        Some(Terminal::Failed) => DeployOutcome::Failed {
            deploy: output::to_value(&deploy_data),
        },
        Some(Terminal::Live) => {
            let url = deploy_data.url.as_deref().unwrap_or("").to_string();
            DeployOutcome::Live {
                url,
                deploy: output::to_value(&deploy_data),
            }
        }
        Some(Terminal::Superseded) => DeployOutcome::Superseded {
            deploy: output::to_value(&deploy_data),
        },
        Some(Terminal::Cancelled) => {
            // Target env torn down before the deploy ran (getfloo/floo#1354) — a moot
            // terminal like superseded, NOT a failure. Must not exit 1 / say "retry".
            DeployOutcome::Cancelled {
                deploy: output::to_value(&deploy_data),
            }
        }
        None => {
            // Ambiguous status (timeout, unknown) — report as failed.
            output::warn(&format!(
                "Deploy ended with unexpected status: {}",
                final_status
            ));
            DeployOutcome::Failed {
                deploy: output::to_value(&deploy_data),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        connect_rerun_command, grant_access_instructions, installation_settings_url,
        repo_access_timeout_suggestion, setup_session_lost_suggestion, setup_spinner_message,
        setup_timeout_suggestion,
    };
    use crate::api_types::{GitHubSetupPollResponse, GitHubSetupStatus};

    #[test]
    fn test_setup_spinner_message_for_org_approval() {
        assert_eq!(
            setup_spinner_message(&GitHubSetupStatus::AwaitingOrgApproval),
            "Waiting for org admin approval..."
        );
    }

    #[test]
    fn test_setup_timeout_suggestion_mentions_repo_and_approval() {
        let suggestion = setup_timeout_suggestion("floo apps github connect getfloo/example");
        assert!(suggestion.contains("getfloo/example"));
        assert!(suggestion.contains("approval"));
    }

    #[test]
    fn test_setup_session_lost_suggestion_points_back_to_connect() {
        let suggestion = setup_session_lost_suggestion("floo apps github connect getfloo/example");
        assert!(suggestion.contains("floo apps github connect getfloo/example"));
    }

    #[test]
    fn test_repo_access_timeout_suggestion_mentions_settings_url() {
        let suggestion = repo_access_timeout_suggestion(
            "https://github.com/organizations/getfloo/settings/installations",
            "floo apps github connect getfloo/example",
        );
        assert!(suggestion.contains("settings/installations"));
        assert!(suggestion.contains("approval"));
    }

    #[test]
    fn test_grant_instructions_name_account_repo_url_and_rerun() {
        // The four things an agent needs and cannot infer. If any one is
        // missing the message is a status report, not an instruction.
        let steps = grant_access_instructions(
            "pdonohoe02/galleon",
            "pdonohoe02",
            "https://github.com/settings/installations/777",
            "floo apps github connect pdonohoe02/galleon",
            false,
        );
        assert!(steps.contains("pdonohoe02/galleon"));
        assert!(steps.contains("\"pdonohoe02\""));
        assert!(steps.contains("https://github.com/settings/installations/777"));
        assert!(steps.contains("floo apps github connect pdonohoe02/galleon"));
        assert!(steps.contains("Repository access"));
        assert!(steps.contains("human"));
    }

    #[test]
    fn test_grant_instructions_warn_that_another_account_grants_nothing() {
        // The exact trap: the App was installed, on a different account, and
        // nothing said that installing elsewhere grants nothing here.
        let steps = grant_access_instructions(
            "pdonohoe02/galleon",
            "pdonohoe02",
            "https://github.com/apps/getfloo/installations/new",
            "floo apps github connect pdonohoe02/galleon",
            true,
        );
        assert!(steps.contains("does NOT grant access"));
        assert!(steps.contains("not a different organization"));
    }

    #[test]
    fn test_installation_settings_url_is_not_org_shaped_for_a_personal_account() {
        // The /organizations/ path 404s for a personal account, which ends the
        // instruction at a broken link.
        let url = installation_settings_url("pdonohoe02", Some(777));
        assert_eq!(url, "https://github.com/settings/installations/777");
        assert!(!installation_settings_url("pdonohoe02", None).contains("/organizations/"));
    }

    #[test]
    fn test_awaiting_selection_is_a_known_status_not_a_parse_error() {
        // Regression: the API has returned `awaiting_selection` since the
        // multi-installation work. A CLI enum without it failed the whole
        // response at HTTP 200 and surfaced "error decoding response body",
        // which no operator could act on.
        let resp: GitHubSetupPollResponse = serde_json::from_str(
            r#"{"status":"awaiting_selection","installation_id":null,
                "candidates":[{"installation_id":1,"owner_login":"pdonohoe02"}]}"#,
        )
        .expect("awaiting_selection must deserialize");
        assert_eq!(resp.status, GitHubSetupStatus::AwaitingSelection);
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(
            resp.candidates[0].owner_login.as_deref(),
            Some("pdonohoe02")
        );
    }

    #[test]
    fn test_unknown_status_degrades_instead_of_failing_the_response() {
        // A future API state must not reproduce the same undiagnosable crash.
        let resp: GitHubSetupPollResponse =
            serde_json::from_str(r#"{"status":"some_future_state","installation_id":null}"#)
                .expect("an unknown status must still deserialize");
        assert_eq!(resp.status, GitHubSetupStatus::Unknown);
        assert!(resp.candidates.is_empty());
    }

    #[test]
    fn test_connect_rerun_command_preserves_flags() {
        let command = connect_rerun_command(
            "getfloo/example",
            Some("demo-app"),
            Some("release"),
            true,
            true,
            true,
        );
        assert_eq!(
            command,
            "floo apps github connect getfloo/example --app demo-app --branch release --skip-env-check --no-deploy --no-browser"
        );
    }
}
