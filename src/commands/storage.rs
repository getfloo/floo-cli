use std::process;

use crate::api_types::{ManagedServiceSummary, StorageObjectRestoreResponse};
use crate::errors::ErrorCode;
use crate::output;

pub fn versions(app: Option<&str>, name: &str, env: &str, object_path: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let service = resolve_storage_service(&client, &app_id, &app_name, name);

    let response = match client.list_storage_object_versions(&app_id, &service.id, object_path, env)
    {
        Ok(response) => response,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            &format!("Storage versions for {object_path} on {app_name}"),
            Some(output::to_value(&response)),
        );
        return;
    }

    output::info(
        &format!(
            "Storage versions for {} on {} (storage:{}, env={}):",
            response.object_path, app_name, service.name, env
        ),
        None,
    );
    output::info(&format!("  Bucket: {}", response.bucket_name), None);

    let rows: Vec<Vec<String>> = response
        .versions
        .iter()
        .map(|version| {
            vec![
                version.generation.clone(),
                if version.is_live {
                    "live".to_string()
                } else {
                    "noncurrent".to_string()
                },
                version.size_human.clone(),
                version
                    .content_type
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                version
                    .updated_at
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();
    output::table(
        &["Generation", "State", "Size", "Content type", "Updated"],
        &rows,
        None,
    );

    if response.truncated {
        output::warn("Version list truncated. Narrow the object path and retry.");
    }
}

pub fn restore(app: Option<&str>, name: &str, env: &str, object_path: &str, generation: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let service = resolve_storage_service(&client, &app_id, &app_name, name);

    let response = match client.restore_storage_object_generation(
        &app_id,
        &service.id,
        object_path,
        generation,
        env,
    ) {
        Ok(response) => response,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            &format!("Restored storage object {object_path} on {app_name}"),
            Some(output::to_value(&response)),
        );
        return;
    }

    render_restore(&response, &app_name, &service.name, env);
}

fn resolve_storage_service(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    app_name: &str,
    name: &str,
) -> ManagedServiceSummary {
    let managed_services = match client.list_managed_services(app_id) {
        Ok(response) => response.managed_services,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let service = managed_services
        .into_iter()
        .find(|service| service.service_type == "storage" && service.name == name);

    match service {
        Some(service) => service,
        None => {
            output::error(
                &format!("No managed storage service named '{name}' on {app_name}."),
                &ErrorCode::ManagedServiceNotFound,
                Some("Run 'floo services list' to see managed storage services."),
            );
            process::exit(1);
        }
    }
}

fn render_restore(
    response: &StorageObjectRestoreResponse,
    app_name: &str,
    service_name: &str,
    env: &str,
) {
    output::info(
        &format!(
            "Restored {} on {} (storage:{}, env={}).",
            response.object_path, app_name, service_name, env
        ),
        None,
    );
    output::info(
        &format!("  Restored generation: {}", response.restored_generation),
        None,
    );
    output::info(
        &format!("  New live generation: {}", response.live_generation),
        None,
    );
    output::info(&format!("  Bucket: {}", response.bucket_name), None);
    output::info(&format!("  Size: {}", response.size_human), None);
}
