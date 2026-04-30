use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::Path;
use std::process;

use crate::config::load_config;
use crate::container::{self, BuildSpec, RunSpec, Runtime, DEFAULT_WORKDIR};
use crate::errors::{ErrorCode, FlooError};
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

    let cwd = super::read_cwd_or_exit();

    let resolved = match project_config::resolve_app_context(&cwd, app_flag.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let app = super::resolve_app_or_exit(&client, &resolved.app_name);
    let app_id = app.id.clone();

    // Working dir = the service's `path` from floo.app.toml.
    let working_dir = {
        let app_config = match super::load_app_config_for_resolved_app(&resolved) {
            Ok(c) => c,
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
                let dir = resolved.config_dir.join(p);
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

    // --- Container preflight: Dockerfile + runtime must both exist before we
    // touch the API. Failing here costs zero on the platform side and gives
    // the user a clear error message.
    let dockerfile_path = working_dir.join("Dockerfile");
    if !dockerfile_path.exists() {
        output::error(
            &format!(
                "Service '{service}' has no Dockerfile at {}.",
                dockerfile_path.display()
            ),
            &ErrorCode::DockerfileMissing,
            Some(
                "floo run executes inside the same container that ships to production. \
                 Add a Dockerfile to the service path, or run 'floo init' to scaffold one.",
            ),
        );
        process::exit(1);
    }

    let runtime = match container::detect_runtime() {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // --- Create scoped dev session (env vars + Postgres IP auth, no listener).
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

    // --- Ensure image exists, building if needed.
    let image = match ensure_image(
        runtime,
        &resolved.app_name,
        service,
        &working_dir,
        &dockerfile_path,
    ) {
        Ok(tag) => tag,
        Err(e) => {
            let _ = client.delete_dev_session(&app_id, &session_id);
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // --- Compose the container run.
    let workdir = read_workdir(&dockerfile_path);
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    if let Some(svc_env) = session.services.get(service) {
        for (k, v) in svc_env {
            env.insert(k.clone(), v.clone());
        }
    }

    let (bin, args) = command.split_first().expect("command is non-empty");
    let shell_cmd = shell_join(bin, args);

    let interactive_stdin = std::io::stdin().is_terminal();
    let interactive_stdout = std::io::stdout().is_terminal();
    let allocate_tty = !output::is_json_mode() && interactive_stdin && interactive_stdout;

    let spec = RunSpec {
        image,
        workdir_in_container: workdir.clone(),
        source_mount_host: working_dir.clone(),
        env,
        command: shell_cmd,
        ports: BTreeMap::new(),
        preserved_paths: container::default_preserved_paths(),
        name: container::container_name(&resolved.app_name, service),
        interactive: interactive_stdin,
        tty: allocate_tty,
        init: true,
    };

    let status = match container::run_foreground(runtime, &spec) {
        Ok(s) => s,
        Err(e) => {
            let _ = client.delete_dev_session(&app_id, &session_id);
            output::error(&e.message, &e.code, e.suggestion.as_deref());
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
                "container": {
                    "runtime": runtime.binary(),
                    "image": spec.name,
                    "workdir": workdir,
                },
            }
        }));
    }

    let _ = client.delete_dev_session(&app_id, &session_id);
    process::exit(exit_code);
}

fn ensure_image(
    runtime: Runtime,
    app: &str,
    service: &str,
    context_dir: &Path,
    dockerfile: &Path,
) -> Result<String, FlooError> {
    let dockerfile_content = std::fs::read_to_string(dockerfile).map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::DockerfileMissing,
            format!("Failed to read {}: {e}", dockerfile.display()),
            "Check file permissions on the Dockerfile.".to_string(),
        )
    })?;
    let hash = container::compute_build_hash(&dockerfile_content, context_dir);
    let tag = container::image_tag(app, service, &hash);

    if container::image_exists(runtime, &tag) {
        return Ok(tag);
    }

    if !output::is_json_mode() {
        output::info(
            &format!("Building dev image '{tag}' (first run or deps changed)..."),
            None,
        );
    }
    container::build_image(
        runtime,
        &BuildSpec {
            tag: tag.clone(),
            context_dir: context_dir.to_path_buf(),
            dockerfile: dockerfile.to_path_buf(),
        },
    )?;
    Ok(tag)
}

fn read_workdir(dockerfile: &Path) -> String {
    std::fs::read_to_string(dockerfile)
        .ok()
        .and_then(|c| container::parse_workdir(&c))
        .unwrap_or_else(|| DEFAULT_WORKDIR.to_string())
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
        assert_eq!(
            shell_join("/path/to my/script", &["arg".into()]),
            "'/path/to my/script' arg"
        );
    }

    #[test]
    fn shell_quote_embedded_single_quote() {
        assert_eq!(shell_quote_if_needed("it's"), "'it'\\''s'");
    }
}
