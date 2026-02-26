use std::process;

use crate::output;
use crate::resolve::resolve_app;

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

    let total = result
        .get("total")
        .and_then(|v| v.as_u64())
        .unwrap_or(apps.len() as u64) as u32;

    if apps.is_empty() {
        if !output::is_json_mode() {
            output::info("No apps yet. Deploy one with floo deploy.", None);
        } else {
            output::success(
                "No apps.",
                Some(serde_json::json!({"apps": [], "total": total, "page": page, "per_page": per_page})),
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

    if !output::is_json_mode() && total > page * per_page {
        let remaining = total - page * per_page;
        output::dim_line(&format!(
            "{remaining} more app{} not shown. Use --page {} to see next page.",
            if remaining == 1 { "" } else { "s" },
            page + 1
        ));
    }
}

pub fn status(app_name: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let app_data = match resolve_app(&client, app_name) {
        Ok(a) => a,
        Err(e) => {
            if e.code == "APP_NOT_FOUND" {
                output::error(
                    &format!("App '{app_name}' not found."),
                    "APP_NOT_FOUND",
                    Some("Check the app name or ID and try again."),
                );
            } else {
                output::error(&e.message, &e.code, None);
            }
            process::exit(1);
        }
    };

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
    let app_data = match resolve_app(&client, app_name) {
        Ok(a) => a,
        Err(e) => {
            if e.code == "APP_NOT_FOUND" {
                output::error(
                    &format!("App '{app_name}' not found."),
                    "APP_NOT_FOUND",
                    Some("Check the app name or ID and try again."),
                );
            } else {
                output::error(&e.message, &e.code, None);
            }
            process::exit(1);
        }
    };

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

pub fn connect(repo: &str, installation_id: u64, app_name: &str, branch: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let app_data = match resolve_app(&client, app_name) {
        Ok(a) => a,
        Err(e) => {
            if e.code == "APP_NOT_FOUND" {
                output::error(
                    &format!("App '{app_name}' not found."),
                    "APP_NOT_FOUND",
                    Some("Check the app name or ID and try again."),
                );
            } else {
                output::error(&e.message, &e.code, None);
            }
            process::exit(1);
        }
    };

    let app_id = super::expect_str_field(&app_data, "id");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    let result = match client.github_connect(app_id, repo, installation_id, branch) {
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

    let connected_branch = super::expect_str_field(&result, "default_branch");

    output::success(
        &format!("Connected {name} to {repo} (branch: {connected_branch})"),
        Some(result),
    );
}

pub fn disconnect(app_name: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let app_data = match resolve_app(&client, app_name) {
        Ok(a) => a,
        Err(e) => {
            if e.code == "APP_NOT_FOUND" {
                output::error(
                    &format!("App '{app_name}' not found."),
                    "APP_NOT_FOUND",
                    Some("Check the app name or ID and try again."),
                );
            } else {
                output::error(&e.message, &e.code, None);
            }
            process::exit(1);
        }
    };

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
