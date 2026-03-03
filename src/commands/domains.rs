use std::process;

use crate::api_client::FlooClient;
use crate::errors::ErrorCode;
use crate::output;

fn check_services_flag(client: &FlooClient, app_id: &str, services: Option<&str>) {
    let result = match client.list_services(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if result.services.len() > 1 && services.is_none() {
        output::error(
            "Multiple services found. Specify --services.",
            &ErrorCode::MultipleServicesNoTarget,
            Some("Use --services <name> to target a specific service."),
        );
        process::exit(1);
    }

    if let Some(svc_name) = services {
        let exists = result.services.iter().any(|s| s.name == svc_name);
        if !exists {
            output::error(
                &format!("Service '{svc_name}' not found."),
                &ErrorCode::ServiceNotFound,
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

    let result = match client.add_domain(&app_id, hostname, services) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            &format!("Added {hostname}"),
            Some(output::to_value(&result)),
        );
    } else {
        output::success(
            &format!("Added domain {hostname} to {app_name}."),
            Some(serde_json::Value::Null),
        );
        let status = result.status.as_deref().unwrap_or("-");
        output::info(&format!("  Status: {status}"), None);
        if let Some(dns) = result.dns_instructions.as_deref() {
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
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if result.domains.is_empty() {
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

    let rows: Vec<Vec<String>> = result
        .domains
        .iter()
        .map(|d| {
            vec![
                d.hostname.clone(),
                d.status.as_deref().unwrap_or("-").to_string(),
                d.dns_instructions
                    .as_deref()
                    .unwrap_or("\u{2014}")
                    .to_string(),
            ]
        })
        .collect();

    output::table(
        &["Domain", "Status", "DNS"],
        &rows,
        Some(output::to_value(&result)),
    );
}

pub fn remove(hostname: &str, app: Option<&str>, services: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    check_services_flag(&client, &app_id, services);

    if let Err(e) = client.delete_domain(&app_id, hostname) {
        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
        process::exit(1);
    }

    output::success(
        &format!("Removed domain {hostname} from {app_name}."),
        Some(serde_json::json!({"hostname": hostname})),
    );
}
