use std::process;

use crate::api_types::CreateManagedServiceRequest;
use crate::errors::ErrorCode;
use crate::output;

/// List every service on an app — both the user-authored app services
/// (Cloud Run revisions) and the platform-managed services (postgres,
/// redis, storage). The two live on different authoring surfaces but
/// belong in one read model so `floo services list` is a complete view.
/// Matches the info command's "both surfaces" routing landed in #110.
pub fn list(app: Option<&str>, env: &str) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);
    let app_id = app_id.as_str();
    let app_name = app_name.as_str();

    let app_services = match client.list_services(app_id, Some(env)) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    // Managed services fetch failure is non-fatal — app services still render
    // and we surface the error as a warning so the user knows the view is
    // partial, not wrong.
    let managed_services = match client.list_managed_services(app_id) {
        Ok(r) => Some(r.managed_services),
        Err(e) => {
            output::warn(&format!(
                "Could not fetch managed services (partial view): {}",
                e.message
            ));
            None
        }
    };
    let managed_services = managed_services.unwrap_or_default();

    if app_services.services.is_empty() && managed_services.is_empty() {
        if output::is_json_mode() {
            output::success(
                &format!("No services on {app_name}."),
                Some(serde_json::json!({
                    "app_services": [],
                    "managed_services": [],
                })),
            );
        } else {
            output::info(
                &format!(
                    "No services on {app_name}. Add one with 'floo services add <type>' or by declaring services in floo.app.toml."
                ),
                None,
            );
        }
        return;
    }

    if output::is_json_mode() {
        output::success(
            &format!("Services for {app_name}"),
            Some(serde_json::json!({
                "app_services": output::to_value(&app_services.services),
                "managed_services": output::to_value(&managed_services),
            })),
        );
        return;
    }

    // Human mode: render unified table rows. Managed services show their
    // type as the "Type" cell and "managed" as the ingress so users see them
    // as a distinct class without a second table.
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(
        app_services.services.len() + managed_services.len(),
    );
    for s in &app_services.services {
        rows.push(vec![
            s.name.clone(),
            s.service_type.as_deref().unwrap_or("-").to_string(),
            s.status.as_deref().unwrap_or("-").to_string(),
            s.ingress.as_deref().unwrap_or("-").to_string(),
            s.cloud_run_url.as_deref().unwrap_or("-").to_string(),
        ]);
    }
    for ms in &managed_services {
        rows.push(vec![
            ms.name.clone(),
            ms.service_type.clone(),
            ms.status.clone(),
            "managed".to_string(),
            "\u{2014}".to_string(),
        ]);
    }

    output::table(&["Name", "Type", "Status", "Ingress", "URL"], &rows, None);
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
        Ok(r) => r.managed_services,
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

    let created = detail.created_at.as_deref().unwrap_or("-");
    output::info(
        &format!(
            "Managed service {} (name: {}, app: {app_name}):",
            detail.service_type, detail.name
        ),
        None,
    );
    output::info(&format!("  Status:   {}", detail.status), None);
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

pub fn add(service_type: &str, app: Option<&str>, tier: &str, name: &str) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let body = CreateManagedServiceRequest {
        service_type,
        name,
        tier,
    };

    let detail = match client.create_managed_service(&app_id, &body) {
        Ok(d) => d,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    // Update the lock file so the managed-service state shows up in git diffs.
    if let Err(e) = crate::services_lock::record_add(&detail) {
        output::warn(&format!(
            "Provisioned {service_type} for {app_name}, but failed to update .floo/services.lock: {e}. Run the command again or hand-edit nothing — the platform is the source of truth."
        ));
    }

    if output::is_json_mode() {
        output::success(
            &format!("Provisioned {service_type} for {app_name}"),
            Some(output::to_value(&detail)),
        );
        return;
    }

    output::info(
        &format!(
            "\u{2713} Provisioned {service_type} (name: {}, tier: {tier}) for {app_name}.",
            detail.name
        ),
        None,
    );
    if !detail.env_var_keys.is_empty() {
        output::info(
            &format!(
                "  Injected env vars on next deploy: {}",
                detail.env_var_keys.join(", ")
            ),
            None,
        );
    }
}

/// Tier-3 destructive: destroying a managed service is irreversible data loss.
///
/// The UX contract:
/// - Interactive: must type the service name to confirm. No y/N shortcut.
/// - Non-interactive (CI, agents): `--yes-i-know-this-destroys-data` flag is
///   required. The flag is deliberately verbose so it cannot be reached for by
///   reflex. A script using this flag must have user authorization for the
///   specific resource (per the skill rule).
pub fn remove(service_type: &str, app: Option<&str>, name: &str, confirmed: bool) {
    use crate::confirm::{confirm_tier3, ConfirmOutcome, RiskMetadata, Tier};

    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    // Look up the row we're about to destroy so we can tell the user exactly
    // what data will be lost. Also gives us the real UUID for the DELETE call.
    let managed = match client.list_managed_services(&app_id) {
        Ok(r) => r.managed_services,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let target = managed
        .iter()
        .find(|m| m.service_type == service_type && m.name == name);

    let target = match target {
        Some(t) => t,
        None => {
            output::error(
                &format!("No managed {service_type} named '{name}' on {app_name}."),
                &ErrorCode::ManagedServiceNotFound,
                Some("Run 'floo services list' to see what's provisioned."),
            );
            process::exit(1);
        }
    };

    let mut preamble = vec![
        format!(
            "\u{26a0} You are about to permanently destroy the following managed service:"
        ),
        format!("    app:   {app_name}"),
        format!("    type:  {service_type}"),
        format!("    name:  {name}"),
        format!("    id:    {} (status={})", target.id, target.status),
    ];
    if !target.env_var_keys.is_empty() {
        preamble.push(format!(
            "    env vars removed from runtime: {}",
            target.env_var_keys.join(", ")
        ));
    }

    match confirm_tier3(name, &preamble, confirmed) {
        ConfirmOutcome::Proceed => {}
        ConfirmOutcome::Aborted => {
            if !output::is_json_mode() {
                output::info("Aborted \u{2014} nothing was destroyed.", None);
            }
            process::exit(1);
        }
        ConfirmOutcome::Refused { suggestion } => {
            crate::confirm::exit_refused(
                &format!(
                    "Refusing to destroy managed {service_type} '{name}' on {app_name} without explicit confirmation."
                ),
                &suggestion,
            );
        }
    }

    if let Err(e) = client.delete_managed_service(&app_id, &target.id) {
        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
        process::exit(1);
    }

    if let Err(e) = crate::services_lock::record_remove(service_type, name) {
        output::warn(&format!(
            "Destroyed {service_type}/{name} on {app_name}, but failed to update .floo/services.lock: {e}."
        ));
    }

    let risk: RiskMetadata = Tier::Three.into();
    if output::is_json_mode() {
        output::success(
            &format!("Destroyed managed {service_type}/{name} on {app_name}"),
            Some(serde_json::json!({
                "type": service_type,
                "name": name,
                "app": app_name,
                "destructive": risk.destructive,
                "data_loss": risk.data_loss,
                "tier": risk.tier,
            })),
        );
        return;
    }

    output::info(
        &format!(
            "\u{2713} Destroyed managed {service_type}/{name} on {app_name}."
        ),
        None,
    );
}

/// One-shot migration from legacy `[postgres]`/`[redis]`/`[storage]` TOML
/// sections to CLI-managed state.
///
/// Zero data impact: the underlying managed-service rows already exist
/// (auto-provisioned by the deprecated TOML path). This command just:
/// 1. Reads the TOML sections.
/// 2. Calls POST for each (idempotent — the API returns the existing row).
/// 3. Writes `.floo/services.lock` with the final state.
/// 4. Prints instructions to delete the TOML sections going forward.
///
/// See docs/knowledge/domains/managed-services.md for the full doctrine.
pub fn migrate(app: Option<&str>, path: &std::path::Path) {
    use crate::api_types::CreateManagedServiceRequest;
    use crate::project_config;

    super::require_auth();
    let client = super::init_client(None);

    let canonical_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            output::error(
                &format!("Path '{}' is not a directory.", path.display()),
                &ErrorCode::InvalidPath,
                Some("Provide a valid project directory."),
            );
            process::exit(1);
        }
    };

    let resolved = match project_config::resolve_app_context(&canonical_path, app) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let declarations = project_config::discover_managed_services(&resolved);
    if declarations.is_empty() {
        if output::is_json_mode() {
            output::success(
                "No legacy managed-service sections to migrate.",
                Some(serde_json::json!({
                    "migrated": [],
                    "app": resolved.app_name,
                })),
            );
        } else {
            output::info(
                &format!(
                    "No [postgres]/[redis]/[storage] sections in floo.app.toml for {}. Nothing to migrate.",
                    resolved.app_name
                ),
                None,
            );
        }
        return;
    }

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let mut migrated: Vec<serde_json::Value> = Vec::new();
    for decl in &declarations {
        // `name` in ManagedServiceDeclaration is the type ("postgres"/"redis"/"storage").
        let service_type = decl.name.as_str();
        let tier = decl.tier.as_deref().unwrap_or("basic");

        let request = CreateManagedServiceRequest {
            service_type,
            name: "default",
            tier,
        };

        let detail = match client.create_managed_service(&app_id, &request) {
            Ok(d) => d,
            Err(e) => {
                output::error(
                    &format!(
                        "Failed to migrate {service_type}: {message}",
                        message = e.message
                    ),
                    &ErrorCode::from_api(&e.code),
                    Some(
                        "The managed service row may already exist; check 'floo services list'. \
                         If this persists, file feedback via 'floo feedback --category bug'.",
                    ),
                );
                process::exit(1);
            }
        };

        if let Err(e) = crate::services_lock::record_add(&detail) {
            output::warn(&format!(
                "Migrated {service_type} but failed to update .floo/services.lock: {e}"
            ));
        }

        migrated.push(serde_json::json!({
            "type": service_type,
            "name": detail.name,
            "status": detail.status,
            "tier": tier,
        }));
    }

    let sections: Vec<&str> = declarations.iter().map(|d| d.name.as_str()).collect();
    let sections_display = sections
        .iter()
        .map(|s| format!("[{s}]"))
        .collect::<Vec<_>>()
        .join(", ");

    if output::is_json_mode() {
        output::success(
            &format!(
                "Migrated {} managed service(s) for {app_name}.",
                migrated.len()
            ),
            Some(serde_json::json!({
                "app": app_name,
                "migrated": migrated,
                "next_steps": [
                    format!("Delete the {sections_display} sections from floo.app.toml"),
                    "Commit the updated .floo/services.lock".to_string(),
                    "Push — the deprecation warning will stop firing on next deploy".to_string(),
                ],
            })),
        );
        return;
    }

    output::info(
        &format!(
            "\u{2713} Migrated {} managed service(s) for {app_name}.",
            migrated.len()
        ),
        None,
    );
    for item in &migrated {
        if let (Some(t), Some(n)) = (
            item.get("type").and_then(|v| v.as_str()),
            item.get("name").and_then(|v| v.as_str()),
        ) {
            output::info(&format!("    {t}/{n}"), None);
        }
    }
    output::info("", None);
    output::info("Next steps:", None);
    output::info(
        &format!("  1. Delete the {sections_display} sections from floo.app.toml"),
        None,
    );
    output::info("  2. Commit the updated .floo/services.lock", None);
    output::info(
        "  3. Push — the deprecation warning will stop firing on next deploy",
        None,
    );
}
