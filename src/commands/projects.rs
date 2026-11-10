use std::io::{self, BufRead};
use std::path::Path;
use std::process::{self, Command, Stdio};

use crate::api_client::FlooClient;
use crate::config::load_config;
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;
use crate::resolve::resolve_app;

fn fail(error: FlooApiError) -> ! {
    let next = if error.status_code == 401 || error.code == "NOT_AUTHENTICATED" {
        "next: floo auth login"
    } else {
        "next: floo apps list"
    };
    let hint = error
        .extra
        .as_ref()
        .and_then(|extra| extra.get("hint"))
        .and_then(|hint| hint.as_str());
    let suggestion = match hint {
        Some(hint) => format!("{hint}\n{next}"),
        None => next.to_string(),
    };
    output::error_with_details(
        &error.message,
        &ErrorCode::from_api(&error.code),
        Some(&suggestion),
        error.extra.as_ref(),
    );
    process::exit(1);
}

fn client() -> FlooClient {
    let config = load_config();
    if config.api_key.is_none() {
        fail(FlooApiError::new(
            401,
            "NOT_AUTHENTICATED",
            "Not logged in.",
        ));
    }
    FlooClient::new(Some(config)).unwrap_or_else(|error| fail(error))
}

// Git runs ! helpers through a POSIX shell, including Git for Windows. Quote
// the executable as one shell word: workspace paths may contain spaces or '.
pub(crate) fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_./-".contains(&byte))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// End `floo clone` on an app with no floo-hosted repository, naming what to run instead.
fn no_hosted_repo(message: &str, code: &str, next: &str) -> ! {
    output::error(message, &ErrorCode::from_api(code), Some(next));
    process::exit(1);
}

pub fn clone(name_or_id: &str, dir: Option<&Path>) {
    let app = resolve_app(&client(), name_or_id).unwrap_or_else(|error| fail(error));
    let repo = match app.source.as_ref() {
        Some(source) if source.kind == "floo" => source.repo.as_deref().unwrap_or_default(),
        Some(source) => {
            let repo = source.repo.as_deref().unwrap_or_default();
            no_hosted_repo(
                &format!(
                    "{} deploys from {repo} on GitHub. Clone it from there.",
                    app.name
                ),
                "SOURCE_IS_GITHUB",
                &format!("next: git clone https://github.com/{repo}.git"),
            )
        }
        None => no_hosted_repo(
            &format!(
                "{} has no code yet, so there is nothing to clone.",
                app.name
            ),
            "APP_HAS_NO_SOURCE",
            "next: floo deploy (in the app's folder)",
        ),
    };
    // Only pass credential-free managed HTTPS URLs to git. This also prevents
    // clone URLs from invoking an arbitrary git transport or storing userinfo.
    if !managed_app_id(repo).is_some_and(|id| id.eq_ignore_ascii_case(&app.id)) {
        fail(FlooApiError::new(
            0,
            "INVALID_RESPONSE",
            "The app's repository must be a floo-hosted GitHub repository matching the app ID.",
        ));
    }
    let clone_url = format!("https://github.com/{repo}.git");
    let executable = std::env::current_exe()
        .unwrap_or_else(|error| fail(FlooApiError::new(0, "FILE_ERROR", error.to_string())));
    let executable = executable
        .to_str()
        .unwrap_or_else(|| {
            fail(FlooApiError::new(
                0,
                "INVALID_PATH",
                "The floo binary path must be valid Unicode for git's credential helper.",
            ))
        })
        .to_owned();
    #[cfg(windows)]
    let executable = executable.replace('\\', "/");
    let helper = format!(
        "credential.helper=!{} git-credential",
        shell_quote(&executable)
    );
    let dir = dir.unwrap_or_else(|| Path::new(&app.name));
    let result = Command::new("git")
        .arg("clone")
        // Reset inherited helper lists so credential-store/keychains never see
        // the token during git's subsequent credential approval operation.
        .args(["-c", "credential.helper=", "-c", &helper])
        .args(["-c", "credential.useHttpPath=true"])
        .arg("--")
        .arg(&clone_url)
        .arg(dir)
        // Preserve floo's single-JSON-object stdout contract.
        .stdout(Stdio::from(io::stderr()))
        .status();
    match result {
        Ok(status) if status.success() => {}
        result => {
            let message = match result {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    "Git is not installed or is not on PATH. Install git in your workspace."
                        .to_string()
                }
                Err(error) => format!("Failed to run git clone: {error}"),
                Ok(status) => format!("git clone failed ({status}). Check the git error above."),
            };
            output::error(
                &message,
                &ErrorCode::from_api("GIT_CLONE_FAILED"),
                Some(&format!(
                    "next: floo clone {} {}",
                    shell_quote(name_or_id),
                    shell_quote(&dir.to_string_lossy())
                )),
            );
            process::exit(1);
        }
    }
    output::success(
        &format!("directory: {}\nnext: read AGENTS.md", dir.display()),
        Some(serde_json::json!({
            "directory": dir,
            "app": { "id": app.id, "name": app.name, "source": app.source },
            "next": "read AGENTS.md",
        })),
    );
}

pub(crate) fn managed_app_id(path: &str) -> Option<String> {
    let (org, repo) = path.split_once('/')?;
    if org.is_empty()
        || !org
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let (org_id, app_id) = repo.strip_prefix("floo-")?.split_once('-')?;
    if [org_id, app_id]
        .iter()
        .any(|id| id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return None;
    }
    Some(
        format!(
            "{}-{}-{}-{}-{}",
            &app_id[..8],
            &app_id[8..12],
            &app_id[12..16],
            &app_id[16..20],
            &app_id[20..]
        )
        .to_ascii_lowercase(),
    )
}

fn credential_app_id(input: impl BufRead) -> io::Result<Option<String>> {
    let (mut protocol, mut host, mut path) = (None, None, None);
    for line in input.lines() {
        let line = line?;
        if line.is_empty() {
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Ok(None);
        };
        let field = match key {
            "protocol" => &mut protocol,
            "host" => &mut host,
            "path" => &mut path,
            _ => continue,
        };
        if field.replace(value.to_string()).is_some() {
            return Ok(None);
        }
    }
    Ok(
        if protocol.as_deref() == Some("https") && host.as_deref() == Some("github.com") {
            path.as_deref().and_then(managed_app_id)
        } else {
            None
        },
    )
}

pub fn git_credential(operation: &str) {
    if operation != "get" {
        return;
    }
    let app_id = credential_app_id(io::stdin().lock()).unwrap_or_else(|_| {
        fail(FlooApiError::new(
            0,
            "FILE_ERROR",
            "Failed to read git credential request.",
        ))
    });
    let Some(app_id) = app_id else { return };
    let credential = client()
        .project_git_token(&app_id)
        .unwrap_or_else(|error| fail(error));
    output::git_credentials(&credential.token).unwrap_or_else(|_| {
        fail(FlooApiError::new(
            0,
            "INVALID_RESPONSE",
            "Failed to write git credentials: invalid token or closed output.",
        ))
    });
}
