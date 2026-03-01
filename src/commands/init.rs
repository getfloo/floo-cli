use std::collections::HashMap;
use std::path::PathBuf;
use std::process;

use crate::detection::detect;
use crate::errors::ErrorCode;
use crate::names::generate_name;
use crate::output;
use crate::project_config::{
    self, AppFileAppSection, AppFileConfig, AppServiceEntry, AppServiceType, ServiceFileAppSection,
    ServiceFileConfig, ServiceIngress, ServiceSection, ServiceType,
};

pub fn init(name: Option<String>, path: PathBuf) {
    let project_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            output::error(
                &format!("Path '{}' does not exist.", path.display()),
                &ErrorCode::InvalidPath,
                Some("Provide a valid project directory."),
            );
            process::exit(1);
        }
    };

    if !project_path.is_dir() {
        output::error(
            &format!("Path '{}' is not a directory.", path.display()),
            &ErrorCode::InvalidPath,
            Some("Provide a valid project directory."),
        );
        process::exit(1);
    }

    // Error if config already exists
    if project_path.join(project_config::APP_CONFIG_FILE).exists() {
        output::error(
            &format!("{} already exists.", project_config::APP_CONFIG_FILE),
            &ErrorCode::ConfigExists,
            Some("Use `floo service add` to add services to the existing config."),
        );
        process::exit(1);
    }

    let detection = detect(&project_path);

    if output::is_interactive() {
        init_interactive(&project_path, name, &detection);
    } else {
        init_non_interactive(&project_path, name, &detection);
    }
}

fn init_non_interactive(
    project_path: &std::path::Path,
    name: Option<String>,
    detection: &crate::detection::DetectionResult,
) {
    let app_name = match name {
        Some(n) => n,
        None => {
            output::error(
                "App name required in non-interactive mode.",
                &ErrorCode::MissingAppName,
                Some("Usage: floo init <name> [--json]"),
            );
            process::exit(1);
        }
    };

    let default_type = detection.default_service_type();
    let service_type = match default_type {
        "api" => ServiceType::Api,
        _ => ServiceType::Web,
    };

    let env_file = super::detect_env_file(project_path);

    let service_name = default_type.to_string();
    let port = detection.default_port();

    let service_file = ServiceFileConfig {
        app: ServiceFileAppSection {
            name: app_name.clone(),
            access_mode: None,
        },
        service: ServiceSection {
            name: service_name.clone(),
            service_type,
            port,
            ingress: Some(ServiceIngress::Public),
            env_file,
            domain: None,
        },
    };

    let app_file = AppFileConfig {
        app: AppFileAppSection {
            name: app_name.clone(),
            access_mode: None,
        },
        services: HashMap::new(),
        environments: HashMap::new(),
    };

    let mut files_written = Vec::new();

    if let Err(e) = project_config::write_app_config(project_path, &app_file) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }
    files_written.push(project_config::APP_CONFIG_FILE);

    if let Err(e) = project_config::write_service_config(project_path, &service_file) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }
    files_written.push(project_config::SERVICE_CONFIG_FILE);

    output::success(
        &format!("Initialized app '{app_name}'."),
        Some(serde_json::json!({
            "app_name": app_name,
            "files_written": files_written,
            "detection": detection.to_value(),
            "service": {
                "name": service_name,
                "type": service_type.to_string(),
                "port": port,
            },
        })),
    );
}

fn init_interactive(
    project_path: &std::path::Path,
    name: Option<String>,
    detection: &crate::detection::DetectionResult,
) {
    // Show detection info
    if detection.runtime != "unknown" {
        let framework_label = detection
            .framework
            .as_deref()
            .map(|f| format!(" ({f})"))
            .unwrap_or_default();
        output::info(
            &format!(
                "Detected {}{framework_label} \u{2014} {} confidence",
                detection.runtime, detection.confidence
            ),
            None,
        );
    }

    let default_name = name.unwrap_or_else(generate_name);
    let app_name = output::prompt_with_default("App name", &default_name);

    let mut services_map: HashMap<String, AppServiceEntry> = HashMap::new();
    let mut first_service_file: Option<ServiceFileConfig> = None;

    // First service prompt
    if output::confirm("Add a service?") {
        loop {
            let default_svc_name = detection.default_service_type().to_string();
            let svc_name = output::prompt_with_default("Service name", &default_svc_name);

            let default_path = ".".to_string();
            let svc_path = output::prompt_with_default("Service path", &default_path);

            let default_port = detection.default_port().to_string();
            let port_str = output::prompt_with_default("Port", &default_port);
            let port: u16 = port_str.parse().unwrap_or_else(|_| {
                output::error(
                    &format!("Invalid port number: '{port_str}'."),
                    &ErrorCode::InvalidFormat,
                    Some("Port must be a number between 1 and 65535."),
                );
                process::exit(1);
            });

            let default_type = detection.default_service_type();
            let type_str = output::prompt_with_default("Type (web/api/worker)", default_type);
            let service_type = match type_str.as_str() {
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

            let svc_dir = project_path.join(&svc_path);
            let env_file = match super::detect_env_file(&svc_dir) {
                Some(name) => {
                    let use_it =
                        output::confirm(&format!("  {name} detected. Use it for cloud deploy?"));
                    if use_it {
                        Some(name)
                    } else {
                        None
                    }
                }
                None => None,
            };

            let app_service_type = match service_type {
                ServiceType::Api => AppServiceType::Api,
                ServiceType::Worker => AppServiceType::Worker,
                ServiceType::Web => AppServiceType::Web,
            };

            // Only add to app.toml services map if path != "."
            if svc_path != "." {
                services_map.insert(
                    svc_name.clone(),
                    AppServiceEntry {
                        service_type: app_service_type,
                        path: Some(format!("./{svc_path}")),
                        repo: None,
                        version: None,
                        plan: None,
                        ingress: None,
                        domain: None,
                    },
                );
            }

            // Write floo.service.toml for first service (or root service)
            if first_service_file.is_none() && svc_path == "." {
                first_service_file = Some(ServiceFileConfig {
                    app: ServiceFileAppSection {
                        name: app_name.clone(),
                        access_mode: None,
                    },
                    service: ServiceSection {
                        name: svc_name.clone(),
                        service_type,
                        port,
                        ingress: Some(ServiceIngress::Public),
                        env_file: env_file.clone(),
                        domain: None,
                    },
                });
            } else {
                // Write floo.service.toml in subdirectory
                let svc_dir = project_path.join(&svc_path);
                if svc_dir.is_dir() {
                    let svc_file = ServiceFileConfig {
                        app: ServiceFileAppSection {
                            name: app_name.clone(),
                            access_mode: None,
                        },
                        service: ServiceSection {
                            name: svc_name.clone(),
                            service_type,
                            port,
                            ingress: Some(ServiceIngress::Public),
                            env_file: env_file.clone(),
                            domain: None,
                        },
                    };
                    if let Err(e) = project_config::write_service_config(&svc_dir, &svc_file) {
                        output::error(&e.message, &e.code, None);
                        process::exit(1);
                    }
                    output::info(
                        &format!("Wrote {svc_path}/{}", project_config::SERVICE_CONFIG_FILE),
                        None,
                    );
                }
            }

            if !output::confirm("Add another service?") {
                break;
            }
        }
    } else {
        // No explicit service — create a default one
        let default_type = detection.default_service_type();
        let service_type = match default_type {
            "api" => ServiceType::Api,
            _ => ServiceType::Web,
        };
        let env_file = super::detect_env_file(project_path);
        first_service_file = Some(ServiceFileConfig {
            app: ServiceFileAppSection {
                name: app_name.clone(),
                access_mode: None,
            },
            service: ServiceSection {
                name: default_type.to_string(),
                service_type,
                port: detection.default_port(),
                ingress: Some(ServiceIngress::Public),
                env_file,
                domain: None,
            },
        });
    }

    // Write app config
    let app_file = AppFileConfig {
        app: AppFileAppSection {
            name: app_name.clone(),
            access_mode: None,
        },
        services: services_map,
        environments: HashMap::new(),
    };

    if let Err(e) = project_config::write_app_config(project_path, &app_file) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }
    output::info(&format!("Wrote {}", project_config::APP_CONFIG_FILE), None);

    // Write root service config
    if let Some(svc_file) = first_service_file {
        if let Err(e) = project_config::write_service_config(project_path, &svc_file) {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
        output::info(
            &format!("Wrote {}", project_config::SERVICE_CONFIG_FILE),
            None,
        );
    }

    output::success(&format!("Initialized app '{app_name}'."), None);
}
