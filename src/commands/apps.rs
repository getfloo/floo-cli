use std::process;

use crate::errors::ErrorCode;
use crate::output;

pub fn list(page: u32, per_page: u32) {
    super::require_auth();
    let client = super::init_client(None);
    let result = match client.list_apps(page, per_page) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let total = result.total.unwrap_or_else(|| {
        eprintln!("Warning: API response missing 'total' field; pagination may be inaccurate.");
        result.apps.len() as u64
    }) as u32;

    if result.apps.is_empty() {
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

    // Resolve org display name once for the table (human mode only).
    // All listed apps belong to the caller's org, so one lookup suffices.
    let org_display: Option<String> = if !output::is_json_mode() {
        client
            .get_org_me()
            .ok()
            .and_then(|o| o.display_name().map(String::from))
    } else {
        None
    };

    let rows: Vec<Vec<String>> = result
        .apps
        .iter()
        .map(|a| {
            let org = org_display
                .as_deref()
                .or(a.org_id.as_deref())
                .unwrap_or("\u{2014}");
            vec![
                a.name.clone(),
                a.status.as_deref().unwrap_or("-").to_string(),
                org.to_string(),
                a.url.as_deref().unwrap_or("\u{2014}").to_string(),
                a.runtime.as_deref().unwrap_or("\u{2014}").to_string(),
                a.created_at.as_deref().unwrap_or("-").to_string(),
            ]
        })
        .collect();

    output::table(
        &["Name", "Status", "Org", "URL", "Runtime", "Created"],
        &rows,
        Some(
            serde_json::json!({"apps": output::to_value(&result.apps), "total": total, "page": page, "per_page": per_page}),
        ),
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
    let app = super::resolve_app_or_exit(&client, app_name);

    if output::is_json_mode() {
        output::success(&format!("App {}", app.name), Some(output::to_value(&app)));
    } else {
        let org_display = resolve_org_display(&client, app.org_id.as_deref());
        output::info(&app.name, None);
        output::info(
            &format!("  Status:   {}", app.status.as_deref().unwrap_or("-")),
            None,
        );
        output::info(
            &format!("  URL:      {}", app.url.as_deref().unwrap_or("\u{2014}")),
            None,
        );
        output::info(
            &format!(
                "  Runtime:  {}",
                app.runtime.as_deref().unwrap_or("\u{2014}")
            ),
            None,
        );
        output::info(&format!("  Org:      {org_display}"), None);
        output::info(&format!("  ID:       {}", app.id), None);
        output::info(
            &format!("  Created:  {}", app.created_at.as_deref().unwrap_or("-")),
            None,
        );
    }
}

fn resolve_org_display(client: &crate::api_client::FlooClient, org_id: Option<&str>) -> String {
    let Some(org_id) = org_id else {
        return "\u{2014}".to_string();
    };
    match client.get_org(org_id) {
        Ok(org) => org
            .display_name()
            .map(String::from)
            .unwrap_or_else(|| org_id.to_string()),
        Err(_) => org_id.to_string(),
    }
}

pub fn delete(app_name: &str, force: bool) {
    super::require_auth();
    let client = super::init_client(None);
    let app = super::resolve_app_or_exit(&client, app_name);

    if !force
        && !output::confirm(&format!(
            "Delete app '{}'? This cannot be undone.",
            app.name
        ))
    {
        if !output::is_json_mode() {
            output::info("Cancelled.", None);
        }
        process::exit(0);
    }

    if let Err(e) = client.delete_app(&app.id) {
        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
        process::exit(1);
    }

    output::success(
        &format!("Deleted app '{}'.", app.name),
        Some(serde_json::json!({"id": app.id})),
    );
}

pub fn show_password(app_name: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let app = super::resolve_app_or_exit(&client, app_name);

    match client.get_app_password(&app.id) {
        Ok(resp) => output::success(
            "App password",
            Some(serde_json::json!({ "password": resp.password })),
        ),
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    }
}
