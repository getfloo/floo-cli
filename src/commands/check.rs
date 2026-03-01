use std::path::PathBuf;
use std::process;

use crate::output;
use crate::project_config::{self, validate_service_name};

pub fn check(path: PathBuf) {
    let project_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            output::error(
                &format!("Path '{}' does not exist.", path.display()),
                "INVALID_PATH",
                Some("Provide a valid project directory."),
            );
            process::exit(1);
        }
    };

    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut warnings: Vec<serde_json::Value> = Vec::new();
    let mut services_info: Vec<serde_json::Value> = Vec::new();

    // 1. Check floo.app.toml exists and parses
    let app_config = match project_config::load_app_config(&project_path) {
        Ok(Some(cfg)) => cfg,
        Ok(None) => {
            output::error(
                &format!("{} not found.", project_config::APP_CONFIG_FILE),
                "CONFIG_INVALID",
                Some("Run `floo init` to create config files."),
            );
            process::exit(1);
        }
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let app_name = &app_config.app.name;

    // 2. Check root floo.service.toml if present
    let root_service = match project_config::load_service_config(&project_path) {
        Ok(svc) => svc,
        Err(e) => {
            errors.push(serde_json::json!({
                "path": ".",
                "message": e.message,
            }));
            None
        }
    };

    // Track seen names for duplicate detection
    let mut seen_names: Vec<String> = Vec::new();

    if let Some(ref svc) = root_service {
        // Validate app name match
        if svc.app.name != *app_name {
            errors.push(serde_json::json!({
                "path": ".",
                "message": format!(
                    "Root {} declares app name '{}', but {} declares '{}'.",
                    project_config::SERVICE_CONFIG_FILE,
                    svc.app.name,
                    project_config::APP_CONFIG_FILE,
                    app_name,
                ),
            }));
        }

        validate_service(
            &svc.service,
            ".",
            &project_path,
            &mut errors,
            &mut warnings,
            &mut services_info,
            &mut seen_names,
        );
    }

    // 3. Check each service declared in app.toml with a path
    for (svc_name, entry) in &app_config.services {
        let Some(ref path_str) = entry.path else {
            continue;
        };

        let normalized = path_str.strip_prefix("./").unwrap_or(path_str);
        let normalized = normalized.strip_suffix('/').unwrap_or(normalized);

        if normalized.is_empty() || normalized == "." {
            continue; // root service, already checked
        }

        let svc_dir = project_path.join(normalized);

        // Check directory exists
        if !svc_dir.is_dir() {
            errors.push(serde_json::json!({
                "path": normalized,
                "message": format!("Service '{svc_name}' path '{normalized}/' does not exist."),
            }));
            continue;
        }

        // Check floo.service.toml exists
        let svc_config = match project_config::load_service_config(&svc_dir) {
            Ok(Some(cfg)) => cfg,
            Ok(None) => {
                errors.push(serde_json::json!({
                    "path": normalized,
                    "message": format!(
                        "No {} found at '{normalized}/' (declared as service '{svc_name}' in {}).",
                        project_config::SERVICE_CONFIG_FILE,
                        project_config::APP_CONFIG_FILE,
                    ),
                }));
                continue;
            }
            Err(e) => {
                errors.push(serde_json::json!({
                    "path": normalized,
                    "message": e.message,
                }));
                continue;
            }
        };

        // Check app name match
        if svc_config.app.name != *app_name {
            errors.push(serde_json::json!({
                "path": normalized,
                "message": format!(
                    "Service '{svc_name}' at '{normalized}/{}' declares app name '{}', but {} declares '{}'.",
                    project_config::SERVICE_CONFIG_FILE,
                    svc_config.app.name,
                    project_config::APP_CONFIG_FILE,
                    app_name,
                ),
            }));
        }

        validate_service(
            &svc_config.service,
            normalized,
            &project_path,
            &mut errors,
            &mut warnings,
            &mut services_info,
            &mut seen_names,
        );
    }

    // Output results
    let valid = errors.is_empty();

    if output::is_json_mode() {
        output::print_json(&serde_json::json!({
            "valid": valid,
            "app": app_name,
            "services": services_info,
            "errors": errors,
            "warnings": warnings,
        }));
    } else {
        if !warnings.is_empty() {
            for w in &warnings {
                let msg = w.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let path = w.get("path").and_then(|v| v.as_str()).unwrap_or("");
                eprintln!("  \u{26a0} {path}: {msg}");
            }
        }
        if !errors.is_empty() {
            for e in &errors {
                let msg = e.get("message").and_then(|v| v.as_str()).unwrap_or("");
                eprintln!("  \u{2717} {msg}");
            }
            output::error(
                &format!("{} error(s) found.", errors.len()),
                "CONFIG_INVALID",
                Some("Fix the errors above and run `floo check` again."),
            );
        } else {
            output::success(
                &format!(
                    "Config valid \u{2014} app '{app_name}' with {} service(s).",
                    services_info.len()
                ),
                None,
            );
        }
    }

    if !valid {
        process::exit(1);
    }
}

fn validate_service(
    service: &project_config::ServiceSection,
    path: &str,
    project_path: &std::path::Path,
    errors: &mut Vec<serde_json::Value>,
    warnings: &mut Vec<serde_json::Value>,
    services_info: &mut Vec<serde_json::Value>,
    seen_names: &mut Vec<String>,
) {
    let name = &service.name;

    // Validate service name
    if let Err(msg) = validate_service_name(name) {
        errors.push(serde_json::json!({
            "path": path,
            "message": msg,
        }));
    }

    // Check for duplicate names
    if seen_names.contains(name) {
        errors.push(serde_json::json!({
            "path": path,
            "message": format!("Duplicate service name '{name}'."),
        }));
    } else {
        seen_names.push(name.clone());
    }

    // Validate port
    if service.port == 0 {
        errors.push(serde_json::json!({
            "path": path,
            "message": format!("Service '{name}' has invalid port 0. Ports must be 1-65535."),
        }));
    }

    // Check env_file exists
    if let Some(ref env_file) = service.env_file {
        let svc_dir = project_path.join(path);
        let env_path = svc_dir.join(env_file);
        if !env_path.exists() {
            warnings.push(serde_json::json!({
                "path": path,
                "message": format!("Service '{name}' env_file '{env_file}' not found on disk."),
            }));
        }
    }

    // Check Dockerfile EXPOSE matches port (best-effort)
    let svc_dir = project_path.join(path);
    let dockerfile = svc_dir.join("Dockerfile");
    if dockerfile.exists() {
        match std::fs::read_to_string(&dockerfile) {
            Ok(content) => {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(expose_val) = trimmed.strip_prefix("EXPOSE ") {
                        let expose_val = expose_val.trim();
                        // Handle EXPOSE port/protocol format
                        let port_str = expose_val.split('/').next().unwrap_or(expose_val);
                        if let Ok(exposed_port) = port_str.parse::<u16>() {
                            if exposed_port != service.port {
                                warnings.push(serde_json::json!({
                                    "path": path,
                                    "message": format!(
                                        "Service '{name}' Dockerfile EXPOSE {exposed_port} does not match configured port {}.",
                                        service.port
                                    ),
                                }));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warnings.push(serde_json::json!({
                    "path": path,
                    "message": format!(
                        "Service '{name}' Dockerfile exists but could not be read: {e}. Port check skipped."
                    ),
                }));
            }
        }
    }

    services_info.push(serde_json::json!({
        "name": name,
        "path": path,
        "port": service.port,
        "type": service.service_type.to_string(),
        "ingress": service.resolved_ingress().to_string(),
    }));
}
