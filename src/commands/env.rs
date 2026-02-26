use std::path::Path;
use std::process;

use crate::api_client::FlooClient;
use crate::output;
use crate::project_config;
use crate::resolve::resolve_app;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_app_id(client: &FlooClient, app_flag: Option<&str>) -> (String, String) {
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            "FILE_ERROR",
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

    let app_data = match resolve_app(client, &resolved.app_name) {
        Ok(a) => a,
        Err(e) => {
            if e.code == "APP_NOT_FOUND" {
                output::error(
                    &format!("App '{}' not found.", resolved.app_name),
                    "APP_NOT_FOUND",
                    Some("Check the app name or ID and try again."),
                );
            } else {
                output::error(&e.message, &e.code, None);
            }
            process::exit(1);
        }
    };

    let app_id = super::expect_str_field(&app_data, "id").to_string();
    let app_name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&resolved.app_name)
        .to_string();
    (app_id, app_name)
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
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let services = match result.get("services").and_then(|v| v.as_array()) {
        Some(arr) => arr.clone(),
        None => {
            output::error(
                "Response missing 'services' field.",
                "PARSE_ERROR",
                Some("This is a bug. Please report it."),
            );
            process::exit(1);
        }
    };

    if service_names.is_empty() {
        match services.len() {
            0 => return vec![(None, None)],
            1 => {
                let svc = &services[0];
                let sid = super::expect_str_field(svc, "id").to_string();
                let sname = super::expect_str_field(svc, "name").to_string();
                return vec![(Some(sid), Some(sname))];
            }
            _ => {
                let names: Vec<String> = services
                    .iter()
                    .filter_map(|s| s.get("name").and_then(|v| v.as_str()).map(String::from))
                    .collect();
                output::error(
                    &format!(
                        "App '{app_name}' has multiple services. Specify which with --services."
                    ),
                    "MULTIPLE_SERVICES",
                    Some(&format!("Available services: {}", names.join(", "))),
                );
                process::exit(1);
            }
        }
    }

    service_names
        .iter()
        .map(|name| {
            let found = services
                .iter()
                .find(|s| s.get("name").and_then(|v| v.as_str()) == Some(name.as_str()));
            match found {
                Some(svc) => {
                    let sid = super::expect_str_field(svc, "id").to_string();
                    (Some(sid), Some(name.clone()))
                }
                None => {
                    let available: Vec<String> = services
                        .iter()
                        .filter_map(|s| s.get("name").and_then(|v| v.as_str()).map(String::from))
                        .collect();
                    output::error(
                        &format!("Service '{name}' not found on app '{app_name}'."),
                        "SERVICE_NOT_FOUND",
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
            "INTERNAL_ERROR",
            Some("This is a bug. Please report it."),
        );
        process::exit(1);
    })
}

fn parse_env_file(path: &Path) -> Vec<(String, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            let (code, msg) = match e.kind() {
                std::io::ErrorKind::NotFound => (
                    "ENV_FILE_NOT_FOUND",
                    format!("File not found: {}", path.display()),
                ),
                std::io::ErrorKind::PermissionDenied => (
                    "ENV_FILE_NOT_FOUND",
                    format!("Permission denied reading: {}", path.display()),
                ),
                _ => (
                    "ENV_FILE_NOT_FOUND",
                    format!("Failed to read {}: {e}", path.display()),
                ),
            };
            output::error(&msg, code, Some("Check the file path and permissions."));
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
            "ENV_PARSE_ERROR",
            Some("Each non-comment line must be in KEY=VALUE format."),
        );
        process::exit(1);
    }

    if vars.is_empty() {
        output::error(
            &format!("No valid KEY=VALUE pairs found in {}.", path.display()),
            "ENV_PARSE_ERROR",
            Some("Ensure the file contains lines in KEY=VALUE format."),
        );
        process::exit(1);
    }

    vars
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

pub fn set(key_value: &str, app_flag: Option<&str>, service_names: &[String]) {
    super::require_auth();

    if !key_value.contains('=') {
        output::error(
            "Invalid format. Use KEY=VALUE.",
            "INVALID_FORMAT",
            Some("Example: floo env set DATABASE_URL=postgres://..."),
        );
        process::exit(1);
    }

    let (key, value) = key_value.split_once('=').unwrap();
    let key = key.to_uppercase();

    let client = super::init_client(None);
    let (app_id, app_name) = resolve_app_id(&client, app_flag);
    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);

    for (service_id, service_name) in &targets {
        match client.set_env_var(&app_id, &key, value, service_id.as_deref()) {
            Ok(result) => {
                let target = match service_name {
                    Some(sn) => format!("{app_name}/{sn}"),
                    None => app_name.clone(),
                };
                output::success(&format!("Set {key} on {target}."), Some(result));
            }
            Err(e) => {
                output::error(&e.message, &e.code, None);
                process::exit(1);
            }
        }
    }
}

pub fn list(app_flag: Option<&str>, service_names: &[String]) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = resolve_app_id(&client, app_flag);
    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);

    for (service_id, service_name) in &targets {
        let result = match client.list_env_vars(&app_id, service_id.as_deref()) {
            Ok(r) => r,
            Err(e) => {
                output::error(&e.message, &e.code, None);
                process::exit(1);
            }
        };

        let env_vars = match result.get("env_vars").and_then(|v| v.as_array()) {
            Some(arr) => arr.clone(),
            None => {
                output::error(
                    "Response missing 'env_vars' field.",
                    "PARSE_ERROR",
                    Some("This is a bug. Please report it."),
                );
                process::exit(1);
            }
        };

        let target = match service_name {
            Some(sn) => format!("{app_name}/{sn}"),
            None => app_name.clone(),
        };

        if env_vars.is_empty() {
            if output::is_json_mode() {
                output::success("No env vars.", Some(serde_json::json!({"env_vars": []})));
            } else {
                output::info(&format!("No environment variables set on {target}."), None);
            }
            continue;
        }

        let rows: Vec<Vec<String>> = env_vars
            .iter()
            .map(|ev| {
                vec![
                    ev.get("key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string(),
                    ev.get("masked_value")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string(),
                ]
            })
            .collect();

        output::table(
            &["Key", "Value"],
            &rows,
            Some(serde_json::json!({"env_vars": env_vars})),
        );
    }
}

pub fn remove(key: &str, app_flag: Option<&str>, service_names: &[String]) {
    let key = key.to_uppercase();
    super::require_auth();

    let client = super::init_client(None);
    let (app_id, app_name) = resolve_app_id(&client, app_flag);
    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);

    for (service_id, service_name) in &targets {
        if let Err(e) = client.delete_env_var(&app_id, &key, service_id.as_deref()) {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }

        let target = match service_name {
            Some(sn) => format!("{app_name}/{sn}"),
            None => app_name.clone(),
        };
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
    let (app_id, app_name) = resolve_app_id(&client, app_flag);
    let (service_id, _service_name) =
        resolve_single_service(&client, &app_id, &app_name, service_flag);

    let result = match client.get_env_var(&app_id, &key, service_id.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let value = super::expect_str_field(&result, "value");

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
    super::require_auth();

    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            "FILE_ERROR",
            None,
        );
        process::exit(1);
    });

    // Resolve config once — used for both env_file default and app resolution.
    // NO_CONFIG_FOUND is acceptable when determining env_file path (fall back to .env),
    // but all other errors (LEGACY_CONFIG, INVALID_PROJECT_CONFIG) must surface.
    let resolved = match project_config::resolve_app_context(&cwd, app_flag) {
        Ok(r) => Some(r),
        Err(e) if e.code == "NO_CONFIG_FOUND" => None,
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

    let vars = parse_env_file(&env_file_path);
    let count = vars.len();

    // Use resolved app name if available, otherwise resolve_app_id will re-resolve from config.
    let client = super::init_client(None);
    let (app_id, app_name) = match &resolved {
        Some(r) => {
            let app_data = match resolve_app(&client, &r.app_name) {
                Ok(a) => a,
                Err(e) => {
                    if e.code == "APP_NOT_FOUND" {
                        output::error(
                            &format!("App '{}' not found.", r.app_name),
                            "APP_NOT_FOUND",
                            Some("Check the app name or ID and try again."),
                        );
                    } else {
                        output::error(&e.message, &e.code, None);
                    }
                    process::exit(1);
                }
            };
            let app_id = super::expect_str_field(&app_data, "id").to_string();
            let app_name = app_data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&r.app_name)
                .to_string();
            (app_id, app_name)
        }
        None => resolve_app_id(&client, app_flag),
    };

    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);

    for (service_id, service_name) in &targets {
        match client.import_env_vars(&app_id, &vars, service_id.as_deref()) {
            Ok(result) => {
                let target = match service_name {
                    Some(sn) => format!("{app_name}/{sn}"),
                    None => app_name.clone(),
                };
                output::success(
                    &format!("Imported {count} variable(s) to {target}."),
                    Some(result),
                );
            }
            Err(e) => {
                output::error(&e.message, &e.code, None);
                process::exit(1);
            }
        }
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
