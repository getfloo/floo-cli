use std::path::Path;
use std::process;

use crate::api_client::FlooClient;
use crate::errors::ErrorCode;
use crate::output;
use crate::project_config;

const DEPLOY_HINT: &str =
    "Push a commit to trigger a deploy with the updated env vars, or run: floo deploy";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_target(app_name: &str, service_name: Option<&str>) -> String {
    match service_name {
        Some(sn) => format!("{app_name}/{sn}"),
        None => app_name.to_string(),
    }
}

/// Resolve `--services` names to service IDs.
///
/// Returns a list of `(Option<service_id>, Option<service_name>)` pairs.
/// - App with 0 services and no `--services` → `[(None, None)]` (app-level)
/// - 1 service, no `--services` → auto-select `[(Some(uuid), Some(name))]`
/// - 2+ services, no `--services` → error
/// - `--services` provided → resolve each name to UUID
fn resolve_service_ids(
    client: &FlooClient,
    app_id: &str,
    app_name: &str,
    service_names: &[String],
) -> Vec<(Option<String>, Option<String>)> {
    let result = match client.list_services(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let services = &result.services;

    if service_names.is_empty() {
        match services.len() {
            0 => return vec![(None, None)],
            1 => {
                let svc = &services[0];
                return vec![(Some(svc.id.clone()), Some(svc.name.clone()))];
            }
            _ => {
                let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
                output::error(
                    &format!(
                        "App '{app_name}' has multiple services. Specify which with --services."
                    ),
                    &ErrorCode::MultipleServices,
                    Some(&format!("Available services: {}", names.join(", "))),
                );
                process::exit(1);
            }
        }
    }

    service_names
        .iter()
        .map(|name| {
            let found = services.iter().find(|s| s.name == *name);
            match found {
                Some(svc) => (Some(svc.id.clone()), Some(name.clone())),
                None => {
                    let available: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
                    output::error(
                        &format!("Service '{name}' not found on app '{app_name}'."),
                        &ErrorCode::ServiceNotFound,
                        Some(&format!("Available services: {}", available.join(", "))),
                    );
                    process::exit(1);
                }
            }
        })
        .collect()
}

fn resolve_single_service(
    client: &FlooClient,
    app_id: &str,
    app_name: &str,
    service_flag: Option<&str>,
) -> (Option<String>, Option<String>) {
    let names = match service_flag {
        Some(name) => vec![name.to_string()],
        None => vec![],
    };
    let ids = resolve_service_ids(client, app_id, app_name, &names);
    ids.into_iter().next().unwrap_or_else(|| {
        output::error(
            "Service resolution returned no results.",
            &ErrorCode::InternalError,
            Some("This is a bug. Please report it."),
        );
        process::exit(1);
    })
}

fn parse_env_file(path: &Path) -> Vec<(String, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            let msg = match e.kind() {
                std::io::ErrorKind::NotFound => {
                    format!("File not found: {}", path.display())
                }
                std::io::ErrorKind::PermissionDenied => {
                    format!("Permission denied reading: {}", path.display())
                }
                _ => format!("Failed to read {}: {e}", path.display()),
            };
            output::error(
                &msg,
                &ErrorCode::EnvFileNotFound,
                Some("Check the file path and permissions."),
            );
            process::exit(1);
        }
    };

    let mut vars = Vec::new();
    let mut bad_lines = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        match trimmed.split_once('=') {
            Some((key, value)) => {
                let key = key.trim().to_uppercase();
                let mut value = value.trim().to_string();
                if (value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\''))
                {
                    value = value[1..value.len() - 1].to_string();
                }
                vars.push((key, value));
            }
            None => {
                bad_lines.push(line_num + 1);
            }
        }
    }

    if !bad_lines.is_empty() {
        output::error(
            &format!(
                "Malformed lines in {}: line(s) {}",
                path.display(),
                bad_lines
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            &ErrorCode::EnvParseError,
            Some("Each non-comment line must be in KEY=VALUE format."),
        );
        process::exit(1);
    }

    if vars.is_empty() {
        output::error(
            &format!("No valid KEY=VALUE pairs found in {}.", path.display()),
            &ErrorCode::EnvParseError,
            Some("Ensure the file contains lines in KEY=VALUE format."),
        );
        process::exit(1);
    }

    vars
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

pub fn set(key_value: &str, app_flag: Option<&str>, service_names: &[String], restart: bool) {
    if !key_value.contains('=') {
        output::error(
            "Invalid format. Use KEY=VALUE.",
            &ErrorCode::InvalidFormat,
            Some("Example: floo env set DATABASE_URL=postgres://..."),
        );
        process::exit(1);
    }

    let (key, value) = key_value.split_once('=').unwrap();
    let key = key.to_uppercase();

    if output::is_dry_run_mode() {
        output::dry_run_success(serde_json::json!({
            "action": "env_set",
            "key": key,
            "will_restart": restart,
        }));
        return;
    }

    super::require_auth();

    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);

    let mut last_env_result = serde_json::Value::Null;
    for (service_id, service_name) in &targets {
        match client.set_env_var(&app_id, &key, value, service_id.as_deref()) {
            Ok(result) => {
                let target = format_target(&app_name, service_name.as_deref());
                last_env_result = output::to_value(&result);
                if !restart || !output::is_json_mode() {
                    output::success(
                        &format!("Set {key} on {target}."),
                        Some(output::to_value(&result)),
                    );
                }
            }
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        }
    }

    if !restart && !output::is_json_mode() {
        output::info(DEPLOY_HINT, None);
    }

    if restart {
        let svcs = if service_names.is_empty() {
            None
        } else {
            Some(service_names)
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
                Some(serde_json::json!({"env": last_env_result, "deploy": deploy_data})),
            );
            process::exit(1);
        }

        output::success(
            &format!("Restarted {url}"),
            Some(serde_json::json!({"env": last_env_result, "deploy": deploy_data})),
        );
    }
}

pub fn list(app_flag: Option<&str>, service_names: &[String]) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);

    for (service_id, service_name) in &targets {
        let result = match client.list_env_vars(&app_id, service_id.as_deref()) {
            Ok(r) => r,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };

        let target = format_target(&app_name, service_name.as_deref());

        if result.env_vars.is_empty() {
            if output::is_json_mode() {
                output::success("No env vars.", Some(serde_json::json!({"env_vars": []})));
            } else {
                output::info(&format!("No environment variables set on {target}."), None);
            }
            continue;
        }

        let rows: Vec<Vec<String>> = result
            .env_vars
            .iter()
            .map(|ev| {
                vec![
                    ev.key.clone(),
                    ev.masked_value.as_deref().unwrap_or("-").to_string(),
                ]
            })
            .collect();

        output::table(&["Key", "Value"], &rows, Some(output::to_value(&result)));
    }
}

pub fn remove(key: &str, app_flag: Option<&str>, service_names: &[String]) {
    let key = key.to_uppercase();

    if output::is_dry_run_mode() {
        output::dry_run_success(serde_json::json!({
            "action": "env_remove",
            "key": key,
        }));
        return;
    }

    super::require_auth();

    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);

    for (service_id, service_name) in &targets {
        if let Err(e) = client.delete_env_var(&app_id, &key, service_id.as_deref()) {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }

        let target = format_target(&app_name, service_name.as_deref());
        output::success(
            &format!("Removed {key} from {target}."),
            Some(serde_json::json!({"key": key})),
        );
    }
}

pub fn get(key: &str, app_flag: Option<&str>, service_flag: Option<&str>) {
    let key = key.to_uppercase();
    super::require_auth();

    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let (service_id, _service_name) =
        resolve_single_service(&client, &app_id, &app_name, service_flag);

    let result = match client.get_env_var(&app_id, &key, service_id.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let value = result.value.as_deref().unwrap_or_else(|| {
        output::error(
            "Response missing 'value' field.",
            &ErrorCode::ParseError,
            Some("This is a bug. Please report it."),
        );
        process::exit(1);
    });

    if output::is_json_mode() {
        output::success(
            &format!("Got {key}."),
            Some(serde_json::json!({"key": key, "value": value})),
        );
    } else {
        output::raw_value(value);
    }
}

pub fn import_vars(file_flag: Option<&Path>, app_flag: Option<&str>, service_names: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    });

    // Resolve config once — used for both env_file default and app resolution.
    // NO_CONFIG_FOUND is acceptable when determining env_file path (fall back to .env),
    // but all other errors (LEGACY_CONFIG, INVALID_PROJECT_CONFIG) must surface.
    let resolved = match project_config::resolve_app_context(&cwd, app_flag) {
        Ok(r) => Some(r),
        Err(e) if e.code == ErrorCode::NoConfigFound => None,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let env_file_path = match file_flag {
        Some(p) => p.to_path_buf(),
        None => {
            let from_config = resolved
                .as_ref()
                .and_then(|r| r.service_config.as_ref())
                .and_then(|sc| sc.service.env_file.as_deref());
            match from_config {
                Some(f) => cwd.join(f),
                None => cwd.join(".env"),
            }
        }
    };

    if output::is_dry_run_mode() {
        let vars = parse_env_file(&env_file_path);
        let keys: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        let count = vars.len();
        output::dry_run_success(serde_json::json!({
            "action": "env_import",
            "file": env_file_path.display().to_string(),
            "keys": keys,
            "count": count,
        }));
        return;
    }

    super::require_auth();

    let vars = parse_env_file(&env_file_path);
    let count = vars.len();

    // Use resolved app name if available, otherwise fall back to resolving from config.
    let client = super::init_client(None);
    let (app_id, app_name) = match &resolved {
        Some(r) => {
            let app = super::resolve_app_or_exit(&client, &r.app_name);
            (app.id.clone(), app.name.clone())
        }
        None => super::resolve_app_from_config(&client, app_flag),
    };

    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);

    for (service_id, service_name) in &targets {
        match client.import_env_vars(&app_id, &vars, service_id.as_deref()) {
            Ok(result) => {
                let target = format_target(&app_name, service_name.as_deref());
                output::success(
                    &format!("Imported {count} variable(s) to {target}."),
                    Some(result),
                );
            }
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        }
    }

    if !output::is_json_mode() {
        output::info(DEPLOY_HINT, None);
    }
}

pub fn import_all_services(app_flag: Option<&str>) {
    super::require_auth();

    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    });

    let resolved = match project_config::resolve_app_context(&cwd, app_flag) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // Collect (service_name, env_file_path) pairs from all service configs
    let mut env_file_entries: Vec<(String, std::path::PathBuf)> = Vec::new();

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
            let Some(ref path_str) = entry.path else {
                continue;
            };
            let normalized = path_str.strip_prefix("./").unwrap_or(path_str);
            let normalized = normalized.strip_suffix('/').unwrap_or(normalized);
            if normalized.is_empty() || normalized == "." {
                continue;
            }
            let svc_dir = resolved.config_dir.join(normalized);
            match project_config::load_service_config(&svc_dir) {
                Ok(Some(svc_config)) => {
                    if let Some(ref env_file) = svc_config.service.env_file {
                        let path = svc_dir.join(env_file);
                        env_file_entries.push((svc_config.service.name.clone(), path));
                    } else if !output::is_json_mode() {
                        output::info(
                            &format!(
                                "Skipping service '{}' (no env_file configured).",
                                svc_config.service.name
                            ),
                            None,
                        );
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    output::error(
                        &format!(
                            "Failed to load service config at '{}': {}",
                            svc_dir.display(),
                            e.message
                        ),
                        &e.code,
                        e.suggestion.as_deref(),
                    );
                    process::exit(1);
                }
            }
        }
    }

    if env_file_entries.is_empty() {
        output::error(
            "No services with env_file configured.",
            &ErrorCode::NoEnvFiles,
            Some("Add env_file to your service configs, e.g. env_file = \".env\""),
        );
        process::exit(1);
    }

    let client = super::init_client(None);
    let app = super::resolve_app_or_exit(&client, &resolved.app_name);
    let app_id = app.id.clone();
    let app_name = app.name.clone();

    // Resolve all service IDs from server
    let server_services = match client.list_services(&app_id) {
        Ok(r) => r.services,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let mut total_imported: usize = 0;
    let mut services_imported: usize = 0;

    for (svc_name, env_path) in &env_file_entries {
        let vars = parse_env_file(env_path);
        let count = vars.len();

        // Find service ID on server
        let service_id = server_services
            .iter()
            .find(|s| s.name == *svc_name)
            .map(|s| s.id.as_str());

        match client.import_env_vars(&app_id, &vars, service_id) {
            Ok(result) => {
                let target = format!("{app_name}/{svc_name}");
                output::success(
                    &format!("Imported {count} variable(s) to {target}."),
                    Some(result),
                );
                total_imported += count;
                services_imported += 1;
            }
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        }
    }

    if !output::is_json_mode() {
        if services_imported > 1 {
            output::info(
                &format!(
                    "Imported {total_imported} total variable(s) across {services_imported} service(s)."
                ),
                None,
            );
        }
        output::info(DEPLOY_HINT, None);
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::TempDir;

    use super::*;

    fn write_env_file(dir: &TempDir, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_parse_env_file_basic() {
        let dir = TempDir::new().unwrap();
        let path = write_env_file(&dir, ".env", "KEY=value\nOTHER=123\n");
        let vars = parse_env_file(&path);
        assert_eq!(
            vars,
            vec![
                ("KEY".to_string(), "value".to_string()),
                ("OTHER".to_string(), "123".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_env_file_comments_and_blanks() {
        let dir = TempDir::new().unwrap();
        let path = write_env_file(
            &dir,
            ".env",
            "# this is a comment\n\nKEY=value\n\n# another\nFOO=bar\n",
        );
        let vars = parse_env_file(&path);
        assert_eq!(
            vars,
            vec![
                ("KEY".to_string(), "value".to_string()),
                ("FOO".to_string(), "bar".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_env_file_quotes() {
        let dir = TempDir::new().unwrap();
        let path = write_env_file(
            &dir,
            ".env",
            "DOUBLE=\"hello world\"\nSINGLE='foo bar'\nNONE=plain\n",
        );
        let vars = parse_env_file(&path);
        assert_eq!(
            vars,
            vec![
                ("DOUBLE".to_string(), "hello world".to_string()),
                ("SINGLE".to_string(), "foo bar".to_string()),
                ("NONE".to_string(), "plain".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_env_file_export_prefix() {
        let dir = TempDir::new().unwrap();
        let path = write_env_file(&dir, ".env", "export KEY=value\nexport OTHER=123\n");
        let vars = parse_env_file(&path);
        assert_eq!(
            vars,
            vec![
                ("KEY".to_string(), "value".to_string()),
                ("OTHER".to_string(), "123".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_env_file_uppercase_keys() {
        let dir = TempDir::new().unwrap();
        let path = write_env_file(&dir, ".env", "lower_case=value\n");
        let vars = parse_env_file(&path);
        assert_eq!(vars[0].0, "LOWER_CASE");
    }

    #[test]
    fn test_parse_env_file_value_with_equals() {
        let dir = TempDir::new().unwrap();
        let path = write_env_file(
            &dir,
            ".env",
            "DATABASE_URL=postgres://user:pass@host/db?opt=val\n",
        );
        let vars = parse_env_file(&path);
        assert_eq!(vars[0].0, "DATABASE_URL");
        assert_eq!(vars[0].1, "postgres://user:pass@host/db?opt=val");
    }
}
