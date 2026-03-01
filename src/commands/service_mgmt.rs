use std::process;

use crate::errors::ErrorCode;
use crate::output;
use crate::project_config::{
    self, validate_service_name, AppServiceEntry, AppServiceType, ServiceFileAppSection,
    ServiceFileConfig, ServiceIngress, ServiceSection, ServiceType,
};

pub fn add(
    name: &str,
    path: &str,
    port: Option<u16>,
    service_type: Option<&str>,
    ingress: Option<&str>,
    env_file: Option<&str>,
) {
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    });

    // Validate service name
    if let Err(msg) = validate_service_name(name) {
        output::error(&msg, &ErrorCode::InvalidServiceName, None);
        process::exit(1);
    }

    // Load existing app config (must exist)
    let mut app_config = match project_config::load_app_config(&cwd) {
        Ok(Some(cfg)) => cfg,
        Ok(None) => {
            output::error(
                &format!("{} not found.", project_config::APP_CONFIG_FILE),
                &ErrorCode::NoConfigFound,
                Some("Run `floo init` first to create config files."),
            );
            process::exit(1);
        }
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // Check for duplicates
    if app_config.services.contains_key(name) {
        output::error(
            &format!(
                "Service '{name}' already exists in {}.",
                project_config::APP_CONFIG_FILE
            ),
            &ErrorCode::DuplicateService,
            Some("Choose a different name or use `floo service rm` first."),
        );
        process::exit(1);
    }

    // Also check if a root floo.service.toml exists with this name
    if let Ok(Some(existing_svc)) = project_config::load_service_config(&cwd) {
        if existing_svc.service.name == name {
            output::error(
                &format!(
                    "Service '{name}' already defined in root {}.",
                    project_config::SERVICE_CONFIG_FILE
                ),
                &ErrorCode::DuplicateService,
                Some("Choose a different name."),
            );
            process::exit(1);
        }
    }

    // Resolve port and type
    let resolved_port = match port {
        Some(p) => p,
        None => {
            if !output::is_interactive() {
                output::error(
                    "--port is required in non-interactive mode.",
                    &ErrorCode::MissingPort,
                    Some("Usage: floo service add <name> <path> --port N --type T"),
                );
                process::exit(1);
            }
            let detection = crate::detection::detect(&cwd.join(path));
            let default = detection.default_port().to_string();
            let port_str = output::prompt_with_default("Port", &default);
            port_str.parse().unwrap_or_else(|_| {
                output::error(
                    &format!("Invalid port number: '{port_str}'."),
                    &ErrorCode::InvalidFormat,
                    Some("Port must be a number between 1 and 65535."),
                );
                process::exit(1);
            })
        }
    };

    let resolved_type_str = match service_type {
        Some(t) => t.to_string(),
        None => {
            if !output::is_interactive() {
                output::error(
                    "--type is required in non-interactive mode.",
                    &ErrorCode::MissingType,
                    Some("Usage: floo service add <name> <path> --port N --type T"),
                );
                process::exit(1);
            }
            let detection = crate::detection::detect(&cwd.join(path));
            let default = detection.default_service_type();
            output::prompt_with_default("Type (web/api/worker)", default)
        }
    };

    let svc_type = match resolved_type_str.as_str() {
        "web" => ServiceType::Web,
        "api" => ServiceType::Api,
        "worker" => ServiceType::Worker,
        other => {
            output::error(
                &format!("Unknown service type '{other}'."),
                &ErrorCode::InvalidType,
                Some("Valid types: web, api, worker."),
            );
            process::exit(1);
        }
    };

    let app_svc_type = match svc_type {
        ServiceType::Api => AppServiceType::Api,
        ServiceType::Worker => AppServiceType::Worker,
        ServiceType::Web => AppServiceType::Web,
    };

    let svc_ingress = match ingress {
        Some("internal") => ServiceIngress::Internal,
        Some("public") | None => ServiceIngress::Public,
        Some(other) => {
            output::error(
                &format!("Unknown ingress mode '{other}'."),
                &ErrorCode::InvalidIngress,
                Some("Valid modes: public, internal."),
            );
            process::exit(1);
        }
    };

    let env_file_val = env_file.map(|s| s.to_string());

    let app_name = app_config.app.name.clone();

    // Add to floo.app.toml services map (only for non-root paths)
    let normalized_path = if path == "." { None } else { Some(path) };

    if let Some(p) = normalized_path {
        let path_str = if p.starts_with("./") {
            p.to_string()
        } else {
            format!("./{p}")
        };
        app_config.services.insert(
            name.to_string(),
            AppServiceEntry {
                service_type: app_svc_type,
                path: Some(path_str),
                repo: None,
                version: None,
                plan: None,
                ingress: None,
            },
        );
    }

    // Write updated app config
    if let Err(e) = project_config::write_app_config(&cwd, &app_config) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }

    // Write floo.service.toml in the service's directory
    let svc_dir = cwd.join(path);
    if let Err(e) = std::fs::create_dir_all(&svc_dir) {
        output::error(
            &format!("Failed to create directory '{}': {e}", path),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    }

    let svc_file = ServiceFileConfig {
        app: ServiceFileAppSection {
            name: app_name,
            access_mode: None,
        },
        service: ServiceSection {
            name: name.to_string(),
            service_type: svc_type,
            port: resolved_port,
            ingress: Some(svc_ingress),
            env_file: env_file_val.clone(),
        },
    };

    if let Err(e) = project_config::write_service_config(&svc_dir, &svc_file) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }

    let svc_toml_path = if path == "." {
        project_config::SERVICE_CONFIG_FILE.to_string()
    } else {
        format!("{path}/{}", project_config::SERVICE_CONFIG_FILE)
    };

    output::success(
        &format!("Added service '{name}'."),
        Some(serde_json::json!({
            "name": name,
            "path": path,
            "port": resolved_port,
            "type": resolved_type_str,
            "ingress": svc_ingress.to_string(),
            "env_file": env_file_val,
            "files_written": [
                project_config::APP_CONFIG_FILE,
                svc_toml_path,
            ],
        })),
    );
}

pub fn rm(name: &str, delete_config: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    });

    let mut app_config = match project_config::load_app_config(&cwd) {
        Ok(Some(cfg)) => cfg,
        Ok(None) => {
            output::error(
                &format!("{} not found.", project_config::APP_CONFIG_FILE),
                &ErrorCode::NoConfigFound,
                Some("No config to modify."),
            );
            process::exit(1);
        }
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // Find the service's path before removing
    let service_path = app_config
        .services
        .get(name)
        .and_then(|e| e.path.as_deref())
        .map(|p| p.strip_prefix("./").unwrap_or(p).to_string());

    // Remove from services map
    if app_config.services.remove(name).is_none() {
        // Check if it's the root service
        if let Ok(Some(svc)) = project_config::load_service_config(&cwd) {
            if svc.service.name == name {
                if delete_config {
                    let config_path = cwd.join(project_config::SERVICE_CONFIG_FILE);
                    if let Err(e) = std::fs::remove_file(&config_path) {
                        output::error(
                            &format!(
                                "Failed to delete {}: {e}",
                                project_config::SERVICE_CONFIG_FILE
                            ),
                            &ErrorCode::FileError,
                            None,
                        );
                        process::exit(1);
                    }
                }
                output::success(&format!("Removed service '{name}'."), None);
                return;
            }
        }

        output::error(
            &format!(
                "Service '{name}' not found in {}.",
                project_config::APP_CONFIG_FILE
            ),
            &ErrorCode::ServiceNotFound,
            None,
        );
        process::exit(1);
    }

    // Write updated config
    if let Err(e) = project_config::write_app_config(&cwd, &app_config) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }

    // Optionally delete the service's floo.service.toml
    if delete_config {
        if let Some(ref svc_path) = service_path {
            let config_path = cwd.join(svc_path).join(project_config::SERVICE_CONFIG_FILE);
            if config_path.exists() {
                if let Err(e) = std::fs::remove_file(&config_path) {
                    output::error(
                        &format!(
                            "Failed to delete {}/{}: {e}",
                            svc_path,
                            project_config::SERVICE_CONFIG_FILE
                        ),
                        &ErrorCode::FileError,
                        None,
                    );
                    process::exit(1);
                }
            }
        }
    }

    output::success(&format!("Removed service '{name}'."), None);
}
