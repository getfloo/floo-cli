use std::io::{self, BufRead};
use std::path::Path;
use std::process::{self, Command, Stdio};

use crate::api_client::FlooClient;
use crate::config::load_config;
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;

fn fail(error: FlooApiError) -> ! {
    let next = if error.status_code == 401 || error.code == "NOT_AUTHENTICATED" {
        "next: floo auth login"
    } else {
        "next: floo projects list"
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

pub fn create(name: &str) {
    let project = client()
        .create_project(name)
        .unwrap_or_else(|error| fail(error));
    let next = format!("floo projects clone {}", shell_quote(&project.name));
    let watch = project
        .first_deploy_id
        .as_ref()
        .map(|_| format!("floo deploys watch --app {}", shell_quote(&project.name)));
    let mut data = output::to_value(&project);
    data["login"] = serde_json::json!("hosted, invite-only");
    data["database"] = serde_json::json!("postgres");
    data["next"] = serde_json::json!(next);
    if let Some(watch) = &watch {
        data["watch"] = serde_json::json!(watch);
    }
    if output::is_json_mode() {
        output::success("Project created.", Some(data));
        return;
    }
    output::success("Project created.", None);
    output::info(
        &format!(
            "app: {}\nurl: {}\nlogin: hosted, invite-only\ndatabase: postgres\nrepo: {}",
            project.name, project.app_url, project.clone_url
        ),
        None,
    );
    if let Some(watch) = watch {
        output::info(&format!("follow first deploy: {watch}"), None);
    }
    output::info(&format!("next: {next}"), None);
}

pub fn list() {
    let result = client().list_projects().unwrap_or_else(|error| fail(error));
    let rows = result
        .projects
        .iter()
        .map(|project| {
            vec![
                project.name.clone(),
                project.app_url.clone(),
                project.clone_url.clone(),
            ]
        })
        .collect::<Vec<_>>();
    output::table(
        &["NAME", "URL", "REPO"],
        &rows,
        Some(output::to_value(&result)),
    );
}

// Git runs ! helpers through a POSIX shell, including Git for Windows. Quote
// the executable as one shell word: workspace paths may contain spaces or '.
fn shell_quote(value: &str) -> String {
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

pub fn clone(name_or_id: &str, dir: Option<&Path>) {
    let project = client()
        .get_project(name_or_id)
        .unwrap_or_else(|error| fail(error));
    // Only pass credential-free managed HTTPS URLs to git. This also prevents
    // clone URLs from invoking an arbitrary git transport or storing userinfo.
    let app_id = project
        .clone_url
        .strip_prefix("https://github.com/")
        .and_then(managed_app_id);
    if !app_id.is_some_and(|id| id.eq_ignore_ascii_case(&project.app_id)) {
        fail(FlooApiError::new(
            0,
            "INVALID_RESPONSE",
            "Project clone URL must be a managed GitHub HTTPS repository matching the app ID.",
        ));
    }
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
        "credential.helper=!{} projects git-credential",
        shell_quote(&executable)
    );
    let dir = dir.unwrap_or_else(|| Path::new(&project.name));
    let result = Command::new("git")
        .arg("clone")
        // Reset inherited helper lists so credential-store/keychains never see
        // the token during git's subsequent credential approval operation.
        .args(["-c", "credential.helper=", "-c", &helper])
        .args(["-c", "credential.useHttpPath=true"])
        .arg("--")
        .arg(&project.clone_url)
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
                    "next: floo projects clone {} {}",
                    shell_quote(name_or_id),
                    shell_quote(&dir.to_string_lossy())
                )),
            );
            process::exit(1);
        }
    }
    output::success(
        &format!("directory: {}\nnext: read AGENTS.md", dir.display()),
        Some(serde_json::json!({ "directory": dir, "project": project, "next": "read AGENTS.md" })),
    );
}

fn managed_app_id(path: &str) -> Option<String> {
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
