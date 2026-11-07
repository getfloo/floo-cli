//! `floo status`: is this folder's commit live on dev, and if not, why
//! (`GET /v1/apps/{app_id}/status`). One read answers what `deploys status`,
//! `doctor push` and `apps github status` each answer in part, and the API
//! writes the summary and the next step so every surface says the same thing.

use std::process::{self, Command, Stdio};

use serde_json::Value;

use crate::errors::ErrorCode;
use crate::output;

pub fn status(app: Option<&str>, commit: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _) = super::resolve_app_from_config(&client, app);
    // With no --app, the folder is the app's, so its HEAD is the commit to ask about.
    let commit = match commit {
        Some(commit) => Some(full_sha(commit)),
        None if app.is_none() => git(&["rev-parse", "--verify", "HEAD"]),
        None => None,
    };

    let status = client
        .get_app_status(&app_id, commit.as_deref())
        .unwrap_or_else(|error| {
            output::error(&error.message, &ErrorCode::from_api(&error.code), None);
            process::exit(1);
        });
    let summary = text(&status, "summary").unwrap_or("No status.").to_string();
    if output::is_json_mode() {
        output::success(&summary, Some(status));
        return;
    }
    output::info(&summary, None);
    if let Some(next) = text(&status, "next_step") {
        output::info(&format!("Next: {next}"), None);
    }
    let serving = &status["serving"];
    if let (Some(sha), false) = (serving["commit_sha"].as_str(), status["live"] == true) {
        let url = serving["url"].as_str().unwrap_or("dev");
        output::dim_line(&format!("Dev serves {} at {url}.", super::short_sha(sha)));
    }
}

fn text<'a>(status: &'a Value, field: &str) -> Option<&'a str> {
    status[field].as_str().filter(|value| !value.is_empty())
}

/// The API takes a full SHA; expand a short one when git knows it.
fn full_sha(commit: &str) -> String {
    git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{commit}^{{commit}}"),
    ])
    .unwrap_or_else(|| commit.to_string())
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !stdout.is_empty()).then_some(stdout)
}
