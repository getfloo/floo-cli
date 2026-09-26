use std::process;

use crate::api_types::{ManagedServiceSummary, StorageObjectRestoreResponse};
use crate::confirm::{confirm_tier2, ConfirmOutcome, RiskMetadata, Tier};
use crate::errors::ErrorCode;
use crate::output;

pub fn ls(app: Option<&str>, name: &str, env: &str, prefix: Option<&str>, limit: u8) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let service = resolve_storage_service(&client, &app_id, &app_name, name);
    let response =
        match client.list_storage_objects(&app_id, &service.id, env, prefix.unwrap_or(""), limit) {
            Ok(response) => response,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };

    if output::is_json_mode() {
        output::success("Storage objects listed.", Some(output::to_value(&response)));
        return;
    }

    output::info(
        &format!(
            "Storage objects on {app_name} (storage:{name}, env={env}, bucket={}):",
            response.bucket_name
        ),
        None,
    );
    let rows: Vec<Vec<String>> = response
        .objects
        .iter()
        .map(|object| {
            vec![
                object.name.clone(),
                object
                    .human_size
                    .clone()
                    .unwrap_or_else(|| format!("{} B", object.size)),
                object.updated.clone().unwrap_or_else(|| "-".to_string()),
                object
                    .content_type
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();
    output::table(&["Path", "Size", "Updated", "Content type"], &rows, None);
    if response.truncated {
        output::warn("Object list truncated. Use a narrower prefix to see more.");
    }
}

pub fn usage(app: Option<&str>, name: &str, env: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let service = resolve_storage_service(&client, &app_id, &app_name, name);
    let response = match client.get_storage_usage(&app_id, &service.id, env) {
        Ok(response) => response,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            "Storage usage retrieved.",
            Some(output::to_value(&response)),
        );
        return;
    }

    output::info(
        &format!("Storage usage on {app_name} (storage:{name}, env={env}):"),
        None,
    );
    output::info(&format!("  Bucket: {}", response.bucket_name), None);
    output::info(&format!("  Objects: {}", response.object_count), None);
    output::info(
        &format!("  Size: {} bytes", response.total_size_bytes),
        None,
    );
}

pub fn rm(app: Option<&str>, name: &str, env: &str, object_path: &str, yes: bool) {
    if output::is_dry_run_mode() {
        let risk: RiskMetadata = Tier::Two.into();
        output::dry_run_preview(
            &format!("Would remove storage object '{object_path}' from {env}."),
            serde_json::json!({
                "action": "storage_rm", "app": app, "storage": name,
                "environment": env, "path": object_path,
                "destructive": risk.destructive, "data_loss": risk.data_loss,
                "tier": risk.tier,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let service = resolve_storage_service(&client, &app_id, &app_name, name);
    match confirm_tier2(
        "Remove storage object",
        &format!("'{object_path}' on {app_name} (storage:{name}, env={env})"),
        yes,
    ) {
        ConfirmOutcome::Proceed => {}
        ConfirmOutcome::Aborted => {
            output::info("Cancelled.", None);
            return;
        }
        ConfirmOutcome::Refused { suggestion } => crate::confirm::exit_refused(
            &format!("Refusing to remove storage object '{object_path}' without confirmation."),
            &suggestion,
        ),
    }

    if let Err(e) = client.delete_storage_object(&app_id, &service.id, env, object_path) {
        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
        process::exit(1);
    }

    let risk: RiskMetadata = Tier::Two.into();
    output::success(
        &format!("Removed {object_path} from {app_name} (storage:{name}, env={env}). Restorable for 30 days with `floo storage restore`."),
        Some(serde_json::json!({
            "path": object_path, "environment": env, "app": app_name,
            "storage": name, "deleted": true, "restorable_days": 30,
            "destructive": risk.destructive, "data_loss": risk.data_loss,
            "tier": risk.tier,
        })),
    );
}

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
