use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use crate::api_client::FlooClient;
use crate::api_types::Deploy;
use crate::archive::create_archive;
use crate::config::load_config;
use crate::detection::detect;
use crate::errors::{ErrorCode, FlooApiError};
use crate::names::generate_name;
use crate::output;
use crate::project_config::{
    self, AppAccessMode, AppFileAppSection, AppFileConfig, AppSource, ServiceConfig,
    ServiceFileAppSection, ServiceFileConfig, ServiceIngress, ServiceSection, ServiceType,
};
use crate::resolve::resolve_app;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes
pub(crate) const TERMINAL_STATUSES: &[&str] = &["live", "failed"];

fn status_label(status: &str) -> &str {
    match status {
        "pending" => "Queued...",
        "building" => "Building...",
        "deploying" => "Deploying...",
        _ => "Deploying...",
    }
}

pub fn deploy(
    path: PathBuf,
    app: Option<String>,
    services_filter: Vec<String>,
    restart: bool,
    sync_env: bool,
) {
    let config = load_config();
    if config.api_key.is_none() {
        output::error(
            "Not logged in.",
            &ErrorCode::NotAuthenticated,
            Some("Run 'floo login' to authenticate."),
        );
        process::exit(1);
    }

    // --- Restart path: skip detection/archive, call restart API ---
    if restart {
        let client = super::init_client(Some(config));
        let app_ident = match app.as_deref() {
            Some(a) => a.to_string(),
            None => {
                let cwd = std::env::current_dir().unwrap_or_else(|e| {
                    output::error(
                        &format!("Failed to read current directory: {e}"),
                        &ErrorCode::FileError,
                        None,
                    );
                    process::exit(1);
                });
                match project_config::resolve_app_context(&cwd, None) {
                    Ok(r) => r.app_name,
                    Err(e) => {
                        output::error(&e.message, &e.code, e.suggestion.as_deref());
                        process::exit(1);
                    }
                }
            }
        };

        let app_data = match resolve_app(&client, &app_ident) {
            Ok(a) => a,
            Err(e) => {
                if e.code == "APP_NOT_FOUND" {
                    output::error(
                        &format!("App '{app_ident}' not found."),
                        &ErrorCode::AppNotFound,
                        Some("Check the app name or ID and try again."),
                    );
                } else {
                    output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                }
                process::exit(1);
            }
        };
        let app_id = app_data.id.clone();

        let svcs = if services_filter.is_empty() {
            None
        } else {
            Some(services_filter.as_slice())
        };

        let spinner = output::Spinner::new("Restarting...");
        let deploy_data = match client.restart_app(&app_id, svcs) {
            Ok(d) => {
                spinner.finish();
                d
            }
            Err(e) => {
                spinner.finish();
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };

        let final_status = deploy_data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let url = deploy_data
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("(no URL)");

        if final_status == "failed" {
            output::error_with_data(
                "Restart failed.",
                &ErrorCode::RestartFailed,
                Some("Run `floo logs` for details."),
                Some(serde_json::json!({
                    "app": output::to_value(&app_data),
                    "deploy": deploy_data,
                })),
            );
            process::exit(1);
        }

        output::success(
            &format!("Restarted {url}"),
            Some(serde_json::json!({
                "app": output::to_value(&app_data),
                "deploy": deploy_data,
            })),
        );
        return;
    }

    let project_path = match path.canonicalize() {
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

    if !project_path.is_dir() {
        output::error(
            &format!("Path '{}' is not a directory.", path.display()),
            &ErrorCode::InvalidPath,
            Some("Provide a valid project directory."),
        );
        process::exit(1);
    }

    // Resolve app context from config files (before detection, so we know if multi-service)
    let resolved = match project_config::resolve_app_context(&project_path, app.as_deref()) {
        Ok(r) => Some(r),
        Err(e) if e.code == ErrorCode::NoConfigFound => {
            if !output::is_interactive() {
                output::error(
                    "No floo.app.toml or floo.service.toml found.",
                    &ErrorCode::NoConfigFound,
                    Some("Run `floo init` to create config files."),
                );
                process::exit(1);
            }
            None
        }
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // Detect runtime/framework (needed for API call metadata and first-deploy prompts)
    let detection = detect(&project_path);
    let has_config = resolved.is_some();
    if detection.runtime == "unknown" && !has_config {
        output::error(
            "No supported project files found.",
            &ErrorCode::NoRuntimeDetected,
            Some("Add a package.json, requirements.txt, or Dockerfile to your project."),
        );
        process::exit(1);
    }

    if !output::is_json_mode() && detection.runtime != "unknown" {
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

    if detection.confidence == "low"
        && !has_config
        && !output::confirm("Continue with this detection?")
    {
        process::exit(0);
    }

    // Display config info for resolved apps
    if !output::is_json_mode() {
        if let Some(ref r) = resolved {
            let source_label = match r.source {
                AppSource::Flag => "--app flag".to_string(),
                AppSource::ServiceFile => {
                    format!(
                        "{} in {}",
                        project_config::SERVICE_CONFIG_FILE,
                        r.config_dir.display()
                    )
                }
                AppSource::AppFile => {
                    format!(
                        "{} in {}",
                        project_config::APP_CONFIG_FILE,
                        r.config_dir.display()
                    )
                }
            };
            if let Some(ref svc) = r.service_config {
                output::info(
                    &format!(
                        "App '{}' (from {}) \u{2014} service '{}' ({}, :{}, {})",
                        r.app_name,
                        source_label,
                        svc.service.name,
                        svc.service.service_type,
                        svc.service.port,
                        svc.service.resolved_ingress()
                    ),
                    None,
                );
            } else if let Some(ref app_cfg) = r.app_config {
                let svc_count = app_cfg.services.len();
                if svc_count > 0 {
                    output::info(
                        &format!(
                            "App '{}' (from {}) \u{2014} {} service(s) defined",
                            r.app_name, source_label, svc_count
                        ),
                        None,
                    );
                } else {
                    output::info(
                        &format!("App '{}' (from {})", r.app_name, source_label),
                        None,
                    );
                }
            } else {
                output::info(
                    &format!("App '{}' (from {})", r.app_name, source_label),
                    None,
                );
            }
        }
    }

    // Build the app name and services list
    let (app_name, services, write_configs_on_success) = match resolved {
        Some(ref r) => {
            let all_services = match project_config::discover_services(r) {
                Ok(svcs) => svcs,
                Err(e) => {
                    output::error(&e.message, &e.code, e.suggestion.as_deref());
                    process::exit(1);
                }
            };
            let filtered = match project_config::filter_services(all_services, &services_filter) {
                Ok(svcs) => svcs,
                Err(e) => {
                    output::error(&e.message, &e.code, e.suggestion.as_deref());
                    process::exit(1);
                }
            };
            (r.app_name.clone(), Some(filtered), false)
        }
        None => {
            if !services_filter.is_empty() {
                output::error(
                    "--services requires config files.",
                    &ErrorCode::NoConfigFound,
                    Some("Create floo.app.toml with service entries before using --services."),
                );
                process::exit(1);
            }
            let prompted = prompt_first_deploy(&detection);
            (prompted.app_name, Some(vec![prompted.service]), true)
        }
    };

    // Display per-service summary for multi-service deploys
    if !output::is_json_mode() {
        if let Some(ref svcs) = services {
            if svcs.len() > 1 {
                let names: Vec<&str> = svcs.iter().map(|s| s.name.as_str()).collect();
                output::info(
                    &format!("Deploying {} services: {}", svcs.len(), names.join(", ")),
                    None,
                );
            }
        }
    }

    // Create archive
    let spinner = output::Spinner::new("Packaging source...");
    let archive_path = match create_archive(&project_path) {
        Ok(p) => {
            spinner.finish();
            p
        }
        Err(e) => {
            spinner.finish();
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let client = super::init_client(Some(config));

    // Resolve or create app via API
    let app_data = if let Some(ref r) = resolved {
        if matches!(r.source, AppSource::Flag) {
            // --app flag: look up existing app
            let app_ident = &r.app_name;
            let spinner = output::Spinner::new("Looking up app...");
            let result = match resolve_app(&client, app_ident) {
                Ok(app_data) => app_data,
                Err(error) => {
                    spinner.finish();
                    cleanup(&archive_path);
                    if error.code == "APP_NOT_FOUND" {
                        output::error(
                            &format!("App '{app_ident}' not found."),
                            &ErrorCode::AppNotFound,
                            Some("Check the app name or ID and try again."),
                        );
                    } else {
                        output::error(&error.message, &ErrorCode::from_api(&error.code), None);
                    }
                    process::exit(1);
                }
            };
            spinner.finish();
            result
        } else {
            // Config file: look up by name, create if not found
            let spinner = output::Spinner::new(&format!("Looking up app {}...", r.app_name));
            match resolve_app(&client, &r.app_name) {
                Ok(app_data) => {
                    spinner.finish();
                    app_data
                }
                Err(error) if error.code == "APP_NOT_FOUND" => {
                    spinner.finish();
                    let spinner = output::Spinner::new(&format!("Creating app {}...", r.app_name));
                    match client.create_app(&r.app_name, Some(&detection.runtime)) {
                        Ok(a) => {
                            spinner.finish();
                            a
                        }
                        Err(e) => {
                            spinner.finish();
                            cleanup(&archive_path);
                            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                            process::exit(1);
                        }
                    }
                }
                Err(error) => {
                    spinner.finish();
                    cleanup(&archive_path);
                    output::error(&error.message, &ErrorCode::from_api(&error.code), None);
                    process::exit(1);
                }
            }
        }
    } else {
        // First-deploy: create new app
        let spinner = output::Spinner::new(&format!("Creating app {app_name}..."));
        match client.create_app(&app_name, Some(&detection.runtime)) {
            Ok(a) => {
                spinner.finish();
                a
            }
            Err(e) => {
                spinner.finish();
                cleanup(&archive_path);
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        }
    };
    let app_id = app_data.id.clone();

    // Auto-import env vars on first deploy (or force with --sync-env)
    if let Some(ref r) = resolved {
        sync_env_vars_if_needed(&client, &app_id, r, sync_env);
    }

    // Extract access_mode from config: app_config wins over service_config
    let access_mode: Option<AppAccessMode> = resolved.as_ref().and_then(|r| {
        r.app_config
            .as_ref()
            .and_then(|c| c.app.access_mode)
            .or_else(|| r.service_config.as_ref().and_then(|c| c.app.access_mode))
    });

    // Deploy
    let svc_slice = services.as_deref();
    let spinner = output::Spinner::new("Uploading...");
    let mut deploy_data = match client.create_deploy(
        &app_id,
        &archive_path,
        &detection.runtime,
        detection.framework.as_deref(),
        svc_slice,
        access_mode.as_ref().map(|m| m.as_str()),
    ) {
        Ok(d) => {
            spinner.finish();
            d
        }
        Err(e) => {
            spinner.finish();
            cleanup(&archive_path);
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    // Clean up archive immediately after upload
    cleanup(&archive_path);

    // Wait for deploy to complete via SSE streaming or polling
    let initial_status = deploy_data.status.as_deref().unwrap_or("");

    if TERMINAL_STATUSES.contains(&initial_status) {
        // Phase 1: deploy already complete synchronously, skip streaming/polling
    } else if !output::is_json_mode() {
        // Phase 2 human mode: try SSE streaming, fall back to polling
        let deploy_id = deploy_data.id.clone();
        match stream_deploy(&client, &app_id, &deploy_id) {
            Ok(final_data) => deploy_data = final_data,
            Err(e) => {
                // SSE failed — fall back to polling
                eprintln!(
                    "Stream unavailable ({}), falling back to polling...",
                    e.code
                );
                deploy_data = poll_deploy(&client, &app_id, &deploy_data);
            }
        }
    } else {
        // Phase 2 JSON mode: stream structured NDJSON events via SSE
        let deploy_id = deploy_data.id.clone();
        match stream_deploy_json(&client, &app_id, &deploy_id) {
            Ok(final_data) => deploy_data = final_data,
            Err(_) => deploy_data = poll_deploy(&client, &app_id, &deploy_data),
        }
    }

    let final_status = deploy_data.status.as_deref().unwrap_or("");

    if final_status == "failed" {
        let build_logs = deploy_data.build_logs.as_deref().unwrap_or("");
        output::error_with_data(
            "Deploy failed.",
            &ErrorCode::DeployFailed,
            Some("Check build output above, or run `floo logs` for details."),
            Some(serde_json::json!({
                "app": output::to_value(&app_data),
                "deploy": output::to_value(&deploy_data),
                "build_logs": build_logs,
            })),
        );
        process::exit(1);
    }

    // Write config files on first deploy success
    if write_configs_on_success {
        if let Some(ref svcs) = services {
            if let Some(svc) = svcs.first() {
                write_first_deploy_configs(&project_path, &app_name, svc);
            }
        }
    }

    let url = deploy_data.url.as_deref().unwrap_or("");

    let service_names: Vec<&str> = services
        .as_ref()
        .map(|svcs| svcs.iter().map(|s| s.name.as_str()).collect())
        .unwrap_or_default();

    output::success(
        &format!("Deployed to {url}"),
        Some(serde_json::json!({
            "app": output::to_value(&app_data),
            "deploy": output::to_value(&deploy_data),
            "detection": detection.to_value(),
            "services": service_names,
        })),
    );
}

struct FirstDeployResult {
    app_name: String,
    service: ServiceConfig,
}

fn prompt_first_deploy(detection: &crate::detection::DetectionResult) -> FirstDeployResult {
    let default_name = generate_name();
    let app_name = output::prompt_with_default("App name", &default_name);

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
    let service_type = match default_type {
        "api" => ServiceType::Api,
        _ => ServiceType::Web,
    };

    FirstDeployResult {
        app_name,
        service: ServiceConfig {
            name: default_type.to_string(),
            service_type,
            path: ".".to_string(),
            port,
            ingress: ServiceIngress::Public,
            domain: None,
        },
    }
}

fn write_first_deploy_configs(project_path: &Path, app_name: &str, service: &ServiceConfig) {
    let env_file = super::detect_env_file(project_path);

    let service_file = ServiceFileConfig {
        app: ServiceFileAppSection {
            name: app_name.to_string(),
            access_mode: None,
        },
        service: ServiceSection {
            name: service.name.clone(),
            service_type: service.service_type,
            port: service.port,
            ingress: Some(service.ingress),
            env_file,
            domain: None,
        },
    };

    let app_file = AppFileConfig {
        app: AppFileAppSection {
            name: app_name.to_string(),
            access_mode: None,
        },
        services: HashMap::new(),
    };

    if let Err(e) = project_config::write_app_config(project_path, &app_file) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }
    if !output::is_json_mode() {
        output::info(&format!("Wrote {}", project_config::APP_CONFIG_FILE), None);
    }

    if let Err(e) = project_config::write_service_config(project_path, &service_file) {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }
    if !output::is_json_mode() {
        output::info(
            &format!("Wrote {}", project_config::SERVICE_CONFIG_FILE),
            None,
        );
    }
}

/// Stream deploy logs via SSE and return the final deploy state.
pub(crate) fn stream_deploy(
    client: &FlooClient,
    app_id: &str,
    deploy_id: &str,
) -> Result<Deploy, FlooApiError> {
    let response = client.stream_deploy_logs(app_id, deploy_id)?;
    let reader = std::io::BufReader::new(response);

    let mut event_type = String::new();
    let mut data_buf = String::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("SSE connection error: {e}");
                break;
            }
        };

        if let Some(suffix) = line.strip_prefix("event: ") {
            event_type = suffix.to_string();
        } else if let Some(suffix) = line.strip_prefix("data: ") {
            data_buf = suffix.to_string();
        } else if line.starts_with(':') {
            continue; // SSE comment (heartbeat)
        } else if line.is_empty() && !event_type.is_empty() {
            // Event complete — process it
            match event_type.as_str() {
                "status" => match serde_json::from_str::<serde_json::Value>(&data_buf) {
                    Ok(parsed) => {
                        let status = parsed.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        output::bold_line(status_label(status));
                    }
                    Err(e) => eprintln!("Malformed SSE status event: {e}"),
                },
                "log" => match serde_json::from_str::<serde_json::Value>(&data_buf) {
                    Ok(parsed) => {
                        if let Some(text) = parsed.get("text").and_then(|v| v.as_str()) {
                            for log_line in text.trim().lines() {
                                output::dim_line(log_line);
                            }
                        }
                    }
                    Err(e) => eprintln!("Malformed SSE log event: {e}"),
                },
                "done" => {
                    break;
                }
                "error" => match serde_json::from_str::<serde_json::Value>(&data_buf) {
                    Ok(parsed) => {
                        let msg = parsed
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Stream error");
                        return Err(FlooApiError::new(0, "STREAM_ERROR", msg));
                    }
                    Err(e) => {
                        eprintln!("Malformed SSE error event: {e}");
                        break;
                    }
                },
                _ => {}
            }
            event_type.clear();
            data_buf.clear();
        }
    }

    // After stream ends, fetch final deploy state for success/error output
    client.get_deploy(app_id, deploy_id)
}

/// Stream deploy events via SSE and emit NDJSON to stdout for JSON mode.
pub(crate) fn stream_deploy_json(
    client: &FlooClient,
    app_id: &str,
    deploy_id: &str,
) -> Result<Deploy, FlooApiError> {
    let response = client.stream_deploy_logs(app_id, deploy_id)?;
    let reader = std::io::BufReader::new(response);

    let mut event_type = String::new();
    let mut data_buf = String::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };

        if let Some(suffix) = line.strip_prefix("event: ") {
            event_type = suffix.to_string();
        } else if let Some(suffix) = line.strip_prefix("data: ") {
            data_buf = suffix.to_string();
        } else if line.starts_with(':') {
            continue;
        } else if line.is_empty() && !event_type.is_empty() {
            match event_type.as_str() {
                "status" => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data_buf) {
                        let status = parsed.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        output::print_json(
                            &serde_json::json!({"event": "status", "status": status}),
                        );
                    }
                }
                "log" => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data_buf) {
                        if let Some(text) = parsed.get("text").and_then(|v| v.as_str()) {
                            output::print_json(&serde_json::json!({"event": "log", "text": text}));
                        }
                    }
                }
                "done" => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data_buf) {
                        let status = parsed.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        let url = parsed.get("url").and_then(|v| v.as_str()).unwrap_or("");
                        output::print_json(
                            &serde_json::json!({"event": "done", "status": status, "url": url}),
                        );
                    }
                    break;
                }
                "error" => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data_buf) {
                        let msg = parsed
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Stream error");
                        return Err(FlooApiError::new(0, "STREAM_ERROR", msg));
                    }
                    break;
                }
                _ => {}
            }
            event_type.clear();
            data_buf.clear();
        }
    }

    client.get_deploy(app_id, deploy_id)
}

/// Poll the deploy endpoint until it reaches a terminal status.
pub(crate) fn poll_deploy(client: &FlooClient, app_id: &str, initial_data: &Deploy) -> Deploy {
    let deploy_id = initial_data.id.clone();
    let poll_start = Instant::now();
    let mut last_log_len: usize = 0;
    let mut deploy_data = initial_data.clone();

    while !TERMINAL_STATUSES.contains(&deploy_data.status.as_deref().unwrap_or("")) {
        if !output::is_json_mode() {
            let build_logs = deploy_data.build_logs.as_deref().unwrap_or("");
            if build_logs.len() > last_log_len {
                let new_logs = &build_logs[last_log_len..];
                for line in new_logs.trim().lines() {
                    output::dim_line(line);
                }
                last_log_len = build_logs.len();
            }

            let status = deploy_data.status.as_deref().unwrap_or("");
            output::bold_line(status_label(status));
        }

        thread::sleep(POLL_INTERVAL);

        if poll_start.elapsed() >= POLL_TIMEOUT {
            output::error(
                "Deploy timed out after 10 minutes",
                &ErrorCode::DeployTimeout,
                Some(&format!(
                    "The deploy may still complete — check status with \
                     `floo apps status {app_id}` (deploy ID: {deploy_id})"
                )),
            );
            process::exit(1);
        }

        deploy_data = match client.get_deploy(app_id, &deploy_id) {
            Ok(d) => d,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };
    }

    // Print any remaining build logs for the final state
    if !output::is_json_mode() {
        let build_logs = deploy_data.build_logs.as_deref().unwrap_or("");
        if build_logs.len() > last_log_len {
            let new_logs = &build_logs[last_log_len..];
            for line in new_logs.trim().lines() {
                output::dim_line(line);
            }
        }
    }

    deploy_data
}

/// Non-fatal .env parser for the deploy path. Returns None on errors instead of exiting.
/// Separate from env.rs::parse_env_file because the deploy path is best-effort.
fn parse_env_file_soft(path: &Path) -> Option<Vec<(String, String)>> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return None,
    };

    let mut vars = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim().to_uppercase();
            let mut value = value.trim().to_string();
            if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value = value[1..value.len() - 1].to_string();
            }
            vars.push((key, value));
        }
    }

    if vars.is_empty() {
        return None;
    }

    Some(vars)
}

/// Auto-import env vars from configured env_file on first deploy (server has 0 vars),
/// or when --sync-env is passed. Reads env_file from service configs (source of truth).
pub(crate) fn sync_env_vars_if_needed(
    client: &FlooClient,
    app_id: &str,
    resolved: &project_config::ResolvedApp,
    force_sync: bool,
) {
    // Collect (service_name, env_file_path) from env_file field in service configs
    let mut env_file_entries: Vec<(String, PathBuf)> = Vec::new();

    // Check root service
    if let Some(ref svc_config) = resolved.service_config {
        if let Some(ref env_file) = svc_config.service.env_file {
            let path = resolved.config_dir.join(env_file);
            env_file_entries.push((svc_config.service.name.clone(), path));
        }
    }

    // Check sub-services from app config
    if let Some(ref app_config) = resolved.app_config {
        for entry in app_config.services.values() {
            if let Some(ref path_str) = entry.path {
                let normalized = path_str.strip_prefix("./").unwrap_or(path_str);
                let normalized = normalized.strip_suffix('/').unwrap_or(normalized);
                if normalized.is_empty() || normalized == "." {
                    continue;
                }
                let svc_dir = resolved.config_dir.join(normalized);
                if let Ok(Some(svc_config)) = project_config::load_service_config(&svc_dir) {
                    if let Some(ref env_file) = svc_config.service.env_file {
                        let path = svc_dir.join(env_file);
                        env_file_entries.push((svc_config.service.name.clone(), path));
                    }
                }
            }
        }
    }

    if env_file_entries.is_empty() {
        return;
    }

    // Get server-side services — silently return on API error (services may not exist on first deploy)
    let server_services = match client.list_services(app_id) {
        Ok(r) => r.services,
        Err(_) => return,
    };

    for (svc_name, env_file_path) in &env_file_entries {
        let server_svc = server_services.iter().find(|s| s.name == *svc_name);

        let Some(svc) = server_svc else { continue };
        let svc_id = &svc.id;

        // Check env var count on server
        let env_count = match client.list_env_vars(app_id, Some(svc_id)) {
            Ok(r) => r.env_vars.len(),
            Err(_) => continue,
        };

        // Skip if already has vars and not force-syncing
        if !force_sync && env_count > 0 {
            continue;
        }

        if !env_file_path.exists() {
            let file_name = env_file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("env file");
            output::warn(&format!(
                "Service '{svc_name}' has env_file configured but {file_name} not found on disk."
            ));
            continue;
        }

        let vars = match parse_env_file_soft(env_file_path) {
            Some(v) => v,
            None => continue,
        };

        let count = vars.len();
        let file_name = env_file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("env file");

        if !output::is_json_mode() {
            output::info(
                &format!(
                    "Importing {count} env var(s) for service '{svc_name}' from {file_name}..."
                ),
                None,
            );
        }

        if let Err(e) = client.import_env_vars(app_id, &vars, Some(svc_id)) {
            output::warn(&format!(
                "Failed to import env vars for service '{svc_name}': {}",
                e.message
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_parse_env_file_soft_basic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".floo.env");
        fs::write(&path, "KEY=value\nOTHER=123\n").unwrap();
        let vars = parse_env_file_soft(&path).unwrap();
        assert_eq!(
            vars,
            vec![
                ("KEY".to_string(), "value".to_string()),
                ("OTHER".to_string(), "123".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_env_file_soft_missing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.env");
        assert!(parse_env_file_soft(&path).is_none());
    }

    #[test]
    fn test_parse_env_file_soft_empty_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "").unwrap();
        assert!(parse_env_file_soft(&path).is_none());
    }

    #[test]
    fn test_parse_env_file_soft_skips_malformed() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "GOOD=value\nBADLINE\n# comment\nALSO_GOOD=123\n").unwrap();
        let vars = parse_env_file_soft(&path).unwrap();
        assert_eq!(
            vars,
            vec![
                ("GOOD".to_string(), "value".to_string()),
                ("ALSO_GOOD".to_string(), "123".to_string()),
            ]
        );
    }
}

pub(crate) fn cleanup(path: &PathBuf) {
    if path.exists() {
        if let Err(error) = std::fs::remove_file(path) {
            if !output::is_json_mode() {
                eprintln!(
                    "Warning: failed to remove temporary archive {}: {error}",
                    path.display()
                );
            }
        }
    }
}
