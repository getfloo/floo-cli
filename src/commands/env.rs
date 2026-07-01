use std::path::{Path, PathBuf};
use std::process;

use crate::api_client::FlooClient;
use crate::errors::ErrorCode;
use crate::output;
use crate::project_config;

const DEPLOY_HINT: &str =
    "Push a commit to trigger a deploy with the updated env vars, or run: floo redeploy";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate that an `env_file` path from config stays within the project root.
///
/// Rejects absolute paths and paths that escape the project via `..`.
/// Returns the canonicalized path on success.
pub(crate) fn validate_env_file_path(
    env_file: &str,
    project_root: &Path,
) -> Result<PathBuf, String> {
    let p = Path::new(env_file);
    if p.is_absolute() {
        return Err(format!(
            "env_file must be a relative path, got absolute path: {env_file}"
        ));
    }
    let joined = project_root.join(env_file);
    let canonical = joined
        .canonicalize()
        .map_err(|e| format!("env_file '{}' could not be resolved: {e}", joined.display()))?;
    let root_canonical = project_root.canonicalize().map_err(|e| {
        format!(
            "Project root '{}' could not be resolved: {e}",
            project_root.display()
        )
    })?;
    if !canonical.starts_with(&root_canonical) {
        return Err(format!(
            "env_file '{}' resolves to '{}' which is outside the project directory.",
            env_file,
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn format_target(app_name: &str, service_name: Option<&str>) -> String {
    match service_name {
        Some(sn) => format!("{app_name}/{sn}"),
        None => app_name.to_string(),
    }
}

/// Render `--services` and `--env` scope as a parenthesized suffix for dry-run
/// previews — e.g. " (services: api, web) (env: prod)".
///
/// Codex review flagged that env dry-run previews previously hid the actual
/// scope: `floo env set KEY=val --services api --env prod --dry-run` rendered
/// as "Would set KEY on app foo." even though the real command would only
/// touch the api service in prod. The preview must reflect the target the
/// real command would mutate, otherwise an agent or human verifying a prod
/// change can't tell from the preview that prod is what they're about to
/// touch.
fn format_env_scope(service_names: &[String], env: &str) -> String {
    let mut suffix = String::new();
    if !service_names.is_empty() {
        suffix.push_str(&format!(" (services: {})", service_names.join(", ")));
    }
    if env != "dev" {
        suffix.push_str(&format!(" (env: {env})"));
    }
    suffix
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
    let result = match client.list_services(app_id, None) {
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

/// Strip a single trailing newline (`\n` or `\r\n`) from a value read off a
/// side channel. `echo "$SECRET"` and most editors append one; stripping
/// exactly one keeps the common case ergonomic without mangling a value whose
/// content genuinely contains newlines. Documented on the `--stdin` /
/// `--value-file` flags so the behavior is not a surprise.
fn strip_one_trailing_newline(mut s: String) -> String {
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
    s
}

/// Read an env-var value from stdin (`--stdin`). Used to keep secrets out of
/// argv / shell history / `ps`.
fn read_stdin_value() -> String {
    use std::io::Read;
    let mut buf = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
        output::error(
            &format!("Failed to read value from stdin: {e}"),
            &ErrorCode::FileError,
            Some("Pipe the value in, e.g. printf %s \"$SECRET\" | floo env set KEY --stdin"),
        );
        process::exit(1);
    }
    strip_one_trailing_newline(buf)
}

/// Read an env-var value from a file (`--value-file PATH`).
fn read_value_file(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(c) => strip_one_trailing_newline(c),
        Err(e) => {
            let msg = match e.kind() {
                std::io::ErrorKind::NotFound => format!("Value file not found: {}", path.display()),
                std::io::ErrorKind::PermissionDenied => {
                    format!("Permission denied reading: {}", path.display())
                }
                _ => format!("Failed to read {}: {e}", path.display()),
            };
            output::error(
                &msg,
                &ErrorCode::FileError,
                Some("Check the file path and permissions."),
            );
            process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

pub fn set(
    key_value: &str,
    app_flag: Option<&str>,
    service_names: &[String],
    restart: bool,
    env: &str,
    read_stdin: bool,
    value_file: Option<&Path>,
) {
    let from_side_channel = read_stdin || value_file.is_some();

    // Resolve the key. With `--stdin` / `--value-file` the positional is the
    // bare KEY and the value arrives off the command line, keeping secrets out
    // of argv, shell history, and `ps` (#1152). Otherwise it is `KEY=VALUE`.
    let key = if from_side_channel {
        if key_value.contains('=') {
            output::error(
                "With --stdin or --value-file, pass the key only (no =VALUE).",
                &ErrorCode::InvalidFormat,
                Some("Example: printf %s \"$SECRET\" | floo env set API_KEY --stdin"),
            );
            process::exit(1);
        }
        key_value.to_uppercase()
    } else {
        if !key_value.contains('=') {
            output::error(
                "Invalid format. Use KEY=VALUE (or KEY with --stdin / --value-file).",
                &ErrorCode::InvalidFormat,
                Some("Example: floo env set DATABASE_URL=postgres://..."),
            );
            process::exit(1);
        }
        key_value.split_once('=').unwrap().0.to_uppercase()
    };

    if output::is_dry_run_mode() {
        // Dry-run mutates nothing, so it must NOT consume stdin or read the
        // file — it previews the key/target only.
        let target = app_flag.unwrap_or("(reads from config)");
        let scope = format_env_scope(service_names, env);
        let restart_clause = if restart { " and restart" } else { "" };
        let source = if read_stdin {
            " (value from stdin)"
        } else if value_file.is_some() {
            " (value from file)"
        } else {
            ""
        };
        let preview = format!("Would set {key} on {target}{scope}{restart_clause}{source}.");
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "env_set",
                "key": key,
                "app": app_flag,
                "services": service_names,
                "env": env,
                "will_restart": restart,
                "value_source": if read_stdin { "stdin" } else if value_file.is_some() { "file" } else { "inline" },
            }),
        );
        return;
    }

    super::require_auth();

    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);

    // Read the side-channel value only AFTER every "this set can't run" check
    // (auth, app/target resolution) has passed. Reading first would block an
    // interactive `--stdin` on EOF — or consume a secret file/FIFO — for a
    // logged-out user or a multi-service app with no `--services`, then error
    // anyway (codex #1152). Dry-run already returned above without reading.
    let value: String = if read_stdin {
        read_stdin_value()
    } else if let Some(path) = value_file {
        read_value_file(path)
    } else {
        key_value.split_once('=').unwrap().1.to_string()
    };

    let mut last_env_result = serde_json::Value::Null;
    for (service_id, service_name) in &targets {
        match client.set_env_var(&app_id, &key, &value, service_id.as_deref(), env) {
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

        // cancelled = the target env was torn down before the restart ran: a moot
        // terminal (not a failure — exit 0), but don't claim it "Restarted".
        let restart_msg = if final_status == "cancelled" {
            "Restart cancelled: its target environment was removed before it ran.".to_string()
        } else {
            format!("Restarted {url}")
        };
        output::success(
            &restart_msg,
            Some(serde_json::json!({"env": last_env_result, "deploy": deploy_data})),
        );
    }
}

pub fn list(app_flag: Option<&str>, service_names: &[String], env: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);

    // No `--services` => "list all env vars" (the documented behavior, and the
    // `--help` example). A multi-service app no longer errors with
    // MULTIPLE_SERVICES; it reads every service's vars plus app-level in one
    // pass (#1152). With `--services`, each named service is listed in turn.
    if service_names.is_empty() {
        list_all_services(&client, &app_id, &app_name, env);
        return;
    }

    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);
    for (service_id, service_name) in &targets {
        let result = match client.list_env_vars(&app_id, service_id.as_deref(), env) {
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

/// List every env var on the app — app-level plus every service's — in a single
/// read. The API returns all vars when `service_id` is omitted.
///
/// Scope labelling is by construction, not a heuristic: a row's scope (app-level
/// vs which service) is ambiguous exactly when the app HAS a service — then
/// "app-level vs service-scoped" is a real distinction every row needs spelled
/// out. A service-less app has a single possible scope (app-level), so it gets
/// the plain `Key`/`Value` table. `services.is_empty()` is the whole rule; the
/// earlier scope-count / span conditions were guard-pile refinements that each
/// left another row-shape unlabelled (#1152 architectural pause).
fn list_all_services(client: &FlooClient, app_id: &str, app_name: &str, env: &str) {
    let result = match client.list_env_vars(app_id, None, env) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if result.env_vars.is_empty() {
        if output::is_json_mode() {
            output::success("No env vars.", Some(serde_json::json!({"env_vars": []})));
        } else {
            output::info(
                &format!("No environment variables set on {app_name}."),
                None,
            );
        }
        return;
    }

    let services = match client.list_services(app_id, None) {
        Ok(r) => r.services,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if services.is_empty() {
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
        return;
    }

    let service_name = |service_id: &Option<String>| -> String {
        match service_id {
            Some(id) => services
                .iter()
                .find(|s| s.id == *id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "(unknown)".to_string()),
            None => "(app)".to_string(),
        }
    };

    let rows: Vec<Vec<String>> = result
        .env_vars
        .iter()
        .map(|ev| {
            vec![
                ev.key.clone(),
                service_name(&ev.service_id),
                ev.masked_value.as_deref().unwrap_or("-").to_string(),
            ]
        })
        .collect();

    output::table(
        &["Key", "Service", "Value"],
        &rows,
        Some(output::to_value(&result)),
    );
}

pub fn remove(key: &str, app_flag: Option<&str>, service_names: &[String], env: &str) {
    let key = key.to_uppercase();

    if output::is_dry_run_mode() {
        let target = app_flag.unwrap_or("(reads from config)");
        let scope = format_env_scope(service_names, env);
        let preview = format!("Would remove {key} from {target}{scope}.");
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "env_remove",
                "key": key,
                "app": app_flag,
                "services": service_names,
                "env": env,
            }),
        );
        return;
    }

    super::require_auth();

    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let targets = resolve_service_ids(&client, &app_id, &app_name, service_names);

    for (service_id, service_name) in &targets {
        if let Err(e) = client.delete_env_var(&app_id, &key, service_id.as_deref(), env) {
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

pub fn get(key: &str, app_flag: Option<&str>, service_flag: Option<&str>, env: &str) {
    let key = key.to_uppercase();
    super::require_auth();

    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let (service_id, _service_name) =
        resolve_single_service(&client, &app_id, &app_name, service_flag);

    let result = match client.get_env_var(&app_id, &key, service_id.as_deref(), env) {
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
        // JSON mode redacts secret-shaped values at the `print_json` boundary
        // (unless `--reveal-secrets`) and stamps `contains_secrets`. Leave it to
        // that chokepoint so `env get --json` matches every other command.
        output::success(
            &format!("Got {key}."),
            Some(serde_json::json!({"key": key, "value": value})),
        );
    } else if crate::redact::is_secret(&key, value) && !crate::redact::is_reveal_secrets() {
        // Human mode mirrors the JSON redactor's classifier: refuse to print a
        // secret-shaped value to the terminal (and into shell history / a
        // transcript) without an explicit opt-in. Erroring rather than printing
        // a placeholder keeps `env get`'s contract honest — it returns the
        // usable value or fails, never a lying "***REDACTED***" stand-in (#1152).
        output::error(
            &format!("{key} looks like a secret; refusing to print it in plaintext."),
            &ErrorCode::SecretRevealRequired,
            Some("Re-run with --reveal-secrets to print the value."),
        );
        process::exit(1);
    } else {
        output::raw_value(value);
    }
}

pub fn import_vars(
    file_flag: Option<&Path>,
    app_flag: Option<&str>,
    service_names: &[String],
    env: &str,
) {
    let cwd = super::read_cwd_or_exit();

    // Resolve config — used for env_file default path. Missing config is OK.
    // When --app is provided, config errors in the current dir are irrelevant
    // (agent may be in an unrelated directory).
    let resolved = match project_config::resolve_app_context(&cwd, app_flag) {
        Ok(r) => Some(r),
        Err(e) if e.code == ErrorCode::NoConfigFound => None,
        Err(e) => {
            if app_flag.is_some() {
                None
            } else {
                output::error(&e.message, &e.code, e.suggestion.as_deref());
                process::exit(1);
            }
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
                Some(f) => match validate_env_file_path(f, &cwd) {
                    Ok(p) => p,
                    Err(msg) => {
                        output::error(&msg, &ErrorCode::InvalidPath, None);
                        process::exit(1);
                    }
                },
                None => cwd.join(".env"),
            }
        }
    };

    if output::is_dry_run_mode() {
        let vars = parse_env_file(&env_file_path);
        let keys: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        let count = vars.len();
        let target = app_flag
            .map(String::from)
            .or_else(|| resolved.as_ref().map(|r| r.app_name.clone()))
            .unwrap_or_else(|| "(reads from config)".to_string());
        let scope = format_env_scope(service_names, env);
        let preview = format!(
            "Would import {count} variable(s) from {} to {target}{scope}.\nKeys: {}",
            env_file_path.display(),
            keys.join(", "),
        );
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "env_import",
                "file": env_file_path.display().to_string(),
                "app": target,
                "services": service_names,
                "env": env,
                "keys": keys,
                "count": count,
            }),
        );
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
        match client.import_env_vars(&app_id, &vars, service_id.as_deref(), env) {
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

pub fn import_all_services(app_flag: Option<&str>, env: &str) {
    super::require_auth();

    let cwd = super::read_cwd_or_exit();

    let resolved = match project_config::resolve_app_context(&cwd, app_flag) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // Collect (service_name, env_file_path) pairs from all service configs
    let mut env_file_entries: Vec<(String, PathBuf)> = Vec::new();

    // Check root service
    if let Some(ref svc_config) = resolved.service_config {
        if let Some(ref env_file) = svc_config.service.env_file {
            match validate_env_file_path(env_file, &resolved.config_dir) {
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
                        match validate_env_file_path(env_file, &svc_dir) {
                            Ok(path) => {
                                env_file_entries.push((svc_config.service.name.clone(), path))
                            }
                            Err(msg) => {
                                output::error(&msg, &ErrorCode::InvalidPath, None);
                                process::exit(1);
                            }
                        }
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
    let server_services = match client.list_services(&app_id, None) {
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

        // Find service ID on server — refuse to fall back to app-scope if not found
        let service_id = server_services
            .iter()
            .find(|s| s.name == *svc_name)
            .map(|s| s.id.as_str());

        if service_id.is_none() {
            output::error(
                &format!(
                    "Service '{}' not found on server. Cannot import env vars — refusing to widen scope to app level.",
                    svc_name
                ),
                &ErrorCode::ServiceNotFound,
                Some("Deploy the app first so services are created, then re-run this command."),
            );
            process::exit(1);
        }

        match client.import_env_vars(&app_id, &vars, service_id, env) {
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

    #[test]
    fn test_strip_one_trailing_newline() {
        assert_eq!(strip_one_trailing_newline("secret\n".to_string()), "secret");
        assert_eq!(
            strip_one_trailing_newline("secret\r\n".to_string()),
            "secret"
        );
        assert_eq!(strip_one_trailing_newline("secret".to_string()), "secret");
        // Only ONE trailing newline is stripped — a value that genuinely ends in
        // a blank line keeps the rest.
        assert_eq!(
            strip_one_trailing_newline("secret\n\n".to_string()),
            "secret\n"
        );
        // Interior newlines (multi-line values) are preserved.
        assert_eq!(
            strip_one_trailing_newline("line1\nline2\n".to_string()),
            "line1\nline2"
        );
        assert_eq!(strip_one_trailing_newline(String::new()), "");
    }
}
