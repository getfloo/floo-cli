use std::process;

use crate::errors::ErrorCode;
use crate::output;

pub fn list(app: Option<&str>, env: &str) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let app_id = app_id.as_str();
    let app_name = app_name.as_str();

    let result = match client.list_services(app_id, Some(env)) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
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
                s.ingress.as_deref().unwrap_or("-").to_string(),
                s.cloud_run_url.as_deref().unwrap_or("-").to_string(),
            ]
        })
        .collect();

    output::table(
        &["Name", "Type", "Status", "Ingress", "URL"],
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

    let app_services = match client.list_services(app_id, None) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if let Some(svc) = app_services.services.iter().find(|s| s.name == service_name) {
        render_app_service(svc, service_name, app_name);
        return;
    }

    // Application-service name didn't match. Try managed services (postgres, redis, storage).
    // Both the type name (e.g. "postgres") and the row name (default = "default") are accepted.
    let managed = match client.list_managed_services(app_id) {
        Ok(r) => r.services,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let managed_match = managed.iter().find(|m| {
        m.service_type == service_name
            || m.name == service_name
            || format!("{}-{}", m.service_type, m.name) == service_name
    });

    if let Some(ms) = managed_match {
        let detail = match client.get_managed_service(app_id, &ms.id) {
            Ok(d) => d,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };
        render_managed_service(&detail, service_name, app_name);
        return;
    }

    // Nothing matched. Build a helpful "did you mean?" listing both surfaces.
    let mut available: Vec<String> = app_services
        .services
        .iter()
        .map(|s| s.name.clone())
        .collect();
    for m in &managed {
        available.push(m.service_type.clone());
    }
    let suggestion = if available.is_empty() {
        "This app has no services yet. Run 'floo services list' to verify.".to_string()
    } else {
        format!(
            "Available services: {}. Run 'floo services list' for details.",
            available.join(", ")
        )
    };
    output::error(
        &format!("Service '{service_name}' not found on {app_name}."),
        &ErrorCode::ServiceNotFound,
        Some(&suggestion),
    );
    process::exit(1);
}

fn render_app_service(svc: &crate::api_types::ApiService, service_name: &str, app_name: &str) {
    if output::is_json_mode() {
        output::success(
            &format!("Service {service_name} on {app_name}"),
            Some(output::to_value(svc)),
        );
        return;
    }

    let svc_type = svc.service_type.as_deref().unwrap_or("-");
    let status = svc.status.as_deref().unwrap_or("-");
    let ingress = svc.ingress.as_deref().unwrap_or("-");
    let url = svc.cloud_run_url.as_deref().unwrap_or("-");
    let port = svc
        .port
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".to_string());

    output::info(&format!("Service {service_name} ({app_name}):"), None);
    output::info(&format!("  Type:    {svc_type}"), None);
    output::info(&format!("  Status:  {status}"), None);
    output::info(&format!("  Ingress: {ingress}"), None);
    output::info(&format!("  URL:     {url}"), None);
    output::info(&format!("  Port:    {port}"), None);
}

fn render_managed_service(
    detail: &crate::api_types::ManagedServiceDetail,
    service_name: &str,
    app_name: &str,
) {
    if output::is_json_mode() {
        output::success(
            &format!("Managed service {service_name} on {app_name}"),
            Some(output::to_value(detail)),
        );
        return;
    }

    let tier = detail.tier.as_deref().unwrap_or("basic");
    let created = detail.created_at.as_deref().unwrap_or("-");
    output::info(
        &format!(
            "Managed service {} (name: {}, app: {app_name}):",
            detail.service_type, detail.name
        ),
        None,
    );
    output::info(&format!("  Status:   {}", detail.status), None);
    output::info(&format!("  Tier:     {tier}"), None);
    output::info(&format!("  Created:  {created}"), None);
    if !detail.env_var_keys.is_empty() {
        output::info(
            &format!("  Env vars: {}", detail.env_var_keys.join(", ")),
            None,
        );
        output::info(
            "  (credentials are injected at runtime; use 'floo env list' to see keys)",
            None,
        );
    }
}
