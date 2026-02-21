use std::process;

use crate::config::load_config;
use crate::output;
use crate::resolve::resolve_app;

fn require_auth() {
    let config = load_config();
    if config.api_key.is_none() {
        output::error(
            "Not logged in.",
            "NOT_AUTHENTICATED",
            Some("Run 'floo login' to authenticate."),
        );
        process::exit(1);
    }
}

pub fn promote(app_name: &str, tag: Option<&str>) {
    require_auth();
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

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    let _spinner = output::Spinner::new(&format!("Promoting {name} to prod..."));

    let result = match client.promote_app(app_id, tag) {
        Ok(r) => r,
        Err(e) => {
            let suggestion = match e.code.as_str() {
                "GITHUB_NOT_CONNECTED" => {
                    Some("Connect a GitHub repo first: floo apps connect --repo org/repo --installation-id <id> --app <name>")
                }
                "NO_DEV_DEPLOY" => Some("Deploy to dev first: floo deploy --app <name>"),
                "RELEASE_TAG_EXISTS" => Some("Use a different tag with --tag <tag>"),
                _ => None,
            };
            output::error(&e.message, &e.code, suggestion);
            process::exit(1);
        }
    };

    let result_tag = result
        .get("tag")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let release_url = result
        .get("release_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if output::is_json_mode() {
        output::success(
            &format!("Promoted {name} → prod ({result_tag})"),
            Some(result),
        );
    } else {
        output::success(
            &format!("Promoted {name} → prod ({result_tag})"),
            Some(serde_json::json!({
                "app": name,
                "tag": result_tag,
                "release_url": release_url,
            })),
        );
        if !release_url.is_empty() {
            output::dim_line(&format!("Release: {release_url}"));
        }
        output::dim_line("Deployment in progress via GitHub webhook.");
    }
}

pub fn list(app_name: &str) {
    require_auth();
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

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");

    let result = match client.list_releases(app_id, 1, 20) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let releases = result
        .get("releases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if releases.is_empty() {
        if output::is_json_mode() {
            output::success("No releases.", Some(serde_json::json!({"releases": []})));
        } else {
            output::info("No releases yet. Promote with: floo promote <app>", None);
        }
        return;
    }

    let rows: Vec<Vec<String>> = releases
        .iter()
        .map(|r| {
            let sha = r.get("commit_sha").and_then(|v| v.as_str()).unwrap_or("-");
            let short_sha = if sha.len() > 7 { &sha[..7] } else { sha };
            vec![
                r.get("release_number")
                    .and_then(|v| v.as_u64())
                    .map(|n| format!("#{n}"))
                    .unwrap_or_else(|| "-".to_string()),
                r.get("tag")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                short_sha.to_string(),
                r.get("promoted_by")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                r.get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
            ]
        })
        .collect();

    output::table(
        &["#", "Tag", "Commit", "Promoted By", "Created"],
        &rows,
        Some(result),
    );
}

pub fn show(release_id: &str, app_name: &str) {
    require_auth();
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

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");

    let release = match client.get_release(app_id, release_id) {
        Ok(r) => r,
        Err(e) => {
            if e.code == "RELEASE_NOT_FOUND" {
                output::error(
                    &format!("Release '{release_id}' not found."),
                    "RELEASE_NOT_FOUND",
                    Some("Check the release ID and try again."),
                );
            } else {
                output::error(&e.message, &e.code, None);
            }
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        let tag = release
            .get("tag")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        output::success(&format!("Release {tag}"), Some(release));
    } else {
        let tag = release.get("tag").and_then(|v| v.as_str()).unwrap_or("-");
        let number = release
            .get("release_number")
            .and_then(|v| v.as_u64())
            .map(|n| format!("#{n}"))
            .unwrap_or_else(|| "-".to_string());
        let sha = release
            .get("commit_sha")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let promoted_by = release
            .get("promoted_by")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let created = release
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let deploy_id = release
            .get("deploy_id")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let image = release
            .get("image_digest")
            .and_then(|v| v.as_str())
            .unwrap_or("-");

        output::info(&format!("Release {tag} ({number})"), None);
        output::info(&format!("  Tag:         {tag}"), None);
        output::info(&format!("  Commit:      {sha}"), None);
        output::info(&format!("  Promoted by: {promoted_by}"), None);
        output::info(&format!("  Deploy ID:   {deploy_id}"), None);
        output::info(&format!("  Image:       {image}"), None);
        output::info(&format!("  Created:     {created}"), None);
    }
}
