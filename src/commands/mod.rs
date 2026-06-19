use std::process;

use crate::api_client::FlooClient;
use crate::api_types::App;
use crate::config::{load_config, FlooConfig};
use crate::errors::{ErrorCode, FlooError};
use crate::output;

pub mod analytics;
pub mod apps;
pub mod auth;
pub mod billing;
pub mod command_tree;
pub mod cron;
pub mod db;
pub mod deploy;
pub mod deploys;
pub mod dev;
pub mod docs;
pub mod doctor;
pub mod domains;
pub mod env;
pub mod feedback;
pub mod github;
pub mod init;
pub mod logs;
pub mod orgs;
pub mod releases;
pub mod reparo;
pub mod rollbacks;
pub mod run;

pub mod services;
pub mod skills;
pub mod storage;
pub mod update;

pub(crate) fn init_client(config: Option<FlooConfig>) -> FlooClient {
    match FlooClient::new(config) {
        Ok(client) => client,
        Err(error) => {
            output::error(
                &error.message,
                &ErrorCode::from_api(&error.code),
                Some("Check your network/TLS setup and try again."),
            );
            process::exit(1);
        }
    }
}

/// Read the current working directory, or print a `CwdError` and `exit(1)`.
///
/// Every command that resolves the app from local config (`dev`, `run`, env
/// import, logs, github connect, cron `--dry-run`, and `resolve_app_from_config`
/// itself) needs cwd before walking up for `floo.app.toml`. This is the single
/// place that turns a failed read into the user-facing error, so the
/// code/message/suggestion can't drift across call sites.
pub(crate) fn read_cwd_or_exit() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            &ErrorCode::CwdError,
            Some("Ensure the current directory exists and you have read permission."),
        );
        process::exit(1);
    })
}

pub(crate) fn require_auth() {
    let config = load_config();
    if config.api_key.is_none() {
        output::error(
            "Not logged in.",
            &ErrorCode::NotAuthenticated,
            Some("Run 'floo auth login' to authenticate."),
        );
        process::exit(1);
    }
}

pub(crate) fn resolve_app_or_exit(client: &FlooClient, app_name: &str) -> App {
    match crate::resolve::resolve_app(client, app_name) {
        Ok(a) => a,
        Err(e) => {
            // A 404 from resolving a single app means the app doesn't exist;
            // match on status, not a drift-prone code string (see is_not_found).
            if e.is_not_found() {
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
    }
}

pub(crate) fn resolve_app_from_config(
    client: &FlooClient,
    app_flag: Option<&str>,
) -> (String, String) {
    // Short-circuit: --app flag means we don't need local config
    if let Some(app_name) = app_flag {
        let app = resolve_app_or_exit(client, app_name);
        return (app.id.clone(), app.name.clone());
    }

    let cwd = read_cwd_or_exit();
    let resolved = match crate::project_config::resolve_app_context(&cwd, None) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };
    let app = resolve_app_or_exit(client, &resolved.app_name);
    (app.id.clone(), app.name.clone())
}

pub(crate) fn load_app_config_for_resolved_app(
    resolved: &crate::project_config::ResolvedApp,
) -> Result<crate::project_config::AppFileConfig, FlooError> {
    match crate::project_config::load_app_config(&resolved.config_dir)? {
        Some(config) => Ok(config),
        None => {
            // When the caller passed `--app`, the flag's help text reads
            // "reads from config if omitted", which suggests it can fully
            // substitute for the local config. It can't: `floo.app.toml`
            // is also where service definitions live, and `dev` / `run`
            // need those. Disambiguate here so the error doesn't read
            // like the flag was ignored.
            let suggestion = match resolved.source {
                crate::project_config::AppSource::Flag => {
                    "--app sets the app name but service definitions still come \
                     from floo.app.toml. cd to your project root, or run \
                     'floo init' if this directory is a new project."
                        .to_string()
                }
                _ => "Run 'floo init' to create a project config, or cd to your \
                      project root."
                    .to_string(),
            };
            Err(FlooError::with_suggestion(
                ErrorCode::NoConfigFound,
                format!(
                    "No {} found in '{}'.",
                    crate::project_config::APP_CONFIG_FILE,
                    resolved.config_dir.display()
                ),
                suggestion,
            ))
        }
    }
}

/// Truncate a commit SHA to 7 characters for display.
pub(crate) fn short_sha(sha: &str) -> &str {
    if sha.len() > 7 && sha.is_ascii() {
        &sha[..7]
    } else {
        sha
    }
}

/// Detect the deploy env file in a directory: prefers .floo.env, falls back to .env.
/// Used at config creation time (init, service add) to populate env_file in floo.service.toml.
pub(crate) fn detect_env_file(dir: &std::path::Path) -> Option<String> {
    if dir.join(".floo.env").exists() {
        Some(".floo.env".to_string())
    } else if dir.join(".env").exists() {
        Some(".env".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn load_app_config_for_resolved_app_uses_resolved_config_dir() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(crate::project_config::APP_CONFIG_FILE),
            r#"
[app]
name = "nested-app"

[services.web]
type = "web"
path = "./web"
port = 3000
dev_command = "npm run dev"
"#,
        )
        .unwrap();
        let nested = dir.path().join("apps/web");
        fs::create_dir_all(&nested).unwrap();

        let resolved = crate::project_config::ResolvedApp {
            app_name: "nested-app".to_string(),
            source: crate::project_config::AppSource::AppFile,
            service_config: None,
            app_config: None,
            config_dir: dir.path().to_path_buf(),
        };

        let config = load_app_config_for_resolved_app(&resolved).unwrap();
        assert_eq!(
            config.services["web"].dev_command.as_deref(),
            Some("npm run dev")
        );
    }

    #[test]
    fn load_app_config_for_resolved_app_reports_resolved_dir_in_error() {
        let dir = TempDir::new().unwrap();
        let resolved = crate::project_config::ResolvedApp {
            app_name: "missing-app".to_string(),
            source: crate::project_config::AppSource::AppFile,
            service_config: None,
            app_config: None,
            config_dir: dir.path().to_path_buf(),
        };

        let err = load_app_config_for_resolved_app(&resolved).unwrap_err();
        assert_eq!(err.code, ErrorCode::NoConfigFound);
        assert!(err.message.contains(&dir.path().display().to_string()));
    }

    #[test]
    fn load_app_config_for_resolved_app_explains_app_flag_when_used() {
        // Regression for feedback 65b28405 (2026-05-01): `floo dev --app X`
        // run from a directory without floo.app.toml errored with the same
        // generic suggestion as no-flag mode. The flag's --help reads "reads
        // from config if omitted" which implies it can substitute for the
        // file; the error must be honest that service definitions still
        // live in the file.
        let dir = TempDir::new().unwrap();
        let resolved = crate::project_config::ResolvedApp {
            app_name: "via-flag".to_string(),
            source: crate::project_config::AppSource::Flag,
            service_config: None,
            app_config: None,
            config_dir: dir.path().to_path_buf(),
        };

        let err = load_app_config_for_resolved_app(&resolved).unwrap_err();
        assert_eq!(err.code, ErrorCode::NoConfigFound);
        let suggestion = err.suggestion.expect("suggestion should be present");
        assert!(
            suggestion.contains("--app"),
            "suggestion should explain --app: {suggestion}"
        );
        assert!(
            suggestion.contains("service definitions"),
            "suggestion should mention service definitions: {suggestion}"
        );
    }
}
