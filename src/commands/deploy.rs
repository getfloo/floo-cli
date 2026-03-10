use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use crate::api_client::FlooClient;
use crate::api_types::Deploy;
use crate::config::load_config;
use crate::detection::{detect_for_services, DetectionResult};
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;
use crate::project_config::{self, validate_service_name, AppAccessMode, AppSource, ServiceConfig};
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
    // --- Restart path: skip detection, needs auth upfront ---
    if restart {
        let config = load_config();
        if config.api_key.is_none() {
            output::error(
                "Not logged in.",
                &ErrorCode::NotAuthenticated,
                Some("Run 'floo login' to authenticate."),
            );
            process::exit(1);
        }

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

        if output::is_dry_run_mode() {
            let service_names: Vec<&str> = services_filter.iter().map(|s| s.as_str()).collect();
            output::dry_run_success(serde_json::json!({
                "action": "restart",
                "app": app_ident,
                "services": service_names,
            }));
            return;
        }

        let client = super::init_client(Some(config));
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

    // ===== Deploy preflight (no auth required) =====

    // 1. Canonicalize path
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

    // 2. Resolve app context
    let resolved = match project_config::resolve_app_context(&project_path, app.as_deref()) {
        Ok(r) => r,
        Err(e) if e.code == ErrorCode::NoConfigFound => {
            output::error(
                "No floo.app.toml or floo.service.toml found.",
                &ErrorCode::NoConfigFound,
                Some("Run 'floo init' to create config files, then 'floo apps github connect <repo>' to connect to GitHub."),
            );
            process::exit(1);
        }
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let app_name = resolved.app_name.clone();

    // 3. Discover + filter services
    let all_services = match project_config::discover_services(&resolved) {
        Ok(svcs) => svcs,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };
    let services = match project_config::filter_services(all_services, &services_filter) {
        Ok(svcs) => svcs,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // 4. Per-service runtime detection
    let svc_pairs: Vec<(&str, &str)> = services
        .iter()
        .map(|s| (s.name.as_str(), s.path.as_str()))
        .collect();
    let (primary_detection, per_service_detection) = detect_for_services(&project_path, &svc_pairs);

    // 5. Validate per-service (port, name, Dockerfile EXPOSE, env_file)
    let (preflight_errors, preflight_warnings) =
        validate_preflight(&project_path, &services, &resolved);

    // 6. Display preflight info
    if !output::is_json_mode() {
        display_preflight_human(
            &app_name,
            &resolved,
            &services,
            &per_service_detection,
            &preflight_warnings,
            &preflight_errors,
        );
    }

    // 7. Dry-run exit — full preflight output, no auth needed
    if output::is_dry_run_mode() {
        let svc_json: Vec<serde_json::Value> = services
            .iter()
            .zip(per_service_detection.iter())
            .map(|(svc, (_, det))| {
                serde_json::json!({
                    "name": svc.name,
                    "path": svc.path,
                    "port": svc.port,
                    "type": svc.service_type.to_string(),
                    "ingress": svc.ingress.to_string(),
                    "runtime": det.runtime,
                    "framework": det.framework,
                    "confidence": det.confidence,
                })
            })
            .collect();

        let warning_strings: Vec<&str> = preflight_warnings
            .iter()
            .map(|w| w.get("message").and_then(|v| v.as_str()).unwrap_or(""))
            .collect();

        output::dry_run_success(serde_json::json!({
            "action": "deploy",
            "app": app_name,
            "services": svc_json,
            "warnings": warning_strings,
            "valid": preflight_errors.is_empty(),
        }));
        return;
    }

    // 8. Auth check — only needed for actual deploy
    let config = load_config();
    if config.api_key.is_none() {
        output::error(
            "Not logged in.",
            &ErrorCode::NotAuthenticated,
            Some("Run 'floo login' to authenticate."),
        );
        process::exit(1);
    }

    // 9. Fail if preflight has errors
    if !preflight_errors.is_empty() {
        let count = preflight_errors.len();
        for e in &preflight_errors {
            let msg = e.get("message").and_then(|v| v.as_str()).unwrap_or("");
            if !output::is_json_mode() {
                eprintln!("  \u{2717} {msg}");
            }
        }
        output::error(
            &format!("{count} preflight error(s) found."),
            &ErrorCode::ConfigInvalid,
            Some("Fix the errors above and run `floo deploy --dry-run` to validate."),
        );
        process::exit(1);
    }

    // Use primary detection for API call metadata
    let detection = primary_detection;

    let client = super::init_client(Some(config));

    // Resolve or create app via API
    let app_data = if matches!(resolved.source, AppSource::Flag) {
        // --app flag: look up existing app
        let app_ident = &resolved.app_name;
        let spinner = output::Spinner::new("Looking up app...");
        let result = match resolve_app(&client, app_ident) {
            Ok(app_data) => app_data,
            Err(error) => {
                spinner.finish();
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
        let spinner = output::Spinner::new(&format!("Looking up app {}...", resolved.app_name));
        match resolve_app(&client, &resolved.app_name) {
            Ok(app_data) => {
                spinner.finish();
                app_data
            }
            Err(error) if error.code == "APP_NOT_FOUND" => {
                spinner.finish();
                let spinner =
                    output::Spinner::new(&format!("Creating app {}...", resolved.app_name));
                match client.create_app(&resolved.app_name, Some(&detection.runtime)) {
                    Ok(a) => {
                        spinner.finish();
                        a
                    }
                    Err(e) => {
                        spinner.finish();
                        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                        process::exit(1);
                    }
                }
            }
            Err(error) => {
                spinner.finish();
                output::error(&error.message, &ErrorCode::from_api(&error.code), None);
                process::exit(1);
            }
        }
    };
    let app_id = app_data.id.clone();

    // Auto-import env vars on first deploy (or force with --sync-env)
    sync_env_vars_if_needed(&client, &app_id, &resolved, sync_env);

    // Extract access_mode: [environments.dev] override > [app] level > service_config
    let access_mode: Option<AppAccessMode> = resolved
        .app_config
        .as_ref()
        .and_then(|c| {
            c.environments
                .get("dev")
                .and_then(|env| env.access_mode)
                .or(c.app.access_mode)
        })
        .or_else(|| {
            resolved
                .service_config
                .as_ref()
                .and_then(|c| c.app.access_mode)
        });

    // Deploy
    let svc_slice = Some(services.as_slice());
    let spinner = output::Spinner::new("Deploying...");
    let mut deploy_data = match client.create_deploy(
        &app_id,
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
            let suggestion = match e.code.as_str() {
                "PLAN_FEATURE_PASSWORD" | "PLAN_FEATURE_FLOO_ACCOUNTS" => {
                    Some("Upgrade your plan at https://app.getfloo.com/settings/billing")
                }
                _ => None,
            };
            output::error(&e.message, &ErrorCode::from_api(&e.code), suggestion);
            process::exit(1);
        }
    };

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

    let url = deploy_data.url.as_deref().unwrap_or("");

    if !output::is_json_mode() {
        if let Some(ref password) = deploy_data.generated_password {
            output::info(&format!("  Generated password: {password}"), None);
            output::info("  To retrieve later: floo apps password <name>", None);
        }
        if let Some(ref mode) = access_mode {
            output::info(&format!("  Access: {}", mode.as_str()), None);
        }
    }

    let service_names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();

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

/// Validate services for common config errors. Returns (errors, warnings).
/// Absorbs the validation logic that was previously in `floo check`.
fn validate_preflight(
    project_path: &Path,
    services: &[ServiceConfig],
    resolved: &project_config::ResolvedApp,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut warnings: Vec<serde_json::Value> = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();

    for svc in services {
        // Validate service name
        if let Err(msg) = validate_service_name(&svc.name) {
            errors.push(serde_json::json!({
                "path": svc.path,
                "message": msg,
            }));
        }

        // Check for duplicate names
        if seen_names.contains(&svc.name) {
            errors.push(serde_json::json!({
                "path": svc.path,
                "message": format!("Duplicate service name '{}'.", svc.name),
            }));
        } else {
            seen_names.push(svc.name.clone());
        }

        // Validate port
        if svc.port == 0 {
            errors.push(serde_json::json!({
                "path": svc.path,
                "message": format!("Service '{}' has invalid port 0. Ports must be 1-65535.", svc.name),
            }));
        }

        let svc_dir = project_path.join(&svc.path);

        // Check env_file exists
        if let Some(ref app_cfg) = resolved.app_config {
            if let Some(entry) = app_cfg.services.get(&svc.name) {
                if let Some(ref env_file) = entry.env_file {
                    let env_path = svc_dir.join(env_file);
                    if !env_path.exists() {
                        warnings.push(serde_json::json!({
                            "path": svc.path,
                            "message": format!("Service '{}' env_file '{env_file}' not found on disk.", svc.name),
                        }));
                    }
                }
            }
        }

        // Check Dockerfile EXPOSE matches port
        let dockerfile = svc_dir.join("Dockerfile");
        if dockerfile.exists() {
            match std::fs::read_to_string(&dockerfile) {
                Ok(content) => {
                    for line in content.lines() {
                        let trimmed = line.trim();
                        if let Some(expose_val) = trimmed.strip_prefix("EXPOSE ") {
                            let expose_val = expose_val.trim();
                            let port_str = expose_val.split('/').next().unwrap_or(expose_val);
                            if let Ok(exposed_port) = port_str.parse::<u16>() {
                                if exposed_port != svc.port {
                                    warnings.push(serde_json::json!({
                                        "path": svc.path,
                                        "message": format!(
                                            "Service '{}' Dockerfile EXPOSE {exposed_port} does not match configured port {}.",
                                            svc.name, svc.port
                                        ),
                                    }));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warnings.push(serde_json::json!({
                        "path": svc.path,
                        "message": format!(
                            "Service '{}' Dockerfile exists but could not be read: {e}. Port check skipped.",
                            svc.name
                        ),
                    }));
                }
            }
        }
    }

    (errors, warnings)
}

/// Display preflight info in human-readable format.
fn display_preflight_human(
    app_name: &str,
    resolved: &project_config::ResolvedApp,
    services: &[ServiceConfig],
    per_service_detection: &[(String, DetectionResult)],
    warnings: &[serde_json::Value],
    errors: &[serde_json::Value],
) {
    let source_label = match resolved.source {
        AppSource::Flag => "--app flag".to_string(),
        AppSource::ServiceFile => {
            format!(
                "{} in {}",
                project_config::SERVICE_CONFIG_FILE,
                resolved.config_dir.display()
            )
        }
        AppSource::AppFile => {
            format!(
                "{} in {}",
                project_config::APP_CONFIG_FILE,
                resolved.config_dir.display()
            )
        }
    };

    eprintln!();
    eprintln!("  App '{}' (from {})", app_name, source_label);

    if services.len() > 1 {
        let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        eprintln!(
            "  Deploying {} services: {}",
            services.len(),
            names.join(", ")
        );
    }
    eprintln!();

    for (svc, (_, det)) in services.iter().zip(per_service_detection.iter()) {
        let path_label = if svc.path == "." {
            String::new()
        } else {
            format!(" (./{path})", path = svc.path)
        };
        eprintln!("  {}{path_label}", svc.name);
        eprintln!(
            "    type: {}, port: {}, ingress: {}",
            svc.service_type, svc.port, svc.ingress
        );

        let framework_label = det
            .framework
            .as_deref()
            .map(|f| format!(" ({f})"))
            .unwrap_or_default();
        eprintln!(
            "    runtime: {}{framework_label} \u{2014} {} confidence",
            det.runtime, det.confidence
        );
        eprintln!();
    }

    // Show global [resources] if present
    if let Some(ref app_cfg) = resolved.app_config {
        if let Some(ref res) = app_cfg.resources {
            let has_any = res.cpu.is_some() || res.memory.is_some() || res.max_instances.is_some();
            if has_any {
                eprintln!("  [resources] (global defaults)");
                let mut parts = Vec::new();
                if let Some(ref cpu) = res.cpu {
                    parts.push(format!("cpu: {cpu}"));
                }
                if let Some(ref memory) = res.memory {
                    parts.push(format!("memory: {memory}"));
                }
                if let Some(max) = res.max_instances {
                    parts.push(format!("max_instances: {max}"));
                }
                eprintln!("    {}", parts.join(", "));
                eprintln!();
            }
        }
    }

    for w in warnings {
        let msg = w.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let path = w.get("path").and_then(|v| v.as_str()).unwrap_or("");
        eprintln!("  \u{26a0} {path}: {msg}");
    }

    if errors.is_empty() {
        output::success("Config valid \u{2014} ready to deploy.", None);
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
