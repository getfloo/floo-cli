use std::process;

use crate::api_client::FlooClient;
use crate::output;

fn check_services_flag(client: &FlooClient, app_id: &str, services: Option<&str>) {
    let result = match client.list_services(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let service_list = result
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

    if service_list.len() > 1 && services.is_none() {
        output::error(
            "Multiple services found. Specify --services.",
            "MULTIPLE_SERVICES_NO_TARGET",
            Some("Use --services <name> to target a specific service."),
        );
        process::exit(1);
    }

    if let Some(svc_name) = services {
        let exists = service_list
            .iter()
            .any(|s| s.get("name").and_then(|v| v.as_str()) == Some(svc_name));
        if !exists {
            output::error(
                &format!("Service '{svc_name}' not found."),
                "SERVICE_NOT_FOUND",
                Some("Run 'floo services list' to see available services."),
            );
            process::exit(1);
        }
    }
}

pub fn add(hostname: &str, app: Option<&str>, services: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    check_services_flag(&client, &app_id, services);

    let result = match client.add_domain(&app_id, hostname) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(&format!("Added {hostname}"), Some(result));
    } else {
        output::success(
            &format!("Added domain {hostname} to {app_name}."),
            Some(serde_json::Value::Null),
        );
        let status = result.get("status").and_then(|v| v.as_str()).unwrap_or("-");
        output::info(&format!("  Status: {status}"), None);
        if let Some(dns) = result.get("dns_instructions").and_then(|v| v.as_str()) {
            output::info(&format!("  DNS:    {dns}"), None);
        }
    }
}

pub fn list(app: Option<&str>, services: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    check_services_flag(&client, &app_id, services);

    let result = match client.list_domains(&app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let domains = result
        .get("domains")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            output::error(
                "Failed to parse domains from API response.",
                "PARSE_ERROR",
                Some("This is a bug. Please report it."),
            );
            process::exit(1);
        })
        .clone();

    if domains.is_empty() {
        if !output::is_json_mode() {
            output::info(
                &format!(
                    "No custom domains on {app_name}. Add one with floo domains add example.com --app {app_name}."
                ),
                None,
            );
        } else {
            output::success("No domains.", Some(serde_json::json!({"domains": []})));
        }
        return;
    }

    let rows: Vec<Vec<String>> = domains
        .iter()
        .map(|d| {
            vec![
                d.get("hostname")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                d.get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                d.get("dns_instructions")
                    .and_then(|v| v.as_str())
                    .unwrap_or("\u{2014}")
                    .to_string(),
            ]
        })
        .collect();

    output::table(
        &["Domain", "Status", "DNS"],
        &rows,
        Some(serde_json::json!({"domains": domains})),
    );
}

pub fn remove(hostname: &str, app: Option<&str>, services: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    check_services_flag(&client, &app_id, services);

    if let Err(e) = client.delete_domain(&app_id, hostname) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }

    output::success(
        &format!("Removed domain {hostname} from {app_name}."),
        Some(serde_json::json!({"hostname": hostname})),
    );
}
