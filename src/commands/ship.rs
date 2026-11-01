//! `floo deploy`: push a local repository to a floo-hosted project and wait for
//! its dev deploy. It chains steps an agent can already run one by one
//! (`floo projects create`, a git push with the project credential helper,
//! `floo deploys watch --commit`) and stops with the next command when the
//! folder is not ready, instead of guessing.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

use crate::api_types::ProjectResponse;
use crate::errors::ErrorCode;
use crate::output;
use crate::project_config;

use super::projects::{managed_app_id, shell_quote};

/// The git remote `floo deploy` adds and pushes to.
pub(crate) const REMOTE: &str = "floo";

fn stop(message: &str, code: &str, next: &str) -> ! {
    output::error(message, &ErrorCode::from_api(code), Some(next));
    process::exit(1);
}

fn git(path: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(path);
    command
}

/// Runs a git command that only reads, returning trimmed stdout on success.
fn git_read(path: &Path, args: &[&str]) -> Option<String> {
    let output = git(path).args(args).stderr(Stdio::null()).output();
    match output {
        Ok(output) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(_) => None,
        Err(error) if error.kind() == io::ErrorKind::NotFound => stop(
            "Git is not installed or is not on PATH.",
            "GIT_NOT_FOUND",
            "Install git, then run floo deploy again.",
        ),
        Err(error) => stop(
            &format!("Failed to run git: {error}"),
            "GIT_FAILED",
            "Run floo deploy again.",
        ),
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Where the push goes.
enum Target {
    /// The folder already has a `floo` remote for this managed project.
    Linked { clone_url: String },
    /// A project with the app's name exists; add the remote and push normally.
    Existing(ProjectResponse),
    /// No project yet: create one. Its only commit is the starter, so the
    /// first push replaces it.
    New,
}

pub fn deploy(path: PathBuf) {
    let display = path.display().to_string();
    let quoted = shell_quote(&display);

    let app_name = match project_config::load_app_config(&path) {
        Ok(Some(config)) => config.app.name,
        Ok(None) => stop(
            &format!("No floo.app.toml in {display}."),
            "NO_CONFIG_FOUND",
            &format!("floo init <app-name> --path {quoted}"),
        ),
        Err(error) => {
            output::error(&error.message, &error.code, error.suggestion.as_deref());
            process::exit(1);
        }
    };

    let Some(top_level) = git_read(&path, &["rev-parse", "--show-toplevel"]) else {
        stop(
            &format!("{display} is not a git repository."),
            "NOT_A_GIT_REPOSITORY",
            &format!(
                "git -C {quoted} init && git -C {quoted} add -A && git -C {quoted} commit -m \"Initial commit\" && floo deploy {quoted}"
            ),
        )
    };
    if !same_dir(Path::new(&top_level), &path) {
        stop(
            &format!(
                "floo.app.toml must sit at the repository root ({top_level}); floo builds the pushed repository from its root."
            ),
            "INVALID_PATH",
            &format!("Move floo.app.toml to {top_level}, then run floo deploy {}.", shell_quote(&top_level)),
        );
    }
    let Some(sha) = git_read(&path, &["rev-parse", "--verify", "HEAD"]) else {
        stop(
            "The repository has no commits yet.",
            "NO_COMMITS",
            &format!("git -C {quoted} add -A && git -C {quoted} commit -m \"Initial commit\" && floo deploy {quoted}"),
        )
    };
    let status = git_read(&path, &["status", "--porcelain"]).unwrap_or_default();
    if !status.is_empty() {
        stop(
            "The working tree has uncommitted changes. floo deploys commits, so they would be left out.",
            "UNCOMMITTED_CHANGES",
            &format!("git -C {quoted} add -A && git -C {quoted} commit -m \"<message>\" && floo deploy {quoted}"),
        );
    }

    let linked = git_read(&path, &["remote", "get-url", REMOTE]);
    if let Some(url) = &linked {
        let managed = url
            .strip_prefix("https://github.com/")
            .and_then(managed_app_id)
            .is_some();
        if !managed {
            stop(
                &format!("The git remote '{REMOTE}' points at {url}, not a floo-hosted project."),
                "INVALID_REMOTE",
                &format!("git -C {quoted} remote remove {REMOTE} && floo deploy {quoted}"),
            );
        }
    }

    if output::is_dry_run_mode() {
        let action = if linked.is_some() {
            "push"
        } else {
            "create-or-push"
        };
        output::dry_run_preview(
            &format!(
                "Would push commit {} of app '{app_name}' to its floo-hosted project{} and wait for the dev deploy.",
                &sha[..sha.len().min(12)],
                if linked.is_some() { "" } else { ", creating the project if it does not exist" },
            ),
            serde_json::json!({
                "action": action,
                "app": app_name,
                "commit": sha,
                "remote": REMOTE,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);
    let target = match linked {
        Some(clone_url) => Target::Linked { clone_url },
        None => match client.get_project(&app_name) {
            Ok(project) => Target::Existing(project),
            Err(error) if error.is_not_found() || error.code == "PROJECT_NOT_FOUND" => Target::New,
            Err(error) => {
                output::error(&error.message, &ErrorCode::from_api(&error.code), None);
                process::exit(1);
            }
        },
    };

    let (clone_url, branch, force) = match target {
        Target::Linked { clone_url } => {
            let project = client.get_project(&app_name).unwrap_or_else(|error| {
                stop(
                    &error.message,
                    &error.code,
                    "Check [app] name in floo.app.toml matches the project, or run floo projects list.",
                )
            });
            if !clone_url
                .trim_end_matches(".git")
                .eq_ignore_ascii_case(project.clone_url.trim_end_matches(".git"))
            {
                stop(
                    &format!(
                        "The '{REMOTE}' remote belongs to a different project than app '{app_name}' in floo.app.toml."
                    ),
                    "APP_NAME_MISMATCH",
                    &format!("git -C {quoted} remote remove {REMOTE} && floo deploy {quoted}"),
                );
            }
            (project.clone_url, project.default_branch, false)
        }
        Target::Existing(project) => {
            add_remote(&path, &project.clone_url);
            (project.clone_url, project.default_branch, false)
        }
        Target::New => {
            if !output::is_json_mode() {
                eprintln!(
                    "Creating floo project '{app_name}' (hosted, invite-only login, postgres)..."
                );
            }
            let project = client.create_project(&app_name).unwrap_or_else(|error| {
                let hint = error
                    .extra
                    .as_ref()
                    .and_then(|extra| extra.get("hint"))
                    .and_then(|hint| hint.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        "Change [app] name in floo.app.toml, or run floo projects list.".to_string()
                    });
                stop(&error.message, &error.code, &hint)
            });
            add_remote(&path, &project.clone_url);
            (project.clone_url, project.default_branch, true)
        }
    };

    push(&path, &clone_url, &branch, force, &quoted);
    super::deploys::watch(Some(&app_name), Some(&sha));
}

fn add_remote(path: &Path, clone_url: &str) {
    if git_read(path, &["remote", "add", REMOTE, clone_url]).is_none() {
        stop(
            &format!("Failed to add the git remote '{REMOTE}'."),
            "GIT_FAILED",
            &format!(
                "git -C {} remote add {REMOTE} {clone_url}",
                shell_quote(&path.display().to_string())
            ),
        );
    }
}

fn push(path: &Path, clone_url: &str, branch: &str, force: bool, quoted: &str) {
    // Only credential-free managed HTTPS URLs reach git, as in `projects clone`.
    if clone_url
        .strip_prefix("https://github.com/")
        .and_then(managed_app_id)
        .is_none()
    {
        stop(
            "The project's clone URL is not a floo-hosted repository.",
            "INVALID_RESPONSE",
            "Run floo projects list and contact floo support with the project name.",
        );
    }
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned))
        .unwrap_or_else(|| {
            stop(
                "The floo binary path must be valid Unicode for git's credential helper.",
                "INVALID_PATH",
                "Install floo in a path without special characters.",
            )
        });
    #[cfg(windows)]
    let executable = executable.replace('\\', "/");
    let helper = format!(
        "credential.helper=!{} projects git-credential",
        shell_quote(&executable)
    );
    let refspec = format!("HEAD:refs/heads/{branch}");
    let mut command = git(path);
    // Reset inherited helpers so no keychain stores the short-lived token.
    command
        .args(["-c", "credential.helper=", "-c", &helper])
        .args(["-c", "credential.useHttpPath=true", "push"]);
    if force {
        command.arg("--force");
    }
    let status = command
        .args([REMOTE, &refspec])
        // Keep stdout for floo's JSON.
        .stdout(Stdio::from(io::stderr()))
        .status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => stop(
            &format!("git push failed ({status}). Check the git error above."),
            "GIT_PUSH_FAILED",
            &if force {
                format!("floo deploy {quoted}")
            } else {
                format!(
                    "git -C {quoted} pull --no-rebase {REMOTE} {branch} && floo deploy {quoted}"
                )
            },
        ),
        Err(error) => stop(
            &format!("Failed to run git push: {error}"),
            "GIT_FAILED",
            &format!("floo deploy {quoted}"),
        ),
    }
}
