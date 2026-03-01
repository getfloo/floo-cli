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

    let services = result
        .get("services")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            output::error(
                "Failed to parse services from API response.",
                "PARSE_ERROR",
                Some("This is a bug. Please report it."),
            );
            process::exit(1);
        })
        .clone();

    if services.is_empty() {
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

    let rows: Vec<Vec<String>> = services
        .iter()
        .map(|s| {
            vec![
                super::expect_str_field(s, "name").to_string(),
                super::expect_str_field(s, "type").to_string(),
                super::expect_str_field(s, "status").to_string(),
                s.get("cloud_run_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
            ]
        })
        .collect();

    output::table(
        &["Name", "Type", "Status", "URL"],
        &rows,
        Some(serde_json::json!({"services": services})),
    );
}

pub fn info(service_name: &str, app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let app_id = app_id.as_str();
    let app_name = app_name.as_str();

    let services_result = match client.list_services(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let services = services_result
        .get("services")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            output::error(
                "Failed to parse services from API response.",
                "PARSE_ERROR",
                Some("This is a bug. Please report it."),
            );
            process::exit(1);
        })
        .clone();

    // Check if any user-managed service matches the given name
    let matched = services
        .iter()
        .find(|s| s.get("name").and_then(|v| v.as_str()) == Some(service_name));

    if let Some(svc) = matched {
        // User-managed service found — display its details
        if output::is_json_mode() {
            output::success(
                &format!("Service {service_name} on {app_name}"),
                Some(svc.clone()),
            );
            return;
        }

        let svc_type = svc.get("type").and_then(|v| v.as_str()).unwrap_or("-");
        let status = svc.get("status").and_then(|v| v.as_str()).unwrap_or("-");
        let url = svc
            .get("cloud_run_url")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let port = svc
            .get("port")
            .and_then(|v| v.as_u64())
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
    if !services.is_empty() {
        let names: Vec<&str> = services
            .iter()
            .filter_map(|s| s.get("name").and_then(|v| v.as_str()))
            .collect();
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
            Some(db_data),
        );
        return;
    }

    let host = super::expect_str_field(&db_data, "host");
    let port = db_data
        .get("port")
        .and_then(|v| v.as_u64())
        .map(|p| p.to_string())
        .unwrap_or_else(|| {
            output::error(
                "Response missing 'port' field.",
                "PARSE_ERROR",
                Some("This is a bug. Please report it."),
            );
            process::exit(1);
        });
    let database = super::expect_str_field(&db_data, "database");
    let status = super::expect_str_field(&db_data, "status");

    output::info(&format!("Service {service_name} ({app_name}):"), None);
    output::info(&format!("  Host:     {host}"), None);
    output::info(&format!("  Port:     {port}"), None);
    output::info(&format!("  Database: {database}"), None);
    output::info(&format!("  Status:   {status}"), None);
    if let Some(username) = db_data.get("username").and_then(|v| v.as_str()) {
        output::info(&format!("  Username: {username}"), None);
    }
    if let Some(schema) = db_data.get("schema_name").and_then(|v| v.as_str()) {
        output::info(&format!("  Schema:   {schema}"), None);
    }
    output::info("", None);
    output::info("DATABASE_URL is injected as an environment variable.", None);
}
