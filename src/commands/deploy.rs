use std::io::BufRead;
use std::path::PathBuf;
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use crate::api_client::FlooClient;
use crate::archive::create_archive;
use crate::config::load_config;
use crate::detection::detect;
use crate::errors::FlooApiError;
use crate::names::generate_name;
use crate::output;
use crate::project_config;
use crate::resolve::resolve_app;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes
const TERMINAL_STATUSES: &[&str] = &["live", "failed"];

fn status_label(status: &str) -> &str {
    match status {
        "pending" => "Queued...",
        "building" => "Building...",
        "deploying" => "Deploying...",
        _ => "Deploying...",
    }
}

fn required_response_id<'a>(value: &'a serde_json::Value, object_name: &str) -> &'a str {
    match value.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id,
        _ => {
            output::error(
                &format!("Unexpected API response: {object_name} is missing required 'id'."),
                "INVALID_RESPONSE",
                Some("This may indicate a CLI/API mismatch. Check for updates with `floo update`."),
            );
            process::exit(1);
        }
    }
}

pub fn deploy(path: PathBuf, name: Option<String>, app: Option<String>) {
    let config = load_config();
    if config.api_key.is_none() {
        output::error(
            "Not logged in.",
            "NOT_AUTHENTICATED",
            Some("Run 'floo login' to authenticate."),
        );
        process::exit(1);
    }

    let project_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            output::error(
                &format!("Path '{}' is not a directory.", path.display()),
                "INVALID_PATH",
                Some("Provide a valid project directory."),
            );
            process::exit(1);
        }
    };

    if !project_path.is_dir() {
        output::error(
            &format!("Path '{}' is not a directory.", path.display()),
            "INVALID_PATH",
            Some("Provide a valid project directory."),
        );
        process::exit(1);
    }

    // Load project config if present
    let project_config = match project_config::load_project_config(&project_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };
    if !output::is_json_mode() {
        if let Some(ref cfg) = project_config {
            let service_list: Vec<String> = cfg
                .services
                .iter()
                .map(|s| {
                    format!(
                        "{} ({} at {}, :{}, {})",
                        s.name, s.service_type, s.path, s.port, s.ingress
                    )
                })
                .collect();
            output::info(
                &format!(
                    "Loaded floo.toml — app '{}', {} service(s): {}",
                    cfg.app.name,
                    cfg.services.len(),
                    service_list.join(", ")
                ),
                None,
            );
        }
    }

    // Detect runtime/framework
    let detection = detect(&project_path);
    if detection.runtime == "unknown" {
        output::error(
            "No supported project files found.",
            "NO_RUNTIME_DETECTED",
            Some("Add a package.json, requirements.txt, or Dockerfile to your project."),
        );
        process::exit(1);
    }

    if !output::is_json_mode() {
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

    if detection.confidence == "low" && !output::confirm("Continue with this detection?") {
        process::exit(0);
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

    // Resolve or create app
    let app_data = if let Some(ref app_ident) = app {
        let spinner = output::Spinner::new("Looking up app...");
        let result = match resolve_app(&client, app_ident) {
            Ok(app_data) => app_data,
            Err(error) => {
                spinner.finish();
                cleanup(&archive_path);
                if error.code == "APP_NOT_FOUND" {
                    output::error(
                        &format!("App '{app_ident}' not found."),
                        "APP_NOT_FOUND",
                        Some("Check the app name or ID and try again."),
                    );
                } else {
                    output::error(&error.message, &error.code, None);
                }
                process::exit(1);
            }
        };
        spinner.finish();
        result
    } else {
        let app_name = name.unwrap_or_else(generate_name);
        let spinner = output::Spinner::new(&format!("Creating app {app_name}..."));
        match client.create_app(&app_name, Some(&detection.runtime)) {
            Ok(a) => {
                spinner.finish();
                a
            }
            Err(e) => {
                spinner.finish();
                cleanup(&archive_path);
                output::error(&e.message, &e.code, None);
                process::exit(1);
            }
        }
    };
    let app_id = required_response_id(&app_data, "app").to_string();

    // Deploy
    let spinner = output::Spinner::new("Uploading...");
    let mut deploy_data = match client.create_deploy(
        &app_id,
        &archive_path,
        &detection.runtime,
        detection.framework.as_deref(),
    ) {
        Ok(d) => {
            spinner.finish();
            d
        }
        Err(e) => {
            spinner.finish();
            cleanup(&archive_path);
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    // Clean up archive immediately after upload
    cleanup(&archive_path);

    // Wait for deploy to complete via SSE streaming or polling
    let initial_status = deploy_data
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if TERMINAL_STATUSES.contains(&initial_status) {
        // Phase 1: deploy already complete synchronously, skip streaming/polling
    } else if !output::is_json_mode() {
        // Phase 2 human mode: try SSE streaming, fall back to polling
        let deploy_id = required_response_id(&deploy_data, "deploy").to_string();
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
        // Phase 2 JSON mode: use polling
        deploy_data = poll_deploy(&client, &app_id, &deploy_data);
    }

    let final_status = deploy_data
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if final_status == "failed" {
        let build_logs = deploy_data
            .get("build_logs")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        output::error_with_data(
            "Deploy failed.",
            "DEPLOY_FAILED",
            Some("Check build output above, or run `floo logs` for details."),
            Some(serde_json::json!({
                "app": app_data,
                "deploy": deploy_data,
                "build_logs": build_logs,
            })),
        );
        process::exit(1);
    }

    let url = deploy_data
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    output::success(
        &format!("Deployed to {url}"),
        Some(serde_json::json!({
            "app": app_data,
            "deploy": deploy_data,
            "detection": detection.to_value(),
        })),
    );
}

/// Stream deploy logs via SSE and return the final deploy state.
fn stream_deploy(
    client: &FlooClient,
    app_id: &str,
    deploy_id: &str,
) -> Result<serde_json::Value, FlooApiError> {
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

/// Poll the deploy endpoint until it reaches a terminal status.
fn poll_deploy(
    client: &FlooClient,
    app_id: &str,
    initial_data: &serde_json::Value,
) -> serde_json::Value {
    let deploy_id = required_response_id(initial_data, "deploy").to_string();
    let poll_start = Instant::now();
    let mut last_log_len: usize = 0;
    let mut deploy_data = initial_data.clone();

    while !TERMINAL_STATUSES.contains(
        &deploy_data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    ) {
        if !output::is_json_mode() {
            let build_logs = deploy_data
                .get("build_logs")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if build_logs.len() > last_log_len {
                let new_logs = &build_logs[last_log_len..];
                for line in new_logs.trim().lines() {
                    output::dim_line(line);
                }
                last_log_len = build_logs.len();
            }

            let status = deploy_data
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            output::bold_line(status_label(status));
        }

        thread::sleep(POLL_INTERVAL);

        if poll_start.elapsed() >= POLL_TIMEOUT {
            output::error(
                "Deploy timed out after 10 minutes",
                "DEPLOY_TIMEOUT",
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
                output::error(&e.message, &e.code, None);
                process::exit(1);
            }
        };
    }

    // Print any remaining build logs for the final state
    if !output::is_json_mode() {
        let build_logs = deploy_data
            .get("build_logs")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if build_logs.len() > last_log_len {
            let new_logs = &build_logs[last_log_len..];
            for line in new_logs.trim().lines() {
                output::dim_line(line);
            }
        }
    }

    deploy_data
}

fn cleanup(path: &PathBuf) {
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
