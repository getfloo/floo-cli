use std::path::PathBuf;
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use crate::api_client::FlooClient;
use crate::archive::create_archive;
use crate::config::load_config;
use crate::detection::detect;
use crate::names::generate_name;
use crate::output;

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

    let client = FlooClient::new(Some(config));

    // Resolve or create app
    let app_data = if let Some(ref app_ident) = app {
        let spinner = output::Spinner::new("Looking up app...");
        let result = match client.get_app(app_ident) {
            Ok(a) => a,
            Err(_) => {
                // Try name match
                match client.list_apps(1, 20) {
                    Ok(resp) => {
                        let found = resp
                            .get("apps")
                            .and_then(|v| v.as_array())
                            .and_then(|apps| {
                                apps.iter().find(|a| {
                                    a.get("name").and_then(|v| v.as_str()) == Some(app_ident)
                                })
                            })
                            .cloned();
                        match found {
                            Some(a) => a,
                            None => {
                                spinner.finish();
                                cleanup(&archive_path);
                                output::error(
                                    &format!("App '{app_ident}' not found."),
                                    "APP_NOT_FOUND",
                                    Some("Check the app ID or name and try again."),
                                );
                                process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        spinner.finish();
                        cleanup(&archive_path);
                        output::error(&e.message, &e.code, None);
                        process::exit(1);
                    }
                }
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

    // Deploy
    let spinner = output::Spinner::new("Uploading...");
    let mut deploy_data = match client.create_deploy(
        app_data.get("id").and_then(|v| v.as_str()).unwrap_or(""),
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

    // Poll until terminal status
    let poll_start = Instant::now();
    let mut last_log_len: usize = 0;
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
                Some("The deploy may still complete — check status with `floo apps status`"),
            );
            process::exit(1);
        }

        let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let deploy_id = deploy_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
        deploy_data = match client.get_deploy(app_id, deploy_id) {
            Ok(d) => d,
            Err(e) => {
                output::error(&e.message, &e.code, None);
                process::exit(1);
            }
        };
    }

    let final_status = deploy_data
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if final_status == "failed" {
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
        output::error("Deploy failed.", "DEPLOY_FAILED", None);
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

fn cleanup(path: &PathBuf) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}
