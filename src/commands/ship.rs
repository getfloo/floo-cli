//! `floo deploy`: ask the API where this folder's code deploys from
//! (`POST /v1/deploy/plan`), then create, connect or push as the plan says and
//! wait for the dev deploy. The plan is read-only, so a stop creates nothing,
//! claims no name, and names the fix. The API makes the decision so the CLI,
//! MCP and HTTP agents get the same answer.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

use serde_json::{json, Value};

use crate::api_client::FlooClient;
use crate::api_types::{AppSource, DeployPlan};
use crate::errors::ErrorCode;
use crate::output;
use crate::project_config;

use super::projects::{managed_app_id, shell_quote};

/// The remote a hosted repo gets when `origin` already points elsewhere.
const HOSTED_REMOTE: &str = "floo";

/// What the user passed to `floo deploy`.
pub struct DeployArgs {
    pub path: PathBuf,
    /// No remote yet: create a private GitHub repo in the user's account.
    pub github: bool,
    /// Let floo host a copy of the code instead of using GitHub.
    pub hosted: bool,
    /// Rename the app in floo.app.toml (and commit that) before deploying.
    pub name: Option<String>,
}

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
    git_stdout(path, args).map(|stdout| stdout.trim().to_string())
}

/// Runs a git command that only reads, returning its stdout as git wrote it.
fn git_stdout(path: &Path, args: &[&str]) -> Option<String> {
    let output = git(path).args(args).stderr(Stdio::null()).output();
    match output {
        Ok(output) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).into_owned())
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

/// A git remote as it may leave the machine: a name and a URL with no credentials.
/// `push_urls` are where `git push` sends it, which a `pushurl` can make differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Remote {
    pub name: String,
    pub url: String,
    pub push_urls: Vec<String>,
}

impl Remote {
    fn points_at(url: &str, repo: &str) -> bool {
        github_repo(url).is_some_and(|found| found.eq_ignore_ascii_case(repo))
    }

    /// Both fetching and pushing reach `repo`, so a push deploys what the plan saw.
    fn reaches(&self, repo: &str) -> bool {
        Self::points_at(&self.url, repo)
            && self.push_urls.iter().all(|url| Self::points_at(url, repo))
    }
}

/// Drops any `user:token@` from a remote URL, so a token in `origin` is never sent
/// or printed. scp-style `git@host:owner/repo` keeps its user, which is not a secret.
pub(crate) fn strip_credentials(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    format!("{scheme}://{host}{path}")
}

/// The `owner/repo` a remote URL points at on github.com, if it does.
pub(crate) fn github_repo(url: &str) -> Option<String> {
    let url = strip_credentials(url);
    let path = if let Some((_, rest)) = url.split_once("://") {
        let (host, path) = rest.split_once('/')?;
        let host = host.rsplit_once(':').map_or(host, |(host, _)| host);
        if !host.eq_ignore_ascii_case("github.com") {
            return None;
        }
        path
    } else {
        let (host, path) = url.split_once(':')?;
        let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
        if !host.eq_ignore_ascii_case("github.com") {
            return None;
        }
        path
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    let valid = |part: &str, extra: &[u8]| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || extra.contains(&byte))
    };
    (valid(owner, b"") && valid(repo, b"._")).then(|| format!("{owner}/{repo}"))
}

/// What the plan needs to know about the folder: remotes, branch and HEAD.
struct Folder {
    remotes: Vec<Remote>,
    branch: Option<String>,
    tracked: Option<String>,
    head: String,
}

impl Folder {
    fn read(path: &Path, head: String) -> Self {
        let names = git_read(path, &["remote"]).unwrap_or_default();
        let remotes = names
            .lines()
            .filter(|name| !name.is_empty())
            .take(20) // the plan reads at most 20
            .filter_map(|name| {
                let url = git_read(path, &["remote", "get-url", name])?;
                let push_urls = git_read(path, &["remote", "get-url", "--push", "--all", name])
                    .unwrap_or_default();
                Some(Remote {
                    name: name.to_string(),
                    url: strip_credentials(&url),
                    push_urls: push_urls.lines().map(strip_credentials).collect(),
                })
            })
            .collect();
        let branch = git_read(path, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
        let tracked = branch.as_deref().and_then(|branch| {
            git_read(
                path,
                &["config", "--get", &format!("branch.{branch}.remote")],
            )
        });
        Self {
            remotes,
            branch,
            tracked,
            head,
        }
    }

    fn plan_body(&self, app: &str, source: Option<&str>) -> Value {
        let remotes: serde_json::Map<String, Value> = self
            .remotes
            .iter()
            .map(|remote| (remote.name.clone(), Value::String(remote.url.clone())))
            .collect();
        json!({
            "app": app,
            "remotes": remotes,
            "remote": self.tracked,
            "branch": self.branch,
            "head": self.head,
            "source": source,
        })
    }

    /// The remote that fetches from and pushes to `repo`, preferring the tracked one.
    fn remote_for(&self, repo: &str) -> Option<&Remote> {
        let matches = |remote: &&Remote| remote.reaches(repo);
        let tracked = self.tracked.as_deref();
        self.remotes
            .iter()
            .filter(matches)
            .find(|remote| Some(remote.name.as_str()) == tracked)
            .or_else(|| self.remotes.iter().find(matches))
    }
}

/// Files that look like secrets: committing them would publish them.
pub(crate) fn looks_secret(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let env_example = [".example", ".sample", ".template", ".dist"]
        .iter()
        .any(|suffix| name.ends_with(suffix));
    (name == ".env" || name.starts_with(".env.")) && !env_example
        || [".pem", ".key", ".p12", ".pfx"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
        || ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"].contains(&name.as_str())
}

/// Stop on uncommitted work, naming secret-looking files first so the fix never
/// tells the user to commit them.
fn require_clean_tree(path: &Path, quoted: &str) {
    let status = git_stdout(
        path,
        &["status", "--porcelain", "-z", "--untracked-files=all"],
    )
    .unwrap_or_default();
    if status.is_empty() {
        return;
    }
    let secrets: Vec<String> = changed_files(&status)
        .into_iter()
        .filter(|file| looks_secret(file))
        .map(str::to_string)
        .collect();
    if !secrets.is_empty() {
        let ignore = secrets
            .iter()
            .map(|file| shell_quote(file))
            .collect::<Vec<_>>()
            .join(" ");
        stop(
            &format!(
                "{} look{} like secrets and git would commit {}: {}. Keep secrets out of the repository and set them with floo env set.",
                if secrets.len() == 1 { "This file" } else { "These files" },
                if secrets.len() == 1 { "s" } else { "" },
                if secrets.len() == 1 { "it" } else { "them" },
                secrets.join(", ")
            ),
            "SECRET_FILES_NOT_IGNORED",
            &format!(
                "printf '%s\\n' {ignore} >> {}/.gitignore, commit .gitignore, then floo env import <file> after the first deploy",
                quoted
            ),
        );
    }
    stop(
        "The working tree has uncommitted changes. floo deploys commits, so they would be left out.",
        "UNCOMMITTED_CHANGES",
        &format!("git -C {quoted} add -A && git -C {quoted} commit -m \"<message>\" && floo deploy {quoted}"),
    );
}

/// The paths in `git status --porcelain -z` output. Each entry is `XY path`, and a
/// rename or copy is followed by its old path as an entry of its own.
pub(crate) fn changed_files(status: &str) -> Vec<&str> {
    let mut files = Vec::new();
    let mut entries = status.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let code = entry.get(..2).unwrap_or_default();
        if let Some(file) = entry.get(3..) {
            files.push(file);
        }
        if code.contains('R') || code.contains('C') {
            entries.next();
        }
    }
    files
}

/// Write `name` into floo.app.toml's [app] table and commit just that file. Runs
/// only after the plan accepted the name, so a refused name changes nothing.
fn rename_app(path: &Path, name: &str, quoted: &str) -> String {
    let file = path.join("floo.app.toml");
    let content = std::fs::read_to_string(&file).unwrap_or_else(|error| {
        stop(
            &format!("Could not read floo.app.toml: {error}"),
            "CONFIG_ERROR",
            "Check the file, then floo deploy again.",
        )
    });
    let Some(updated) = project_config::set_app_name(&content, name) else {
        stop(
            "floo.app.toml has no name = \"...\" line under [app].",
            "CONFIG_INVALID",
            &format!("Add name = \"{name}\" under [app] in floo.app.toml, commit it, then floo deploy {quoted}"),
        );
    };
    let message = format!("Name the floo app {name}");
    let committed = std::fs::write(&file, updated).is_ok()
        && git_read(
            path,
            &["commit", "-q", "-m", &message, "--", "floo.app.toml"],
        )
        .is_some();
    if !committed {
        // Put the old name back so a failed rename leaves the folder as it was.
        let _ = std::fs::write(&file, &content);
        stop(
            "Could not commit the new name in floo.app.toml (check git config user.name and user.email).",
            "CONFIG_WRITE_ERROR",
            &format!("Set name = \"{name}\" under [app], commit it, then floo deploy {quoted}"),
        );
    }
    git_read(path, &["rev-parse", "HEAD"]).unwrap_or_default()
}

pub fn deploy(args: DeployArgs) {
    let path = args.path.as_path();
    let display = path.display().to_string();
    let quoted = shell_quote(&display);

    let configured = match project_config::load_app_config(path) {
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

    let Some(top_level) = git_read(path, &["rev-parse", "--show-toplevel"]) else {
        stop(
            &format!("{display} is not a git repository."),
            "NOT_A_GIT_REPOSITORY",
            &format!(
                "git -C {quoted} init && git -C {quoted} add -A && git -C {quoted} commit -m \"Initial commit\" && floo deploy {quoted}"
            ),
        )
    };
    if !same_dir(Path::new(&top_level), path) {
        stop(
            &format!(
                "floo.app.toml must sit at the repository root ({top_level}); floo builds the pushed repository from its root."
            ),
            "INVALID_PATH",
            &format!("Move floo.app.toml to {top_level}, then run floo deploy {}.", shell_quote(&top_level)),
        );
    }
    if git_read(path, &["rev-parse", "--verify", "HEAD"]).is_none() {
        stop(
            "The repository has no commits yet.",
            "NO_COMMITS",
            &format!("git -C {quoted} add -A && git -C {quoted} commit -m \"Initial commit\" && floo deploy {quoted}"),
        )
    }
    require_clean_tree(path, &quoted);
    let app_name = args.name.clone().unwrap_or_else(|| configured.clone());
    let head = git_read(path, &["rev-parse", "HEAD"]).unwrap_or_default();
    let mut folder = Folder::read(path, head);
    let choice = if args.github {
        Some("github")
    } else if args.hosted {
        Some("floo")
    } else {
        None
    };

    let logged_in = crate::config::load_config().api_key.is_some();
    if output::is_dry_run_mode() && !logged_in {
        output::dry_run_preview(
            &format!(
                "Would plan and deploy commit {} of app '{app_name}'. Sign in (floo auth login) to see where it deploys from.",
                super::short_sha(&folder.head)
            ),
            json!({
                "action": "plan",
                "app": app_name,
                "commit": folder.head,
                "remotes": folder.remotes.iter().map(|r| json!({"name": r.name, "url": r.url})).collect::<Vec<_>>(),
                "branch": folder.branch,
                "source": choice,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);
    let mut plan = ask_plan(&client, &folder, &app_name, choice);
    if output::is_dry_run_mode() {
        output::dry_run_preview(&plan.message, output::to_value(&plan));
        return;
    }
    if plan.ok && plan.decision.as_deref() == Some("create_github_repo") {
        create_github_repo(path, &app_name, &quoted);
        folder = Folder::read(path, folder.head);
        plan = ask_plan(&client, &folder, &app_name, None);
    }
    if !plan.ok {
        stop_with_plan(&plan, &quoted);
    }
    if app_name != configured {
        folder.head = rename_app(path, &app_name, &quoted);
    }
    emit_plan(&plan);

    let Some(mut source) = plan.source.clone() else {
        stop_with_plan(&plan, &quoted)
    };
    match plan.decision.as_deref() {
        Some("push") => {}
        Some("connect_github") => {
            let app = create_app(
                &client,
                &app_name,
                json!({"kind": "github", "repo": source.repo, "branch": source.branch}),
            );
            source = app.source.unwrap_or(source);
        }
        Some("create_hosted") => {
            // The hosted repo takes this folder's branch, so the branch that deploys
            // dev now keeps deploying dev on every later run.
            let mut hosted = json!({"kind": "floo"});
            if let Some(branch) = folder.branch.as_deref().filter(|b| api_branch_name(b)) {
                hosted["branch"] = json!(branch);
            }
            let app = create_app(&client, &app_name, hosted);
            let Some(hosted) = app.source else {
                stop(
                    "floo created the app but did not return its hosted repository.",
                    "INVALID_RESPONSE",
                    &format!("floo deploy {quoted}"),
                )
            };
            source = hosted;
        }
        _ => stop_with_plan(&plan, &quoted),
    }
    ship(path, &folder, &app_name, &source, &quoted);
}

/// Branch names the API accepts for a source.
fn api_branch_name(branch: &str) -> bool {
    branch.len() <= 255
        && branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._/-".contains(&byte))
}

fn ask_plan(client: &FlooClient, folder: &Folder, app: &str, source: Option<&str>) -> DeployPlan {
    client
        .plan_deploy(&folder.plan_body(app, source))
        .unwrap_or_else(|error| {
            // PLAN_REQUIRED and friends carry recovery fields such as upgrade_url.
            output::error_with_details(
                &error.message,
                &ErrorCode::from_api(&error.code),
                None,
                error.extra.as_ref(),
            );
            process::exit(1);
        })
}

fn create_app(client: &FlooClient, name: &str, source: Value) -> crate::api_types::App {
    if !output::is_json_mode() {
        eprintln!("Creating app '{name}'...");
    }
    client
        .create_app_with_source(name, &source)
        .unwrap_or_else(|error| {
            let hint = error
                .extra
                .as_ref()
                .and_then(|extra| extra.get("hint"))
                .and_then(Value::as_str);
            output::error_with_details(
                &error.message,
                &ErrorCode::from_api(&error.code),
                hint,
                error.extra.as_ref(),
            );
            process::exit(1);
        })
}

/// The plan's first step for a person at a terminal: the command, or the page to open.
fn suggestion(plan: &DeployPlan, quoted: &str) -> String {
    if plan.code.as_deref() == Some("APP_NAME_TAKEN") {
        if let Some(name) = plan.suggested_names.first() {
            return format!("floo deploy {quoted} --name {name}");
        }
    }
    plan.next
        .first()
        .and_then(|step| {
            step.open
                .as_deref()
                .map(|url| format!("Open {url}, then floo deploy {quoted}"))
                .or(step.cli.clone())
        })
        .unwrap_or_else(|| format!("floo deploy {quoted}"))
}

fn stop_with_plan(plan: &DeployPlan, quoted: &str) -> ! {
    let code = plan.code.as_deref().unwrap_or("PLAN_INCOMPLETE");
    let next = suggestion(plan, quoted);
    let details = output::to_value(plan);
    output::error_with_details(
        &plan.message,
        &ErrorCode::from_api(code),
        Some(&next),
        Some(&details),
    );
    if !output::is_json_mode() {
        for step in plan.next.iter().skip(1) {
            if let Some(cli) = step.cli.as_deref().or(step.open.as_deref()) {
                eprintln!("  or: {cli}");
            }
        }
    }
    process::exit(1);
}

fn emit_plan(plan: &DeployPlan) {
    if output::is_json_mode() {
        output::print_json(&json!({
            "event": "plan",
            "ok": plan.ok,
            "app": plan.app,
            "org": plan.org,
            "decision": plan.decision,
            "message": plan.message,
            "source": plan.source,
            "warnings": plan.warnings,
        }));
        return;
    }
    eprintln!("{}", plan.message);
    for warning in &plan.warnings {
        output::warn(warning);
    }
}

/// `gh repo create` in the user's own account: floo never creates repos there itself.
fn create_github_repo(path: &Path, app: &str, quoted: &str) {
    let manual = format!(
        "gh auth login, then floo deploy {quoted} --github. Or create an empty private repo at https://github.com/new, then git -C {quoted} remote add origin <url> && git -C {quoted} push -u origin HEAD && floo deploy {quoted}"
    );
    let signed_in = Command::new("gh")
        .args(["auth", "status"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !signed_in {
        stop(
            "Creating the GitHub repository needs the GitHub CLI (gh), signed in.",
            "GITHUB_CLI_REQUIRED",
            &manual,
        );
    }
    if !output::is_json_mode() {
        eprintln!("Creating private GitHub repository {app} with gh...");
    }
    let created = Command::new("gh")
        .args([
            "repo",
            "create",
            app,
            "--private",
            "--remote",
            "origin",
            "--push",
            "--source",
        ])
        .arg(path)
        .stdout(Stdio::from(io::stderr()))
        .status()
        .is_ok_and(|status| status.success());
    if !created {
        stop(
            "gh could not create the repository. Check its error above.",
            "GITHUB_REPO_CREATE_FAILED",
            &manual,
        );
    }
}

/// Push HEAD to the app's source and wait: the default branch deploys dev, any
/// other branch gets a preview and leaves dev alone.
fn ship(path: &Path, folder: &Folder, app: &str, source: &AppSource, quoted: &str) {
    let repo = source.repo.clone().unwrap_or_default();
    let default_branch = source.branch.clone().unwrap_or_else(|| "main".to_string());
    let branch = folder
        .branch
        .clone()
        .unwrap_or_else(|| default_branch.clone());
    let preview = branch != default_branch;
    if source.kind == "floo" {
        let clone_url = format!("https://github.com/{repo}.git");
        ensure_hosted_remote(path, folder, &clone_url, quoted);
        push_hosted(path, &clone_url, &branch, quoted);
    } else {
        let Some(remote) = folder.remote_for(&repo) else {
            if let Some(remote) = folder
                .remotes
                .iter()
                .find(|remote| Remote::points_at(&remote.url, &repo))
            {
                stop(
                    &format!(
                        "git would push {} to {}, not {repo}, which {app} deploys from.",
                        remote.name,
                        remote.push_urls.join(", ")
                    ),
                    "REPO_MISMATCH",
                    &format!(
                        "git -C {quoted} config --unset-all remote.{}.pushurl && floo deploy {quoted}",
                        remote.name
                    ),
                )
            }
            stop(
                &format!("No git remote in this folder points at {repo}, which {app} deploys from."),
                "REPO_MISMATCH",
                &format!("git -C {quoted} remote add origin https://github.com/{repo}.git && floo deploy {quoted}"),
            )
        };
        push_github(path, &remote.name, &branch, &folder.head, quoted);
    }
    if preview {
        super::previews::up(Some(app), &branch, true, Some(&folder.head), None);
    } else {
        super::deploys::watch(Some(app), Some(&folder.head));
    }
}

fn ensure_hosted_remote(path: &Path, folder: &Folder, clone_url: &str, quoted: &str) {
    if folder
        .remotes
        .iter()
        .any(|remote| same_repo(&remote.url, clone_url))
    {
        return;
    }
    let name = if folder.remotes.iter().any(|remote| remote.name == "origin") {
        HOSTED_REMOTE
    } else {
        "origin"
    };
    if git_read(path, &["remote", "add", name, clone_url]).is_none() {
        stop(
            &format!("Failed to add the git remote '{name}' for the hosted repository."),
            "GIT_FAILED",
            &format!("git -C {quoted} remote add {name} {clone_url} && floo deploy {quoted}"),
        );
    }
}

fn same_repo(a: &str, b: &str) -> bool {
    a.trim_end_matches(".git")
        .eq_ignore_ascii_case(b.trim_end_matches(".git"))
}

/// Push to the customer's own repo with their git credentials, never with force.
fn push_github(path: &Path, remote: &str, branch: &str, head: &str, quoted: &str) {
    let on_github = git_read(
        path,
        &[
            "ls-remote",
            "--heads",
            remote,
            &format!("refs/heads/{branch}"),
        ],
    )
    .and_then(|line| line.split_whitespace().next().map(str::to_string));
    if on_github.as_deref() == Some(head) {
        if !output::is_json_mode() {
            eprintln!(
                "{} is already on {remote}/{branch}; not pushing.",
                super::short_sha(head)
            );
        }
        return;
    }
    let refspec = format!("HEAD:refs/heads/{branch}");
    let pushed = git(path)
        .args(["push", remote, refspec.as_str()])
        .stdout(Stdio::from(io::stderr()))
        .stderr(Stdio::piped())
        .output();
    let output = match pushed {
        Ok(output) => output,
        Err(error) => stop(
            &format!("Failed to run git push: {error}"),
            "GIT_FAILED",
            &format!("floo deploy {quoted}"),
        ),
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprint!("{stderr}");
    if output.status.success() {
        return;
    }
    let lower = stderr.to_ascii_lowercase();
    let (message, next) = if lower.contains("protected branch") || lower.contains("gh006") {
        (
            format!("{branch} on {remote} needs a pull request; merging it deploys dev."),
            format!("git -C {quoted} switch -c <branch> && floo deploy {quoted} (a preview URL now, dev on merge)"),
        )
    } else if lower.contains("non-fast-forward") || lower.contains("fetch first") {
        (
            format!("{remote}/{branch} has commits this folder doesn't."),
            format!("git -C {quoted} pull --no-rebase {remote} {branch} && floo deploy {quoted}"),
        )
    } else if lower.contains("permission") || lower.contains("403") || lower.contains("denied") {
        (
            format!("GitHub refused the push to {remote}: the account git used can't write to it."),
            "gh auth status (or your git credentials) to check the account, then floo deploy"
                .to_string(),
        )
    } else {
        (
            format!(
                "git push to {remote} failed ({}). Check the git error above.",
                output.status
            ),
            format!("floo deploy {quoted}"),
        )
    };
    stop(&message, "GIT_PUSH_FAILED", &next);
}

fn push_hosted(path: &Path, clone_url: &str, branch: &str, quoted: &str) {
    // Only credential-free managed HTTPS URLs reach git, as in `projects clone`.
    if clone_url
        .strip_prefix("https://github.com/")
        .and_then(managed_app_id)
        .is_none()
    {
        stop(
            "The app's hosted repository is not a floo-hosted repository.",
            "INVALID_RESPONSE",
            "Contact floo support with the app name.",
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
    // Reset inherited helpers so no keychain stores the short-lived token, and push
    // to the validated URL, not a remote name that could carry another pushurl.
    let status = git(path)
        .args(["-c", "credential.helper=", "-c", &helper])
        .args(["-c", "credential.useHttpPath=true", "push"])
        .args([clone_url, refspec.as_str()])
        // Keep stdout for floo's JSON.
        .stdout(Stdio::from(io::stderr()))
        .status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => stop(
            &format!("git push failed ({status}). Check the git error above."),
            "GIT_PUSH_FAILED",
            &format!(
                "git -C {quoted} pull --no-rebase {clone_url} {branch} && floo deploy {quoted}"
            ),
        ),
        Err(error) => stop(
            &format!("Failed to run git push: {error}"),
            "GIT_FAILED",
            &format!("floo deploy {quoted}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_never_leave_the_machine() {
        assert_eq!(
            strip_credentials("https://x:ghp_secret@github.com/acme/shop.git"),
            "https://github.com/acme/shop.git"
        );
        assert_eq!(
            strip_credentials("ssh://git@github.com/acme/shop"),
            "ssh://github.com/acme/shop"
        );
        assert_eq!(
            strip_credentials("git@github.com:acme/shop.git"),
            "git@github.com:acme/shop.git"
        );
    }

    #[test]
    fn github_remotes_parse_in_every_format() {
        for url in [
            "git@github.com:acme/shop.git",
            "git@GitHub.com:acme/shop",
            "ssh://git@github.com/acme/shop.git",
            "ssh://git@github.com:22/acme/shop",
            "https://github.com/acme/shop",
            "https://token@github.com/acme/shop.git/",
        ] {
            assert_eq!(github_repo(url).as_deref(), Some("acme/shop"), "{url}");
        }
        for url in [
            "https://gitlab.com/acme/shop.git",
            "git@bitbucket.org:acme/shop.git",
            "https://github.com/acme",
            "https://github.com/acme/shop/extra",
            "/srv/git/shop.git",
        ] {
            assert_eq!(github_repo(url), None, "{url}");
        }
    }

    #[test]
    fn status_paths_keep_their_first_character() {
        // `git status` leads with a space for an unstaged change; trimming it once
        // turned `.env` into `env` and hid the secret.
        let status = " M .env\0?? api/.env.local\0R  new.key\0old.txt\0";
        assert_eq!(changed_files(status), [".env", "api/.env.local", "new.key"]);
    }

    #[test]
    fn a_push_url_elsewhere_is_not_the_planned_repo() {
        let remote = |push: &str| Remote {
            name: "origin".to_string(),
            url: "https://github.com/acme/shop.git".to_string(),
            push_urls: vec![push.to_string()],
        };
        assert!(remote("git@github.com:acme/shop.git").reaches("acme/shop"));
        assert!(!remote("https://github.com/other/fork.git").reaches("acme/shop"));
    }

    #[test]
    fn secret_looking_files_are_named() {
        for path in [
            ".env",
            "api/.env.local",
            "certs/server.pem",
            "tls.key",
            "id_ed25519",
            "a/b.P12",
        ] {
            assert!(looks_secret(path), "{path}");
        }
        for path in [
            ".env.example",
            ".env.sample",
            "keyboard.ts",
            "id_ed25519.pub",
            "src/env.ts",
        ] {
            assert!(!looks_secret(path), "{path}");
        }
    }
}
