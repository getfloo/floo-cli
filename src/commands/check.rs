use std::path::PathBuf;
use std::process;

use crate::errors::ErrorCode;
use crate::output;
use crate::project_config::{self, validate_service_name};

pub fn check(path: PathBuf) {
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

    // Resolve config context
    let resolved = match project_config::resolve_app_context(&project_path, None) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let app_name = &resolved.app_name;

    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut warnings: Vec<serde_json::Value> = Vec::new();

    // Discover services using the same logic as deploy
    let services = match project_config::discover_services(&resolved) {
        Ok(svcs) => svcs,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // Validate each discovered service
    let mut seen_names: Vec<String> = Vec::new();
    let mut services_info: Vec<serde_json::Value> = Vec::new();

    for svc in &services {
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

        // Check env_file exists (for inline services, look at app config entry)
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

        // Check Dockerfile EXPOSE matches port (best-effort)
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

        let mut svc_json = serde_json::json!({
            "name": svc.name,
            "path": svc.path,
            "port": svc.port,
            "type": svc.service_type.to_string(),
            "ingress": svc.ingress.to_string(),
        });

        // Include resource fields when set
        if let Some(ref cpu) = svc.cpu {
            svc_json["cpu"] = serde_json::json!(cpu);
        }
        if let Some(ref memory) = svc.memory {
            svc_json["memory"] = serde_json::json!(memory);
        }
        if let Some(max) = svc.max_instances {
            svc_json["max_instances"] = serde_json::json!(max);
        }
        if let Some(ref domain) = svc.domain {
            svc_json["domain"] = serde_json::json!(domain);
        }

        services_info.push(svc_json);
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
        // Architecture display
        eprintln!();
        eprintln!(
            "  App: {app_name} ({} service{})",
            services.len(),
            if services.len() == 1 { "" } else { "s" }
        );
        eprintln!();

        for svc in &services {
            let path_label = if svc.path == "." {
                String::new()
            } else {
                format!(" (./{path})", path = svc.path)
            };
            eprintln!("  {}{path_label}", svc.name);

            let mut details = format!(
                "    type: {}, port: {}, ingress: {}",
                svc.service_type, svc.port, svc.ingress
            );
            if let Some(ref cpu) = svc.cpu {
                details.push_str(&format!(", cpu: {cpu}"));
            }
            if let Some(ref memory) = svc.memory {
                details.push_str(&format!(", memory: {memory}"));
            }
            if let Some(max) = svc.max_instances {
                details.push_str(&format!(", max_instances: {max}"));
            }
            eprintln!("{details}");
            eprintln!();
        }

        // Show global [resources] if present
        if let Some(ref app_cfg) = resolved.app_config {
            if let Some(ref res) = app_cfg.resources {
                let has_any =
                    res.cpu.is_some() || res.memory.is_some() || res.max_instances.is_some();
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
                &ErrorCode::ConfigInvalid,
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
