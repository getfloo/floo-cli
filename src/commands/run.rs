use std::collections::HashMap;
use std::process::{self, Command, Stdio};

use crate::config::load_config;
use crate::errors::ErrorCode;
use crate::output;
use crate::project_config;

pub fn run(service: &str, app_flag: Option<String>, command: Vec<String>) {
    super::require_auth();

    if command.is_empty() {
        output::error(
            "No command provided.",
            &ErrorCode::InvalidProjectConfig,
            Some("Usage: floo run --service <name> -- <command...>"),
        );
        process::exit(1);
    }

    let config = load_config();
    let client = super::init_client(Some(config));

    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            &ErrorCode::CwdError,
            Some("Ensure the current directory exists and you have read permission."),
        );
        process::exit(1);
    });

    let resolved = match project_config::resolve_app_context(&cwd, app_flag.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let app = super::resolve_app_or_exit(&client, &resolved.app_name);
    let app_id = app.id.clone();

    // Resolve the working directory: use the service's `path` from floo.app.toml.
    let working_dir = {
        let app_config = match project_config::load_app_config(&cwd) {
            Ok(Some(c)) => c,
            Ok(None) => {
                output::error(
                    "No floo.app.toml found in current directory.",
                    &ErrorCode::NoConfigFound,
                    Some("Run 'floo init' to create a project config, or cd to your project root."),
                );
                process::exit(1);
            }
            Err(e) => {
                output::error(&e.message, &e.code, e.suggestion.as_deref());
                process::exit(1);
            }
        };

        let entry = match app_config.services.get(service) {
            Some(e) => e,
            None => {
                output::error(
                    &format!("Service '{service}' not found in floo.app.toml."),
                    &ErrorCode::InvalidProjectConfig,
                    Some(
                        "Check the service name or run 'floo services list' to see deployed services.",
                    ),
                );
                process::exit(1);
            }
        };

        match &entry.path {
            Some(p) => {
                let dir = cwd.join(p);
                if !dir.exists() {
                    output::error(
                        &format!("Service '{service}' path '{p}' does not exist."),
                        &ErrorCode::InvalidPath,
                        Some("Check the 'path' value in floo.app.toml."),
                    );
                    process::exit(1);
                }
                dir
            }
            None => cwd.clone(),
        }
    };

    // Create a portless dev session: gets env vars + postgres IP auth without
    // starting a local server. The API skips cross-service discovery for portless entries.
    let api_services = vec![crate::api_types::DevSessionService {
        name: service.to_string(),
        port: None,
    }];

    let spinner = output::Spinner::new("Fetching env vars...");
    let session = match client.create_dev_session(&app_id, &api_services) {
        Ok(s) => {
            spinner.finish();
            s
        }
        Err(e) => {
            spinner.finish();
            output::error(
                &format!("Failed to create dev session: {}", e.message),
                &ErrorCode::from_api(&e.code),
                None,
            );
            process::exit(1);
        }
    };

    let session_id = session.session_id.clone();

    if !output::is_json_mode() {
        output::info(&format!("Running with env from '{service}'"), None);
        if session.postgres_authorized {
            output::info(
                "  Postgres: your IP is authorized for direct connections",
                None,
            );
        }
        eprintln!();
    }

    // Build env: inherit current env, then overlay platform vars for the service.
    let mut env_vars: HashMap<String, String> = std::env::vars().collect();
    if let Some(svc_env) = session.services.get(service) {
        for (k, v) in svc_env {
            env_vars.insert(k.clone(), v.clone());
        }
    }

    // Run the command, inheriting stdin/stdout/stderr so test output flows through naturally.
    let (bin, args) = command.split_first().expect("command is non-empty");
    let status = match Command::new("sh")
        .arg("-c")
        .arg(shell_join(bin, args))
        .current_dir(&working_dir)
        .envs(&env_vars)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
    {
        Ok(s) => s,
        Err(e) => {
            // Clean up the session before exiting — process::exit() skips destructors.
            let _ = client.delete_dev_session(&app_id, &session_id);
            output::error(
                &format!("Failed to run command: {e}"),
                &ErrorCode::InternalError,
                None,
            );
            process::exit(1);
        }
    };

    let exit_code = status.code().unwrap_or(1);

    if output::is_json_mode() {
        output::print_json(&serde_json::json!({
            "success": exit_code == 0,
            "data": {
                "exit_code": exit_code,
                "session_id": session_id,
                "postgres_authorized": session.postgres_authorized,
            }
        }));
    }

    // Tear down the dev session before exit. process::exit() skips destructors,
    // so this must be an explicit call — not a Drop impl.
    let _ = client.delete_dev_session(&app_id, &session_id);
    process::exit(exit_code);
}

/// Build a shell command string, quoting any argument that contains whitespace
/// or shell metacharacters. Both `bin` and `args` are quoted with the same rules.
fn shell_join(bin: &str, args: &[String]) -> String {
    let mut parts = vec![shell_quote_if_needed(bin)];
    for arg in args {
        parts.push(shell_quote_if_needed(arg));
    }
    parts.join(" ")
}

fn shell_quote_if_needed(s: &str) -> String {
    let needs_quoting = s.contains(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | '\\'
                    | '$'
                    | '`'
                    | '!'
                    | '#'
                    | '&'
                    | '|'
                    | ';'
                    | '('
                    | ')'
                    | '<'
                    | '>'
                    | '*'
                    | '?'
                    | '['
                    | ']'
                    | '~'
                    | '{'
                    | '}'
            )
    });
    if needs_quoting {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_join_plain_args() {
        assert_eq!(
            shell_join("pytest", &["tests/unit/".into()]),
            "pytest tests/unit/"
        );
    }

    #[test]
    fn shell_join_arg_with_spaces() {
        assert_eq!(
            shell_join("echo", &["hello world".into()]),
            "echo 'hello world'"
        );
    }

    #[test]
    fn shell_join_no_args() {
        assert_eq!(shell_join("pytest", &[]), "pytest");
    }

    #[test]
    fn shell_join_flag_args() {
        assert_eq!(
            shell_join("pytest", &["-x".into(), "-v".into(), "tests/".into()]),
            "pytest -x -v tests/"
        );
    }

    #[test]
    fn shell_join_dollar_sign_quoted() {
        assert_eq!(shell_join("echo", &["$SECRET".into()]), "echo '$SECRET'");
    }

    #[test]
    fn shell_join_pipe_quoted() {
        assert_eq!(shell_join("echo", &["foo|bar".into()]), "echo 'foo|bar'");
    }

    #[test]
    fn shell_join_semicolon_quoted() {
        assert_eq!(
            shell_join("echo", &["foo;rm -rf /".into()]),
            "echo 'foo;rm -rf /'"
        );
    }

    #[test]
    fn shell_join_bin_with_spaces_quoted() {
        // Binary name with spaces is quoted like any other token.
        assert_eq!(
            shell_join("/path/to my/script", &["arg".into()]),
            "'/path/to my/script' arg"
        );
    }

    #[test]
    fn shell_quote_embedded_single_quote() {
        // Single quote in an argument is escaped correctly.
        assert_eq!(shell_quote_if_needed("it's"), "'it'\\''s'");
    }
}
