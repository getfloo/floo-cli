use std::process;

use crate::output;

pub fn list(app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let app_id = app_id.as_str();
    let app_name = app_name.as_str();

    let result = match client.list_services(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    if result.services.is_empty() {
        if output::is_json_mode() {
            output::success(
                &format!("No services on {app_name}."),
                Some(serde_json::json!({"services": []})),
            );
        } else {
            output::info(&format!("No services on {app_name}."), None);
        }
        return;
    }

    let rows: Vec<Vec<String>> = result
        .services
        .iter()
        .map(|s| {
            vec![
                s.name.clone(),
                s.service_type.as_deref().unwrap_or("-").to_string(),
                s.status.as_deref().unwrap_or("-").to_string(),
                s.cloud_run_url.as_deref().unwrap_or("-").to_string(),
            ]
        })
        .collect();

    output::table(
        &["Name", "Type", "Status", "URL"],
        &rows,
        Some(output::to_value(&result)),
    );
}

pub fn info(service_name: &str, app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let app_id = app_id.as_str();
    let app_name = app_name.as_str();

    let result = match client.list_services(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    // Check if any user-managed service matches the given name
    let matched = result.services.iter().find(|s| s.name == service_name);

    if let Some(svc) = matched {
        // User-managed service found — display its details
        if output::is_json_mode() {
            output::success(
                &format!("Service {service_name} on {app_name}"),
                Some(output::to_value(svc)),
            );
            return;
        }

        let svc_type = svc.service_type.as_deref().unwrap_or("-");
        let status = svc.status.as_deref().unwrap_or("-");
        let url = svc.cloud_run_url.as_deref().unwrap_or("-");
        let port = svc
            .port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());

        output::info(&format!("Service {service_name} ({app_name}):"), None);
        output::info(&format!("  Type:   {svc_type}"), None);
        output::info(&format!("  Status: {status}"), None);
        output::info(&format!("  URL:    {url}"), None);
        output::info(&format!("  Port:   {port}"), None);
        return;
    }

    // No user-managed service found with that name.
    // If the app has user-managed services, the name is simply wrong.
    if !result.services.is_empty() {
        let names: Vec<&str> = result.services.iter().map(|s| s.name.as_str()).collect();
        let suggestion = format!(
            "Available services: {}. Run 'floo services list' for details.",
            names.join(", ")
        );
        output::error(
            &format!("Service '{service_name}' not found on {app_name}."),
            "SERVICE_NOT_FOUND",
            Some(&suggestion),
        );
        process::exit(1);
    }

    // No user-managed services at all — try Floo-managed database.
    let db_data = match client.get_database_info(app_id) {
        Ok(db) => db,
        Err(e) => {
            if e.code == "DATABASE_NOT_FOUND" {
                output::error(
                    &format!("Service '{service_name}' not found on {app_name}."),
                    "SERVICE_NOT_FOUND",
                    Some("Run 'floo services list' to see available services."),
                );
            } else {
                output::error(&e.message, &e.code, None);
            }
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            &format!("Service {service_name} on {app_name}"),
            Some(output::to_value(&db_data)),
        );
        return;
    }

    let host = &db_data.host;
    let port = db_data.port.to_string();
    let database = &db_data.database;
    let status = db_data.status.as_deref().unwrap_or("-");

    output::info(&format!("Service {service_name} ({app_name}):"), None);
    output::info(&format!("  Host:     {host}"), None);
    output::info(&format!("  Port:     {port}"), None);
    output::info(&format!("  Database: {database}"), None);
    output::info(&format!("  Status:   {status}"), None);
    if let Some(username) = db_data.username.as_deref() {
        output::info(&format!("  Username: {username}"), None);
    }
    if let Some(schema) = db_data.schema_name.as_deref() {
        output::info(&format!("  Schema:   {schema}"), None);
    }
    output::info("", None);
    output::info("DATABASE_URL is injected as an environment variable.", None);
}
