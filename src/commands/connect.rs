//! `floo connect owner/repo`: make the customer's GitHub repo the app's source
//! (`PUT /v1/apps/{app_id}/source`). An app floo hosts moves: its history is
//! pushed into the empty GitHub repo with the user's own git credentials, then
//! floo switches the app and archives the hosted repo, and this folder's remote
//! is repointed. An app with no source connects; one that doesn't exist yet is
//! created on the repo.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Stdio};

use serde_json::{json, Value};

use crate::api_client::FlooClient;
use crate::api_types::AppSource;
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;

use super::projects::{managed_app_id, shell_quote};
use super::ship::{git, git_read, github_repo, hosted_git, stop};

pub struct ConnectArgs {
    pub repo: String,
    pub app: Option<String>,
    pub branch: Option<String>,
}

pub fn connect(args: ConnectArgs) {
    let repo = args.repo.trim_end_matches(".git").to_string();
    if github_repo(&format!("https://github.com/{repo}")).as_deref() != Some(repo.as_str()) {
        stop(
            &format!("{repo} is not a GitHub repository name."),
            "INVALID_REPO",
            "floo connect <owner>/<repo>, for example floo connect acme/storefront",
        );
    }
    let cwd = super::read_cwd_or_exit();
    let name = match args.app.clone() {
        Some(name) => name,
        None => match crate::project_config::resolve_app_context(&cwd, None) {
            Ok(resolved) => resolved.app_name,
            Err(error) if error.code == ErrorCode::NoConfigFound => {
                repo.rsplit('/').next().unwrap_or(&repo).to_string()
            }
            // A broken floo.app.toml must not pick some other app to move.
            Err(error) => {
                output::error(&error.message, &error.code, error.suggestion.as_deref());
                process::exit(1);
            }
        },
    };
    super::require_auth();
    let client = super::init_client(None);
    let app = match crate::resolve::resolve_app(&client, &name) {
        Ok(app) => app,
        Err(error) if error.is_not_found() => {
            return create(&client, &name, &repo, args.branch.as_deref());
        }
        // Prints the lookup error the way every other command does, and exits.
        Err(_) => super::resolve_app_or_exit(&client, &name),
    };
    let quoted = shell_quote(&repo);
    let rerun = format!("floo connect {quoted} --app {}", shell_quote(&app.name));
    require_access(&client, &repo, &rerun);

    let hosted = app.source.as_ref().filter(|source| source.kind == "floo");
    let branch = args
        .branch
        .clone()
        .or_else(|| hosted.and_then(|source| source.branch.clone()));
    if let Some(hosted) = hosted {
        push_history(hosted, &repo, &rerun);
    }
    let mut source = json!({"kind": "github", "repo": repo});
    if let Some(branch) = &branch {
        source["branch"] = json!(branch);
    }
    let mut change = client
        .set_app_source(&app.id, &source)
        .unwrap_or_else(|error| fail(&error, &rerun));
    // Also on a rerun after the switch landed, when the source is already the repo.
    let remotes = repoint(&cwd, &app.id, &repo);
    change["remotes_repointed"] = json!(remotes.repointed);
    change["remotes_removed"] = json!(remotes.removed);

    let message = change["message"]
        .as_str()
        .unwrap_or("Connected.")
        .to_string();
    if output::is_json_mode() {
        output::success(&message, Some(change));
        return;
    }
    output::success(&message, None);
    for remote in &remotes.repointed {
        output::info(
            &format!("git remote {remote} now points at https://github.com/{repo}.git."),
            None,
        );
    }
    for remote in &remotes.removed {
        output::info(
            &format!("Removed git remote {remote}, which pointed at the hosted repo."),
            None,
        );
    }
    output::info("Next: floo status", None);
}

fn create(client: &FlooClient, name: &str, repo: &str, branch: Option<&str>) {
    let rerun = format!(
        "floo connect {} --app {}",
        shell_quote(repo),
        shell_quote(name)
    );
    require_access(client, repo, &rerun);
    let mut source = json!({"kind": "github", "repo": repo});
    if let Some(branch) = branch {
        source["branch"] = json!(branch);
    }
    let app = client
        .create_app_with_source(name, &source)
        .unwrap_or_else(|error| fail(&error, &rerun));
    let branch = app
        .source
        .as_ref()
        .and_then(|source| source.branch.clone())
        .unwrap_or_else(|| "main".to_string());
    let message = format!(
        "Created {} on {repo}@{branch}. A push to {branch} deploys dev, and floo is building its newest commit now.",
        app.name
    );
    if output::is_json_mode() {
        output::success(&message, Some(output::to_value(&app)));
        return;
    }
    output::success(&message, None);
    output::info("Next: floo status", None);
}

fn fail(error: &FlooApiError, rerun: &str) -> ! {
    let hint = error
        .extra
        .as_ref()
        .and_then(|extra| extra.get("hint"))
        .and_then(Value::as_str);
    output::error_with_details(
        &error.message,
        &ErrorCode::from_api(&error.code),
        Some(hint.unwrap_or(rerun)),
        error.extra.as_ref(),
    );
    process::exit(1);
}

/// Stop before anything moves when floo's GitHub App can't read the repo.
fn require_access(client: &FlooClient, repo: &str, rerun: &str) {
    let access = client
        .github_check_repo_access(repo)
        .unwrap_or_else(|error| fail(&error, rerun));
    if access["accessible"] == true {
        return;
    }
    let page = |field: &str| access[field].as_str().filter(|url| !url.is_empty());
    let owner = repo.split('/').next().unwrap_or(repo);
    let (message, next) = match access["reason"].as_str() {
        Some("REPO_NOT_IN_INSTALLATION") => (
            format!("floo's GitHub App is installed for your account but can't see {repo}."),
            format!(
                "Open {} and add {repo}, then {rerun}",
                page("settings_url").unwrap_or("https://github.com/settings/installations")
            ),
        ),
        Some("REPO_NOT_ACCESSIBLE") => (
            format!("floo's GitHub App can't read {repo}."),
            format!("Check the app's access to {repo} on GitHub, then {rerun}"),
        ),
        _ => (
            format!("floo's GitHub App isn't installed where it can reach {repo}."),
            match super::ship::github_setup_link(client, repo) {
                Some(link) => format!("Open {link}, install the App on {owner}, then {rerun}"),
                None => format!("Install the floo GitHub App on {owner}, then {rerun}"),
            },
        ),
    };
    stop(&message, "GITHUB_REPO_NOT_ACCESSIBLE", &next);
}

/// Push every branch and tag of the hosted repo into the empty GitHub repo, with
/// the user's own git credentials. A rerun after a push that already landed
/// carries on; a repo with other commits is refused, so nothing is overwritten.
fn push_history(hosted: &AppSource, repo: &str, rerun: &str) {
    let scratch = Scratch::new();
    let moved = move_history(&scratch.0, hosted, repo, rerun);
    drop(scratch); // `stop` exits, which would skip the cleanup.
    if let Err(Stop(message, code, next)) = moved {
        stop(&message, code, &next);
    }
}

struct Stop(String, &'static str, String);

fn move_history(scratch: &Path, hosted: &AppSource, repo: &str, rerun: &str) -> Result<(), Stop> {
    let hosted_repo = hosted.repo.clone().unwrap_or_default();
    let hosted_url = format!("https://github.com/{hosted_repo}.git");
    let target_url = format!("https://github.com/{repo}.git");
    let mirror = scratch.join("hosted.git");
    let cloned = hosted_git(scratch, &hosted_url)
        .args(["clone", "--bare", "--quiet", "--"])
        .arg(&hosted_url)
        .arg(&mirror)
        .stdout(Stdio::from(io::stderr()))
        .status()
        .is_ok_and(|status| status.success());
    if !cloned {
        return Err(Stop(
            format!("Could not read the hosted repo {hosted_repo}. Nothing changed."),
            "GIT_CLONE_FAILED",
            rerun.to_string(),
        ));
    }
    let Some(target) = git_read(&mirror, &["ls-remote", &target_url]) else {
        return Err(Stop(
            format!(
                "git can't reach {repo} with your credentials. Create it as an empty repo first; nothing changed."
            ),
            "REPO_NOT_FOUND",
            format!("gh repo create {repo} --private && {rerun}"),
        ));
    };
    let hosted_refs = git_read(
        &mirror,
        &[
            "for-each-ref",
            "--format=%(objectname)\t%(refname)",
            "refs/heads",
            "refs/tags",
        ],
    )
    .unwrap_or_default();
    // A branch or tag the hosted repo doesn't have at the same commit is someone
    // else's history. Refs that match are a move that already landed in part; the
    // push below fills in the rest.
    let theirs = target.lines().any(|line| {
        let name = line.split_once('\t').map_or("", |(_, name)| name);
        let compared = (name.starts_with("refs/heads/") || name.starts_with("refs/tags/"))
            && !name.ends_with("^{}");
        compared && !hosted_refs.lines().any(|hosted| hosted == line)
    });
    if theirs {
        return Err(Stop(
            format!(
                "{repo} already has commits. floo moves the hosted history only into an empty repo, so nothing is overwritten."
            ),
            "REPO_NOT_EMPTY",
            "Create an empty repo (gh repo create <owner>/<new-repo> --private), then floo connect <owner>/<new-repo>".to_string(),
        ));
    }
    if hosted_refs.is_empty() {
        return Ok(()); // The hosted repo has no commits: nothing to move.
    }
    if !output::is_json_mode() {
        eprintln!("Pushing the hosted history to {repo}...");
    }
    let pushed = git(&mirror)
        .args(["push", "--quiet", &target_url])
        .args(["refs/heads/*:refs/heads/*", "refs/tags/*:refs/tags/*"])
        .stdout(Stdio::from(io::stderr()))
        .status()
        .is_ok_and(|status| status.success());
    if pushed {
        return Ok(());
    }
    Err(Stop(
        format!("git push to {repo} failed. Check the git error above; nothing changed in floo."),
        "GIT_PUSH_FAILED",
        rerun.to_string(),
    ))
}

/// A scratch folder for the hosted repo's bare clone, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("floo-connect-{}", process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        if std::fs::create_dir_all(&dir).is_err() {
            stop(
                "Could not create a scratch folder for the move.",
                "FILE_ERROR",
                "Check the system temp folder is writable, then run floo connect again.",
            );
        }
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Remotes {
    repointed: Vec<String>,
    removed: Vec<String>,
}

/// Point this folder's remotes for the app's hosted repo at the GitHub repo, so
/// `floo deploy` pushes there: fetch and push URLs both, each keeping its
/// protocol. When a remote already fetches from the GitHub repo, a remote that
/// fetches from the hosted one is removed instead. URLs are read as configured,
/// before any `insteadOf` rewrite, and compared as GitHub repos, so SSH and HTTPS
/// match. The hosted repo is found by the app id in its name, so a rerun after
/// the switch still finds it.
fn repoint(cwd: &Path, app_id: &str, repo: &str) -> Remotes {
    let configured = git_read(
        cwd,
        &["config", "--get-regexp", r"^remote\..*\.(url|pushurl)$"],
    )
    .unwrap_or_default();
    // (remote, url or pushurl, the URL as configured, the GitHub repo it names)
    let found: Vec<(&str, &str, &str, String)> = configured
        .lines()
        .filter_map(|line| {
            let (key, url) = line.split_once(' ')?;
            let (name, field) = key.strip_prefix("remote.")?.rsplit_once('.')?;
            Some((name, field, url, github_repo(url)?))
        })
        .collect();
    let hosted =
        |found: &str| managed_app_id(found).is_some_and(|id| id.eq_ignore_ascii_case(app_id));
    let has_target = found
        .iter()
        .any(|(_, field, _, found)| *field == "url" && found.eq_ignore_ascii_case(repo));
    let mut names: Vec<&str> = Vec::new();
    for (name, _, _, found) in &found {
        if hosted(found) && !names.contains(name) {
            names.push(name);
        }
    }
    let mut done = Remotes::default();
    for name in names {
        let fetches_hosted = found
            .iter()
            .any(|(remote, field, _, found)| *remote == name && *field == "url" && hosted(found));
        if has_target && fetches_hosted {
            if git_read(cwd, &["remote", "remove", name]).is_some() {
                done.removed.push(name.to_string());
            } else {
                output::warn(&format!(
                    "Could not remove git remote {name}; run git remote remove {name}."
                ));
            }
            continue;
        }
        let mut repointed = true;
        for (_, field, url, found) in found
            .iter()
            .filter(|(remote, _, _, found)| *remote == name && hosted(found))
        {
            let key = format!("remote.{name}.{field}");
            let moved = swap_repo(url, found, repo);
            let args = [
                "config",
                "--fixed-value",
                "--replace-all",
                &key,
                &moved,
                url,
            ];
            repointed &= git_read(cwd, &args).is_some();
        }
        if repointed {
            done.repointed.push(name.to_string());
        } else {
            output::warn(&format!(
                "Could not repoint git remote {name}; run git remote set-url {name} https://github.com/{repo}.git."
            ));
        }
    }
    done
}

/// `url` with the GitHub repo `from` replaced by `to`, so SSH stays SSH.
fn swap_repo(url: &str, from: &str, to: &str) -> String {
    match url.to_ascii_lowercase().find(&from.to_ascii_lowercase()) {
        Some(at) => format!("{}{to}{}", &url[..at], &url[at + from.len()..]),
        None => format!("https://github.com/{to}.git"),
    }
}
