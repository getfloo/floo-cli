use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::api_client::FlooClient;
use crate::api_types::Deploy;
use crate::config::load_config;
use crate::detection::{detect_for_services, DetectionResult};
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;
use crate::project_config::{
    self, validate_service_name, AppAccessMode, AppAgentMode, AppSource, ServiceConfig,
    ServiceIngress, ServiceType,
};
use crate::resolve::resolve_app;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes
pub(crate) const TERMINAL_STATUSES: &[&str] = &["live", "failed", "superseded"];

#[derive(Debug, Serialize)]
struct EnvInjectionPlan {
    mode: String,
    services: Vec<ServiceEnvInjectionPlan>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ServiceEnvInjectionPlan {
    service: String,
    managed: Vec<ManagedEnvInjection>,
    required: Vec<String>,
    optional: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ManagedEnvInjection {
    handle: String,
    keys: Vec<String>,
}

fn status_label(status: &str) -> &str {
    match status {
        "pending" => "Queued...",
        "building" => "Building...",
        "deploying" => "Deploying...",
        _ => "Deploying...",
    }
}

pub fn preflight(path: PathBuf, app: Option<String>, services_filter: Vec<String>) {
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

    let managed_services = project_config::discover_managed_services(&resolved);

    // 4. Per-service runtime detection
    let svc_pairs: Vec<(&str, &str)> = services
        .iter()
        .map(|s| (s.name.as_str(), s.path.as_str()))
        .collect();
    let (_primary_detection, per_service_detection) =
        detect_for_services(&project_path, &svc_pairs);

    // 5. Validate
    let (preflight_errors, preflight_warnings) =
        validate_preflight(&project_path, &services, &resolved, &managed_services);

    // 6. Generate security notes
    let security_notes = generate_security_notes(&services, &managed_services, &resolved);
    let env_injection_plan = build_env_injection_plan(&services, &managed_services, &resolved);

    // 7. Remote preflight audit (declared vs deployed). Best-effort — auth/resolution
    // failures degrade to a note; local validation still ships.
    let remote_plan = fetch_remote_preflight(&app_name, &managed_services, &resolved.config_dir);

    // 8. Display
    if output::is_json_mode() {
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

        let managed_json: Vec<serde_json::Value> = managed_services
            .iter()
            .map(|ms| {
                serde_json::json!({
                    "name": ms.name,
                    "tier": ms.tier.as_deref().unwrap_or("basic"),
                })
            })
            .collect();

        output::success(
            "",
            Some(serde_json::json!({
                "app": app_name,
                "services": svc_json,
                "managed_services": managed_json,
                "env_injection_plan": env_injection_plan,
                "warnings": warning_strings,
                "security_notes": security_notes,
                "plan": remote_plan.as_ref().map(crate::output::to_value),
                "valid": preflight_errors.is_empty(),
            })),
        );
    } else {
        display_preflight_human(
            &app_name,
            &resolved,
            &services,
            &per_service_detection,
            &env_injection_plan,
            &preflight_warnings,
            &preflight_errors,
        );

        if !managed_services.is_empty() {
            eprintln!("  Managed services (declared):");
            for ms in &managed_services {
                let tier_label = ms.tier.as_deref().unwrap_or("basic");
                eprintln!("    {} (tier {tier_label})", ms.name);
            }
            eprintln!();
        }

        if let Some(ref plan) = remote_plan {
            render_plan_human(plan);
        }

        if !security_notes.is_empty() {
            eprintln!("  Security:");
            for note in &security_notes {
                eprintln!("    \u{26a0} {note}");
            }
            eprintln!();
        }
    }

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
            Some("Fix the errors above and run `floo preflight` to re-validate."),
        );
        process::exit(1);
    }
}

pub fn deploy(
    path: PathBuf,
    app: Option<String>,
    services_filter: Vec<String>,
    rebuild: bool,
    sync_env: bool,
    skip_migrations: bool,
) {
    // Restart path reuses the existing image and never runs migrations,
    // so `--skip-migrations` only makes sense on a rebuild path. Reject
    // the combination loudly rather than silently dropping the flag.
    if skip_migrations && app.is_some() && !rebuild {
        output::error(
            "--skip-migrations requires --rebuild.",
            &ErrorCode::ConfigInvalid,
            Some(
                "Restart paths reuse the existing image and don't run migrations,\
                 so --skip-migrations has no effect there. Add --rebuild or drop the flag.",
            ),
        );
        process::exit(1);
    }

    // --- Path 1 & 2: --app flag provided — no local directory needed ---
    if let Some(ref app_name) = app {
        // Dry-run exits early — no auth or API calls needed
        if output::is_dry_run_mode() {
            let action = if rebuild { "rebuild" } else { "restart" };
            let service_names: Vec<&str> = services_filter.iter().map(|s| s.as_str()).collect();
            let svc_clause = if service_names.is_empty() {
                String::new()
            } else {
                format!(" (services: {})", service_names.join(", "))
            };
            let mig_clause = if skip_migrations {
                " (skip migrations)"
            } else {
                ""
            };
            let preview = format!("Would {action} app '{app_name}'{svc_clause}{mig_clause}.");
            output::dry_run_preview(
                &preview,
                serde_json::json!({
                    "action": action,
                    "app": app_name,
                    "services": service_names,
                    "skip_migrations": skip_migrations,
                }),
            );
            return;
        }

        let config = load_config();
        if config.api_key.is_none() {
            output::error(
                "Not logged in.",
                &ErrorCode::NotAuthenticated,
                Some("Run 'floo auth login' to authenticate."),
            );
            process::exit(1);
        }

        let client = super::init_client(Some(config));
        let app_data = match resolve_app(&client, app_name) {
            Ok(a) => a,
            Err(e) => {
                if e.code == "APP_NOT_FOUND" {
                    output::error(
                        &format!("App '{app_name}' not found."),
                        &ErrorCode::AppNotFound,
                        Some("Check the app name or ID and try again."),
                    );
                } else {
                    output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                }
                process::exit(1);
            }
        };

        if rebuild {
            deploy_rebuild(&client, &app_data, &services_filter, skip_migrations);
        } else {
            deploy_restart(&client, &app_data, &services_filter);
        }
        return;
    }

    // --- Path 3: No --app flag — full preflight from local project directory ---

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

    // 3b. Discover managed service declarations (postgres, redis, etc.)
    let managed_services = project_config::discover_managed_services(&resolved);

    // 4. Per-service runtime detection
    let svc_pairs: Vec<(&str, &str)> = services
        .iter()
        .map(|s| (s.name.as_str(), s.path.as_str()))
        .collect();
    let (primary_detection, per_service_detection) = detect_for_services(&project_path, &svc_pairs);

    // 5. Validate per-service (port, name, Dockerfile EXPOSE, env_file)
    let (preflight_errors, preflight_warnings) =
        validate_preflight(&project_path, &services, &resolved, &managed_services);
    let env_injection_plan = build_env_injection_plan(&services, &managed_services, &resolved);

    // 6. Display preflight info
    if !output::is_json_mode() {
        display_preflight_human(
            &app_name,
            &resolved,
            &services,
            &per_service_detection,
            &env_injection_plan,
            &preflight_warnings,
            &preflight_errors,
        );

        if !managed_services.is_empty() {
            eprintln!("  Managed services:");
            for ms in &managed_services {
                let tier_label = ms.tier.as_deref().unwrap_or("basic");
                eprintln!("    {} (tier {tier_label})", ms.name);
            }
            eprintln!();
        }
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

        let managed_json: Vec<serde_json::Value> = managed_services
            .iter()
            .map(|ms| {
                serde_json::json!({
                    "name": ms.name,
                    "tier": ms.tier.as_deref().unwrap_or("basic"),
                })
            })
            .collect();

        // Preflight already printed the human-friendly service table via
        // display_preflight_human() above (gated on !is_json_mode). Keep the
        // preview line tight so we don't duplicate it.
        let svc_count = services.len();
        let preview = format!(
            "Would deploy app '{app_name}' with {svc_count} service(s){}.",
            if preflight_errors.is_empty() {
                ""
            } else {
                "; preflight errors must be fixed first"
            }
        );
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "deploy",
                "app": app_name,
                "services": svc_json,
                "managed_services": managed_json,
                "env_injection_plan": env_injection_plan,
                "warnings": warning_strings,
                "valid": preflight_errors.is_empty(),
            }),
        );
        return;
    }

    // 8. Auth check — only needed for actual deploy
    let config = load_config();
    if config.api_key.is_none() {
        output::error(
            "Not logged in.",
            &ErrorCode::NotAuthenticated,
            Some("Run 'floo auth login' to authenticate."),
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
            Some("Fix the errors above and run `floo preflight` to validate."),
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

    // Extract agent_mode from [app] section
    let agent_mode: Option<AppAgentMode> =
        resolved.app_config.as_ref().and_then(|c| c.app.agent_mode);

    // Extract auth redirect URIs from [auth] toml section
    let auth_redirect_uris: Option<Vec<String>> = resolved
        .app_config
        .as_ref()
        .and_then(|c| c.auth.as_ref())
        .and_then(|auth| auth.redirect_uris.clone());

    // Extract reparo config from [reparo] toml section
    let reparo_config = resolved.app_config.as_ref().and_then(|c| c.reparo.as_ref());

    // Extract cron job definitions from [cron] toml section
    let cron_entries: Vec<crate::project_config::CronJobEntry> = resolved
        .app_config
        .as_ref()
        .map(|c| {
            c.cron
                .iter()
                .map(|(name, cfg)| crate::project_config::CronJobEntry {
                    name: name.clone(),
                    schedule: cfg.schedule.clone(),
                    command: cfg.command.clone(),
                    service: cfg.service.clone(),
                    timeout: cfg.timeout.unwrap_or(300),
                })
                .collect()
        })
        .unwrap_or_default();
    let cron_jobs_arg = if cron_entries.is_empty() {
        None
    } else {
        Some(cron_entries.as_slice())
    };

    // Extract [github] config
    let github_config = resolved.app_config.as_ref().and_then(|c| c.github.as_ref());

    // Deploy
    let svc_slice = Some(services.as_slice());
    let spinner = output::Spinner::new("Deploying...");
    let mut deploy_data = match client.create_deploy(
        &app_id,
        &detection.runtime,
        detection.framework.as_deref(),
        svc_slice,
        access_mode.as_ref().map(|m| m.as_str()),
        agent_mode.as_ref().map(|m| m.as_str()),
        auth_redirect_uris.as_deref(),
        reparo_config,
        cron_jobs_arg,
        github_config,
        skip_migrations,
    ) {
        Ok(d) => {
            spinner.finish();
            d
        }
        Err(e) => {
            spinner.finish();
            let suggestion = match e.code.as_str() {
                "PLAN_FEATURE_PASSWORD" | "PLAN_FEATURE_ACCOUNTS" | "PLAN_FEATURE_SSO" => {
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
        if !output::is_json_mode() && !build_logs.is_empty() && build_logs != "[no message content]"
        {
            output::bold_line("Build Logs");
            for line in build_logs.lines() {
                output::dim_line(line);
            }
        }
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

    if final_status == "superseded" {
        output::success(
            "Deploy superseded by a newer deploy.",
            Some(serde_json::json!({
                "app": output::to_value(&app_data),
                "deploy": output::to_value(&deploy_data),
                "detection": detection.to_value(),
            })),
        );
        return;
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
        // Closes feedback c9b70eb5 — surface the auto-deploy contract the
        // moment a manual `floo deploy` finishes, so the user knows the
        // next change ships via `git push` (no need to remember `floo deploy`).
        // The hint only renders for human terminals; JSON consumers infer
        // from the connected service in the response.
        let app_url = format!("https://app.getfloo.com/{}", app_data.name);
        output::info(
            &format!(
                "  Next deploys: push to your default branch. Manage at {app_url}"
            ),
            None,
        );
    }

    let service_names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();

    let env_display = deploy_data
        .environment_name
        .as_deref()
        .map(|e| format!("{e} \u{2192} "))
        .unwrap_or_default();
    output::success(
        &format!("Deployed to {env_display}{url}"),
        Some(serde_json::json!({
            "app": output::to_value(&app_data),
            "deploy": output::to_value(&deploy_data),
            "detection": detection.to_value(),
            "services": service_names,
        })),
    );
}

/// Redeploy existing images with fresh env vars (no build). Used when `--app` is
/// provided without `--rebuild`.
fn deploy_restart(
    client: &FlooClient,
    app_data: &crate::api_types::App,
    services_filter: &[String],
) {
    let app_id = &app_data.id;

    if output::is_dry_run_mode() {
        let service_names: Vec<&str> = services_filter.iter().map(|s| s.as_str()).collect();
        let svc_clause = if service_names.is_empty() {
            String::new()
        } else {
            format!(" (services: {})", service_names.join(", "))
        };
        let preview = format!("Would restart app '{}'{svc_clause}.", app_data.name);
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "restart",
                "app": app_data.name,
                "services": service_names,
            }),
        );
        return;
    }

    let svcs: Option<&[String]> = if services_filter.is_empty() {
        None
    } else {
        Some(services_filter)
    };

    let spinner = output::Spinner::new("Restarting...");
    let raw_deploy = match client.restart_app(app_id, svcs) {
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

    // The restart endpoint returns 202 with a DeployResponse — the pipeline
    // runs out-of-band, so the status is typically "pending" at this point.
    // Match the main deploy path and wait for a terminal status before
    // reporting back. Otherwise `floo redeploy --json` returns while the
    // deploy is still in progress and agents have to poll manually. See
    // feedback 966b2a4a.
    let mut deploy_data: Deploy = match serde_json::from_value(raw_deploy.clone()) {
        Ok(d) => d,
        Err(_) => {
            // Server response didn't match the Deploy shape — surface what we
            // got instead of silently pretending restart succeeded.
            output::error_with_data(
                "Restart returned an unexpected response shape.",
                &ErrorCode::RestartFailed,
                Some("Run `floo deploys list --app <name>` to check deploy status."),
                Some(serde_json::json!({
                    "app": output::to_value(app_data),
                    "deploy": raw_deploy,
                })),
            );
            process::exit(1);
        }
    };

    let initial_status = deploy_data.status.as_deref().unwrap_or("");
    if !TERMINAL_STATUSES.contains(&initial_status) {
        let deploy_id = deploy_data.id.clone();
        deploy_data = if !output::is_json_mode() {
            match stream_deploy(client, app_id, &deploy_id) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!(
                        "Stream unavailable ({}), falling back to polling...",
                        e.code
                    );
                    poll_deploy(client, app_id, &deploy_data)
                }
            }
        } else {
            match stream_deploy_json(client, app_id, &deploy_id) {
                Ok(d) => d,
                Err(_) => poll_deploy(client, app_id, &deploy_data),
            }
        };
    }

    let final_status = deploy_data.status.as_deref().unwrap_or("");
    let url = deploy_data.url.as_deref().unwrap_or("(no URL)");

    if final_status == "failed" {
        output::error_with_data(
            "Restart failed.",
            &ErrorCode::RestartFailed,
            Some("Run `floo logs` for details."),
            Some(serde_json::json!({
                "app": output::to_value(app_data),
                "deploy": output::to_value(&deploy_data),
            })),
        );
        process::exit(1);
    }

    if final_status == "superseded" {
        output::success(
            "Restart superseded by a newer deploy.",
            Some(serde_json::json!({
                "app": output::to_value(app_data),
                "deploy": output::to_value(&deploy_data),
            })),
        );
        return;
    }

    let env_display = deploy_data
        .environment_name
        .as_deref()
        .map(|e| format!("{e} \u{2192} "))
        .unwrap_or_default();
    output::success(
        &format!("Restarted {env_display}{url}"),
        Some(serde_json::json!({
            "app": output::to_value(app_data),
            "deploy": output::to_value(&deploy_data),
        })),
    );
}

/// Force a full rebuild from the latest commit. Used when `--app --rebuild` is
/// provided — no local project directory needed.
fn deploy_rebuild(
    client: &FlooClient,
    app_data: &crate::api_types::App,
    services_filter: &[String],
    skip_migrations: bool,
) {
    let app_id = &app_data.id;
    let runtime = app_data.runtime.as_deref().unwrap_or("unknown");

    if output::is_dry_run_mode() {
        let service_names: Vec<&str> = services_filter.iter().map(|s| s.as_str()).collect();
        let svc_clause = if service_names.is_empty() {
            String::new()
        } else {
            format!(" (services: {})", service_names.join(", "))
        };
        let mig_clause = if skip_migrations {
            " (skip migrations)"
        } else {
            ""
        };
        let preview = format!(
            "Would rebuild app '{}' (runtime: {runtime}){svc_clause}{mig_clause}.",
            app_data.name
        );
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "rebuild",
                "app": app_data.name,
                "runtime": runtime,
                "services": service_names,
                "skip_migrations": skip_migrations,
            }),
        );
        return;
    }

    let svcs: Option<&[String]> = if services_filter.is_empty() {
        None
    } else {
        Some(services_filter)
    };

    let spinner = output::Spinner::new("Rebuilding...");
    let mut deploy_data = match client.rebuild_app(app_id, runtime, svcs, skip_migrations) {
        Ok(d) => {
            spinner.finish();
            d
        }
        Err(e) => {
            spinner.finish();
            let suggestion = match e.code.as_str() {
                "PLAN_FEATURE_PASSWORD" | "PLAN_FEATURE_ACCOUNTS" | "PLAN_FEATURE_SSO" => {
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
        // Already complete
    } else if !output::is_json_mode() {
        let deploy_id = deploy_data.id.clone();
        match stream_deploy(client, app_id, &deploy_id) {
            Ok(final_data) => deploy_data = final_data,
            Err(e) => {
                eprintln!(
                    "Stream unavailable ({}), falling back to polling...",
                    e.code
                );
                deploy_data = poll_deploy(client, app_id, &deploy_data);
            }
        }
    } else {
        let deploy_id = deploy_data.id.clone();
        match stream_deploy_json(client, app_id, &deploy_id) {
            Ok(final_data) => deploy_data = final_data,
            Err(_) => deploy_data = poll_deploy(client, app_id, &deploy_data),
        }
    }

    let final_status = deploy_data.status.as_deref().unwrap_or("");

    if final_status == "failed" {
        let build_logs = deploy_data.build_logs.as_deref().unwrap_or("");
        if !output::is_json_mode() && !build_logs.is_empty() && build_logs != "[no message content]"
        {
            output::bold_line("Build Logs");
            for line in build_logs.lines() {
                output::dim_line(line);
            }
        }
        output::error_with_data(
            "Rebuild failed.",
            &ErrorCode::DeployFailed,
            Some("Check build output above, or run `floo logs` for details."),
            Some(serde_json::json!({
                "app": output::to_value(app_data),
                "deploy": output::to_value(&deploy_data),
                "build_logs": build_logs,
            })),
        );
        process::exit(1);
    }

    if final_status == "superseded" {
        output::success(
            "Rebuild superseded by a newer deploy.",
            Some(serde_json::json!({
                "app": output::to_value(app_data),
                "deploy": output::to_value(&deploy_data),
            })),
        );
        return;
    }

    let url = deploy_data.url.as_deref().unwrap_or("");
    output::success(
        &format!("Rebuilt and deployed {url}"),
        Some(serde_json::json!({
            "app": output::to_value(app_data),
            "deploy": output::to_value(&deploy_data),
        })),
    );
}

fn service_looks_like_rails(service_dir: &Path) -> bool {
    let gemfile = service_dir.join("Gemfile");
    if let Ok(contents) = std::fs::read_to_string(&gemfile) {
        let lower = contents.to_lowercase();
        if lower.contains("gem \"rails\"") || lower.contains("gem 'rails'") {
            return true;
        }
    }

    let app_config = service_dir.join("config").join("application.rb");
    if let Ok(contents) = std::fs::read_to_string(app_config) {
        return contents.contains("Rails::Application") || contents.contains("require \"rails\"");
    }

    false
}

fn env_files_for_service(
    service_dir: &Path,
    configured_env_file: Option<&str>,
) -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    if let Some(env_file) = configured_env_file {
        labels.push(env_file.to_string());
    }
    labels.extend(
        [".env", ".env.local", ".env.production", ".env.development"]
            .iter()
            .map(|label| label.to_string()),
    );

    for label in labels {
        if files.iter().any(|(existing, _)| existing == &label) {
            continue;
        }
        files.push((label.clone(), service_dir.join(label)));
    }
    files
}

fn parse_env_assignment(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (key, value) = trimmed.split_once('=')?;
    Some((
        key.trim(),
        value.trim().trim_matches('"').trim_matches('\''),
    ))
}

fn is_cloudsql_socket_database_url(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower.contains("@/")
        && (lower.contains("/cloudsql/")
            || lower.contains("host=/cloudsql")
            || lower.contains("%2fcloudsql%2f")
            || lower.contains("host=%2fcloudsql"))
}

/// Validate services for common config errors. Returns (errors, warnings).
/// Absorbs the validation logic that was previously in `floo check`.
fn validate_preflight(
    project_path: &Path,
    services: &[ServiceConfig],
    resolved: &project_config::ResolvedApp,
    managed_services: &[project_config::ManagedServiceDeclaration],
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut warnings: Vec<serde_json::Value> = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();
    let has_managed_postgres = managed_services.iter().any(|ms| ms.name == "postgres");

    for svc in services {
        // Validate service name
        if let Err(msg) = validate_service_name(&svc.name) {
            errors.push(serde_json::json!({
                "path": svc.path,
                "code": "INVALID_SERVICE_NAME",
                "message": msg,
            }));
        }

        // Check for duplicate names
        if seen_names.contains(&svc.name) {
            errors.push(serde_json::json!({
                "path": svc.path,
                "code": "DUPLICATE_SERVICE_NAME",
                "message": format!("Duplicate service name '{}'.", svc.name),
            }));
        } else {
            seen_names.push(svc.name.clone());
        }

        // Validate port
        if svc.port == 0 {
            errors.push(serde_json::json!({
                "path": svc.path,
                "code": "INVALID_PORT",
                "message": format!("Service '{}' has invalid port 0. Ports must be 1-65535.", svc.name),
            }));
        }

        let svc_dir = project_path.join(&svc.path);
        let configured_env_file = resolved
            .app_config
            .as_ref()
            .and_then(|app_cfg| app_cfg.services.get(&svc.name))
            .and_then(|entry| entry.env_file.as_deref());

        // Check env_file exists
        if let Some(ref app_cfg) = resolved.app_config {
            if let Some(entry) = app_cfg.services.get(&svc.name) {
                if let Some(ref env_file) = entry.env_file {
                    let env_path = svc_dir.join(env_file);
                    if !env_path.exists() {
                        warnings.push(serde_json::json!({
                            "path": svc.path,
                            "code": "ENV_FILE_NOT_FOUND",
                            "message": format!("Service '{}' env_file '{env_file}' not found on disk.", svc.name),
                        }));
                    }
                }
            }
        }

        if has_managed_postgres && service_looks_like_rails(&svc_dir) {
            for (env_label, env_path) in env_files_for_service(&svc_dir, configured_env_file) {
                let Ok(contents) = std::fs::read_to_string(&env_path) else {
                    continue;
                };
                for line in contents.lines() {
                    let Some((key, value)) = parse_env_assignment(line) else {
                        continue;
                    };
                    if key == "DATABASE_URL" && is_cloudsql_socket_database_url(value) {
                        warnings.push(serde_json::json!({
                            "path": svc.path,
                            "code": "RAILS_DATABASE_URL_SOCKET_DSN",
                            "message": format!(
                                "Service '{}' looks like Rails and {env_label} contains a Cloud SQL socket-style DATABASE_URL. Rails parses DATABASE_URL with Ruby's URI parser before app code runs, so postgresql://user:pass@/db?host=/cloudsql/... can fail at boot. Remove the stale local override or use floo's framework-compatible managed Postgres URL.",
                                svc.name
                            ),
                            "hint": "Managed Postgres now injects DATABASE_URL plus PGHOST/PGPORT/PGDATABASE/PGUSER/PGPASSWORD. The DATABASE_URL value should have a normal host, for example postgresql://user:pass@127.0.0.1:5432/db.",
                        }));
                        break;
                    }
                }
            }
        }

        // Check Dockerfile for common issues
        let dockerfile = svc_dir.join("Dockerfile");
        if dockerfile.exists() {
            match std::fs::read_to_string(&dockerfile) {
                Ok(content) => {
                    let no_lockfile = !svc_dir.join("package-lock.json").exists();
                    let mut npm_ci_flagged = false;

                    for line in content.lines() {
                        let trimmed = line.trim();

                        // Skip comments — don't match Dockerfile comment lines
                        if trimmed.starts_with('#') {
                            continue;
                        }

                        // EXPOSE mismatch
                        if let Some(expose_val) = trimmed.strip_prefix("EXPOSE ") {
                            let expose_val = expose_val.trim();
                            let port_str = expose_val.split('/').next().unwrap_or(expose_val);
                            if let Ok(exposed_port) = port_str.parse::<u16>() {
                                if exposed_port != svc.port {
                                    warnings.push(serde_json::json!({
                                        "path": svc.path,
                                        "code": "EXPOSE_PORT_MISMATCH",
                                        "message": format!(
                                            "Service '{}' Dockerfile EXPOSE {exposed_port} does not match configured port {}.",
                                            svc.name, svc.port
                                        ),
                                    }));
                                }
                            }
                        }

                        // CMD exec form with $PORT or ${PORT} — variables don't expand in exec form.
                        // Emitted as a warning (not error) because heredoc content could produce
                        // false positives; the runtime failure makes it obvious quickly.
                        if trimmed.starts_with("CMD [")
                            && (trimmed.contains("$PORT") || trimmed.contains("${PORT}"))
                        {
                            warnings.push(serde_json::json!({
                                "path": svc.path,
                                "code": "CMD_EXEC_FORM_PORT",
                                "message": format!(
                                    "Service '{}' Dockerfile CMD uses exec form with $PORT — $PORT won't expand at runtime.",
                                    svc.name
                                ),
                                "hint": "Use shell form: CMD [\"sh\", \"-c\", \"your-command $PORT\"]",
                            }));
                        }

                        // npm ci without package-lock.json — report once per service
                        if !npm_ci_flagged && no_lockfile && trimmed.contains("npm ci") {
                            errors.push(serde_json::json!({
                                "path": svc.path,
                                "code": "NPM_CI_NO_LOCKFILE",
                                "message": format!(
                                    "Service '{}' Dockerfile uses 'npm ci' but package-lock.json was not found.",
                                    svc.name
                                ),
                                "hint": "Commit package-lock.json or change 'npm ci' to 'npm install' in your Dockerfile",
                            }));
                            npm_ci_flagged = true;
                        }
                    }
                }
                Err(e) => {
                    warnings.push(serde_json::json!({
                        "path": svc.path,
                        "code": "DOCKERFILE_READ_ERROR",
                        "message": format!(
                            "Service '{}' Dockerfile exists but could not be read: {e}. Checks skipped.",
                            svc.name
                        ),
                    }));
                }
            }
        }
    }

    (errors, warnings)
}

/// Generate security notes based on service configuration and managed services.
///
/// These are informational (not errors/warnings) — they help users and agents
/// understand what's exposed and where secrets will be available.
fn generate_security_notes(
    services: &[ServiceConfig],
    managed_services: &[project_config::ManagedServiceDeclaration],
    resolved: &project_config::ResolvedApp,
) -> Vec<String> {
    let mut notes: Vec<String> = Vec::new();

    // Check access_mode — warn if no auth is configured
    // Mirror what the CLI actually sends to the API: deploy.rs:570 still
    // resolves env-override-wins for the body.access_mode it POSTs, so the
    // warning matches the value the deploy will use. Pre-this-PR this only
    // checked `[app]`, which made the warning fire even when the user had
    // set `[environments.dev] access_mode = "accounts"` correctly.
    let access_mode = resolved.app_config.as_ref().and_then(|c| {
        c.environments
            .get("dev")
            .and_then(|env| env.access_mode)
            .or(c.app.access_mode)
    });
    match access_mode {
        None | Some(AppAccessMode::Public) => {
            // Closes feedback 88e32b22 (floo-artifact 2026-05-01): the user
            // had to dig into the docs to discover that access_mode is a
            // toml knob and where it goes. Be specific: `[app]` is the
            // placement that actually applies on push deploys today.
            notes.push(
                "Access mode is 'public' (no auth). Anyone can access your app. \
                 To require auth, set `[app] access_mode = \"accounts\"` in \
                 floo.app.toml — that's the placement applied on every push. \
                 Per-env overrides via `[environments.<name>]` are accepted \
                 by the schema but not yet applied server-side; use \
                 `floo deploy --access-mode` to scope one env in the meantime."
                    .to_string(),
            );
        }
        _ => {} // accounts, password, sso — all have auth
    }

    // Note which services are internet-facing
    let public_services: Vec<&str> = services
        .iter()
        .filter(|s| s.ingress == ServiceIngress::Public)
        .map(|s| s.name.as_str())
        .collect();
    let internal_services: Vec<&str> = services
        .iter()
        .filter(|s| s.ingress == ServiceIngress::Internal)
        .map(|s| s.name.as_str())
        .collect();

    if !public_services.is_empty() && services.len() > 1 {
        notes.push(format!(
            "Internet-facing: {}. Set ingress = \"internal\" in floo.app.toml to restrict.",
            public_services.join(", ")
        ));
    }
    if !internal_services.is_empty() {
        notes.push(format!(
            "Internal only (not internet-facing): {}.",
            internal_services.join(", ")
        ));
    }

    // Warn about managed service env vars reaching frontend services.
    let env_plan = build_env_injection_plan(services, managed_services, resolved);
    if services.len() > 1 {
        let web_service_names: Vec<&str> = services
            .iter()
            .filter(|s| s.service_type == ServiceType::Web)
            .map(|s| s.name.as_str())
            .collect();
        if env_plan.mode == "implicit_all"
            && !web_service_names.is_empty()
            && env_plan.services.iter().any(|svc| !svc.managed.is_empty())
        {
            notes.push(format!(
                "Managed service credentials are implicitly available to every service, including {}. Add [services.<name>.env] managed = [...] to attach them only where needed.",
                web_service_names.join(", "),
            ));
        } else {
            for svc_plan in &env_plan.services {
                let Some(svc) = services.iter().find(|s| s.name == svc_plan.service) else {
                    continue;
                };
                if svc.service_type == ServiceType::Web && !svc_plan.managed.is_empty() {
                    let handles: Vec<&str> =
                        svc_plan.managed.iter().map(|m| m.handle.as_str()).collect();
                    notes.push(format!(
                        "Web service '{}' receives managed credentials: {}. Keep this only if browser-facing code really needs them server-side.",
                        svc.name,
                        handles.join(", "),
                    ));
                }
            }
        }
    }

    // Check for secrets leaked to frontend services via .env files
    for svc in services {
        if svc.service_type != ServiceType::Web {
            continue;
        }
        // Check common env file locations in the service directory
        let svc_path = std::path::Path::new(&svc.path);
        for env_filename in &[".env", ".env.local", ".env.production"] {
            let env_path = svc_path.join(env_filename);
            if let Ok(contents) = std::fs::read_to_string(&env_path) {
                for line in contents.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    if let Some((key, _)) = trimmed.split_once('=') {
                        let key = key.trim();
                        let is_frontend_var = key.starts_with("VITE_")
                            || key.starts_with("NEXT_PUBLIC_")
                            || key.starts_with("REACT_APP_");
                        if !is_frontend_var && looks_like_secret(key) {
                            notes.push(format!(
                                "Secret-looking var '{}' in {}/{} — if this is a backend \
                                 secret, remove it from the web service and set it on the \
                                 api service: floo env set {}=<val> --services api",
                                key, svc.name, env_filename, key
                            ));
                        }
                    }
                }
            }
        }
    }

    // Reminder to run preflight before every deploy
    notes.push("Run 'floo preflight' before every deploy to catch issues early.".to_string());

    notes
}

/// Heuristic: does this env var key look like it contains a secret?
fn looks_like_secret(key: &str) -> bool {
    let key_upper = key.to_uppercase();
    let secret_patterns = [
        "SECRET",
        "KEY",
        "TOKEN",
        "PASSWORD",
        "CREDENTIAL",
        "DATABASE_URL",
        "REDIS_URL",
        "API_KEY",
        "PRIVATE",
        "AUTH",
    ];
    secret_patterns.iter().any(|p| key_upper.contains(p))
}

fn env_contract_for_service(
    resolved: &project_config::ResolvedApp,
    svc: &ServiceConfig,
) -> Option<project_config::ServiceEnvContract> {
    if let Some(app_cfg) = resolved.app_config.as_ref() {
        if let Some(entry) = app_cfg.services.get(&svc.name) {
            if entry.env.is_some() {
                return entry.env.clone();
            }
        }
    }

    let service_dir = if svc.path == "." {
        resolved.config_dir.clone()
    } else {
        resolved.config_dir.join(&svc.path)
    };
    project_config::load_service_env_contract(&service_dir)
        .ok()
        .flatten()
}

fn build_env_injection_plan(
    services: &[ServiceConfig],
    managed_services: &[project_config::ManagedServiceDeclaration],
    resolved: &project_config::ResolvedApp,
) -> EnvInjectionPlan {
    let contracts: Vec<Option<project_config::ServiceEnvContract>> = services
        .iter()
        .map(|svc| env_contract_for_service(resolved, svc))
        .collect();
    let explicit_managed = contracts
        .iter()
        .any(|contract| contract.as_ref().and_then(|c| c.managed.as_ref()).is_some());
    let declared_handles = managed_env_handles(resolved, managed_services);

    let mut notes = Vec::new();
    let mode = if explicit_managed {
        "explicit".to_string()
    } else if !declared_handles.is_empty() {
        notes.push(
            "No service declares env.managed, so managed service credentials use legacy implicit injection."
                .to_string(),
        );
        "implicit_all".to_string()
    } else {
        "none".to_string()
    };

    let service_plans = services
        .iter()
        .zip(contracts.iter())
        .map(|(svc, contract)| {
            let required = contract
                .as_ref()
                .map(|c| c.required.clone())
                .unwrap_or_default();
            let optional = contract
                .as_ref()
                .map(|c| c.optional.clone())
                .unwrap_or_default();
            let handles = if explicit_managed {
                contract
                    .as_ref()
                    .and_then(|c| c.normalized_managed("[env]").ok().flatten())
                    .unwrap_or_default()
            } else {
                declared_handles.clone()
            };
            let managed = handles
                .iter()
                .map(|handle| ManagedEnvInjection {
                    handle: handle.clone(),
                    keys: project_config::managed_env_attachment_keys(handle),
                })
                .collect();
            ServiceEnvInjectionPlan {
                service: svc.name.clone(),
                managed,
                required,
                optional,
            }
        })
        .collect();

    EnvInjectionPlan {
        mode,
        services: service_plans,
        notes,
    }
}

fn managed_env_handles(
    resolved: &project_config::ResolvedApp,
    managed_services: &[project_config::ManagedServiceDeclaration],
) -> Vec<String> {
    let mut handles: Vec<String> = managed_services.iter().map(|ms| ms.name.clone()).collect();
    if let Ok(lock) = crate::services_lock::read(&resolved.config_dir) {
        for managed in lock.managed_services {
            let handle = if managed.name == "default" {
                managed.service_type
            } else {
                format!("{}:{}", managed.service_type, managed.name)
            };
            handles.push(handle);
        }
    }
    handles.sort();
    handles.dedup();
    handles
}

fn display_env_injection_plan(plan: &EnvInjectionPlan) {
    eprintln!("  Env injection plan:");
    match plan.mode.as_str() {
        "explicit" => eprintln!("    mode: explicit per-service env.managed"),
        "implicit_all" => eprintln!("    mode: legacy implicit managed env on every service"),
        _ => eprintln!("    mode: no managed service credentials declared locally"),
    }
    for note in &plan.notes {
        eprintln!("    note: {note}");
    }
    for svc in &plan.services {
        eprintln!("    {}", svc.service);
        if svc.managed.is_empty() {
            eprintln!("      managed: none");
        } else {
            for managed in &svc.managed {
                eprintln!("      {} -> {}", managed.handle, managed.keys.join(", "));
            }
        }
        if !svc.required.is_empty() {
            eprintln!("      required: {}", svc.required.join(", "));
        }
        if !svc.optional.is_empty() {
            eprintln!("      optional: {}", svc.optional.join(", "));
        }
    }
    eprintln!();
}

/// Display preflight info in human-readable format.
fn display_preflight_human(
    app_name: &str,
    resolved: &project_config::ResolvedApp,
    services: &[ServiceConfig],
    per_service_detection: &[(String, DetectionResult)],
    env_injection_plan: &EnvInjectionPlan,
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

    display_env_injection_plan(env_injection_plan);

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
        // Closes feedback c9b70eb5: "no obvious signal anywhere in the Floo
        // workflow that dev auto-deploys on GitHub push." The CLI is the
        // surface the user is in front of right before pushing — surfacing
        // the auto-deploy contract here means they don't have to leave for
        // the docs to learn what `git push` will do next.
        eprintln!();
        eprintln!(
            "  Deploys: dev auto-deploys on every `git push` to your default branch."
        );
        eprintln!("           Cut a GitHub release to promote the same build to production.");
        eprintln!(
            "           See https://getfloo.com/docs/guides/golden-path.md for the full flow."
        );
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
            match super::env::validate_env_file_path(env_file, &resolved.config_dir) {
                Ok(path) => env_file_entries.push((svc_config.service.name.clone(), path)),
                Err(msg) => {
                    output::error(&msg, &ErrorCode::InvalidPath, None);
                    process::exit(1);
                }
            }
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
                        match super::env::validate_env_file_path(env_file, &svc_dir) {
                            Ok(path) => {
                                env_file_entries.push((svc_config.service.name.clone(), path))
                            }
                            Err(msg) => {
                                output::error(&msg, &ErrorCode::InvalidPath, None);
                                process::exit(1);
                            }
                        }
                    }
                }
            }
        }
    }

    if env_file_entries.is_empty() {
        return;
    }

    // Get server-side services — silently return on API error (services may not exist on first deploy)
    let server_services = match client.list_services(app_id, None) {
        Ok(r) => r.services,
        Err(_) => return,
    };

    for (svc_name, env_file_path) in &env_file_entries {
        let server_svc = server_services.iter().find(|s| s.name == *svc_name);

        let Some(svc) = server_svc else { continue };
        let svc_id = &svc.id;

        // Check env var count on server
        let env_count = match client.list_env_vars(app_id, Some(svc_id), "dev") {
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

        if let Err(e) = client.import_env_vars(app_id, &vars, Some(svc_id), "dev") {
            output::warn(&format!(
                "Failed to import env vars for service '{svc_name}': {}",
                e.message
            ));
        }
    }
}

fn fetch_remote_preflight(
    app_name: &str,
    managed: &[project_config::ManagedServiceDeclaration],
    project_root: &Path,
) -> Option<crate::api_types::PreflightPlan> {
    use crate::api_types::DeclaredState;
    use crate::config::load_config;

    load_config().api_key.as_ref()?;

    let client = crate::api_client::FlooClient::new(None).ok()?;
    let app = crate::resolve::resolve_app(&client, app_name).ok()?;

    let declared = DeclaredState {
        managed_services: collect_declared_managed_services(managed, project_root),
    };

    client.preflight(&app.id, &declared).ok()
}

/// Build the full list of declared managed services for preflight by merging:
///
/// - Legacy top-level `[postgres]` / `[redis]` / `[storage]` sections in
///   `floo.app.toml` (passed in as `managed`).
/// - `.floo/services.lock` entries written by `floo services add`.
///
/// The lock file is the canonical record for the new explicit-attachment
/// model — services provisioned via the CLI never appear in `floo.app.toml`,
/// so leaving them out of the preflight request body would make every
/// CLI-managed service look like drift (`to_orphan`) and flip the plan
/// destructive. See feedback id `0cadb329`.
///
/// Dedup by (service_type, name): when the same `(type, name)` appears in
/// both sources, the TOML version wins because it carries an explicit `tier`.
fn collect_declared_managed_services(
    managed: &[project_config::ManagedServiceDeclaration],
    project_root: &Path,
) -> Vec<crate::api_types::DeclaredManagedService> {
    use crate::api_types::DeclaredManagedService;

    let mut declared: Vec<DeclaredManagedService> = managed
        .iter()
        .map(|ms| DeclaredManagedService {
            service_type: ms.name.clone(),
            name: "default".to_string(),
            tier: ms.tier.clone(),
        })
        .collect();

    if let Ok(lock) = crate::services_lock::read(project_root) {
        for entry in lock.managed_services {
            let already_present = declared
                .iter()
                .any(|d| d.service_type == entry.service_type && d.name == entry.name);
            if !already_present {
                declared.push(DeclaredManagedService {
                    service_type: entry.service_type,
                    name: entry.name,
                    tier: None,
                });
            }
        }
    }

    declared
}

fn render_plan_human(plan: &crate::api_types::PreflightPlan) {
    let ms = &plan.managed_services;
    if !ms.to_provision.is_empty() {
        eprintln!("  Will provision on next deploy:");
        for item in &ms.to_provision {
            let tier = item.tier.as_deref().unwrap_or("basic");
            eprintln!(
                "    + {} (tier {tier})",
                format_args!("{}/{}", item.service_type, item.name)
            );
        }
        eprintln!();
    }
    if !ms.to_orphan.is_empty() {
        eprintln!("  \u{26a0} Orphaned managed services (deploy will NOT remove these):");
        for item in &ms.to_orphan {
            let impact = item
                .data_impact
                .as_deref()
                .unwrap_or("managed service data");
            eprintln!("    - {}/{}  [{}]", item.service_type, item.name, impact);
        }
        eprintln!("    Run 'floo services remove <type> --app <name>' to deprovision explicitly.");
        eprintln!();
    }
    if !ms.in_flight_deprovisioning.is_empty() {
        eprintln!("  \u{26a0} Deprovisioning in flight:");
        for item in &ms.in_flight_deprovisioning {
            eprintln!("    … {}/{}", item.service_type, item.name);
        }
        eprintln!();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::TempDir;

    use super::*;

    fn write_lock(dir: &Path, body: &str) {
        let lock_dir = dir.join(".floo");
        fs::create_dir_all(&lock_dir).unwrap();
        fs::write(lock_dir.join("services.lock"), body).unwrap();
    }

    #[test]
    fn test_collect_declared_managed_services_empty_when_nothing_declared() {
        let dir = TempDir::new().unwrap();
        let result = collect_declared_managed_services(&[], dir.path());
        assert!(result.is_empty());
    }

    #[test]
    fn test_collect_declared_managed_services_pulls_lock_entries() {
        let dir = TempDir::new().unwrap();
        write_lock(
            dir.path(),
            r#"{
              "version": 1,
              "managed_services": [
                {"type": "postgres", "name": "default", "status": "ready", "created_at": null},
                {"type": "redis", "name": "default", "status": "ready", "created_at": null},
                {"type": "storage", "name": "default", "status": "ready", "created_at": null}
              ]
            }"#,
        );
        let result = collect_declared_managed_services(&[], dir.path());
        let pairs: Vec<(String, String)> = result
            .iter()
            .map(|d| (d.service_type.clone(), d.name.clone()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("postgres".to_string(), "default".to_string()),
                ("redis".to_string(), "default".to_string()),
                ("storage".to_string(), "default".to_string()),
            ]
        );
    }

    #[test]
    fn test_collect_declared_managed_services_preserves_named_services() {
        let dir = TempDir::new().unwrap();
        write_lock(
            dir.path(),
            r#"{
              "version": 1,
              "managed_services": [
                {"type": "postgres", "name": "default", "status": "ready", "created_at": null},
                {"type": "postgres", "name": "analytics", "status": "ready", "created_at": null}
              ]
            }"#,
        );
        let result = collect_declared_managed_services(&[], dir.path());
        let names: Vec<String> = result.iter().map(|d| d.name.clone()).collect();
        assert!(names.contains(&"default".to_string()));
        assert!(names.contains(&"analytics".to_string()));
    }

    #[test]
    fn test_collect_declared_managed_services_dedups_against_toml() {
        let dir = TempDir::new().unwrap();
        write_lock(
            dir.path(),
            r#"{
              "version": 1,
              "managed_services": [
                {"type": "postgres", "name": "default", "status": "ready", "created_at": null}
              ]
            }"#,
        );
        let toml_decl = vec![project_config::ManagedServiceDeclaration {
            name: "postgres".to_string(),
            tier: Some("basic".to_string()),
        }];
        let result = collect_declared_managed_services(&toml_decl, dir.path());
        assert_eq!(result.len(), 1);
        // TOML wins because it carries an explicit tier.
        assert_eq!(result[0].tier.as_deref(), Some("basic"));
    }

    #[test]
    fn test_collect_declared_managed_services_no_lock_file() {
        let dir = TempDir::new().unwrap();
        // No .floo/services.lock — only TOML declarations come through.
        let toml_decl = vec![project_config::ManagedServiceDeclaration {
            name: "postgres".to_string(),
            tier: None,
        }];
        let result = collect_declared_managed_services(&toml_decl, dir.path());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].service_type, "postgres");
        assert_eq!(result[0].name, "default");
    }

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
