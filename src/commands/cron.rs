use std::path::Path;
use std::process;

use crate::errors::{ErrorCode, FlooApiError, FlooError};
use crate::output;

/// Help text to attach when a cron API call comes back not-found.
///
/// Keyed on the HTTP 404 *status* (`FlooApiError::is_not_found`), not a
/// hard-coded `code` string. The API emits `CRON_JOB_NOT_FOUND`
/// (getfloo/floo `api/app/routes/cron.py`), but this gate used to read
/// `e.code == "NOT_FOUND"`, so it never matched and the hint silently never
/// fired (issue #152). Matching on status keeps CLI↔API code-string drift
/// from disabling the hint again.
fn not_found_suggestion(e: &FlooApiError) -> Option<&'static str> {
    if e.is_not_found() {
        Some("Run `floo cron list` to see available cron jobs.")
    } else {
        None
    }
}

/// Validate a cron job name against the locally-declared cron set for
/// `floo cron run --dry-run` (the config-resolved, no-`--app` path only).
///
/// A `--dry-run` preview should reflect what the real run would do instead of
/// echoing a confident "Would trigger" for a name that doesn't exist (issue
/// #152). When the app is resolved from local config, the real run is driven
/// by that same `floo.app.toml` — its `[cron.<name>]` blocks are what a deploy
/// pushes — so the declared set is the best offline proxy for the cron names
/// the run will accept. It is only a proxy: a job declared but not yet deployed
/// (or deleted server-side) can still skew local vs. live, but that's the most
/// a no-network, no-auth preview can check.
///
/// The `--app` path is deliberately NOT routed here: with `--app`, the real run
/// resolves the app via the API and never reads `floo.app.toml`, so the local
/// cron set may belong to a different app entirely — validating against it
/// would reject names the real run accepts. The caller skips this for `--app`.
fn validate_dry_run_target(cwd: &Path, name: &str) -> Result<(), FlooError> {
    // Resolve config the same way the no-flag real run does (walk up from cwd).
    // No network, no auth — this is a local file read.
    let resolved = crate::project_config::resolve_app_context(cwd, None)?;
    let cron = resolved.app_config.map(|c| c.cron).unwrap_or_default();
    if cron.contains_key(name) {
        return Ok(());
    }

    let mut declared: Vec<&str> = cron.keys().map(String::as_str).collect();
    declared.sort_unstable();
    let suggestion = if declared.is_empty() {
        format!(
            "No cron jobs are declared in {}. Add a [cron.{name}] block, or check the name.",
            crate::project_config::APP_CONFIG_FILE
        )
    } else {
        format!("Declared cron jobs: {}.", declared.join(", "))
    };
    Err(FlooError::with_suggestion(
        // Mirror the API's emitted contract so the preview's error shape
        // matches the real run's 404 (api/app/routes/cron.py).
        ErrorCode::Other("CRON_JOB_NOT_FOUND".to_string()),
        format!(
            "Cron job '{name}' is not declared in {}.",
            crate::project_config::APP_CONFIG_FILE
        ),
        suggestion,
    ))
}

pub fn list(app_flag: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _app_name) = super::resolve_app_from_config(&client, app_flag);

    let result = match client.list_cron_jobs(&app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success("Cron jobs.", Some(output::to_value(&result)));
        return;
    }

    if result.cron_jobs.is_empty() {
        output::info("No cron jobs configured.", None);
        return;
    }

    let rows: Vec<Vec<String>> = result
        .cron_jobs
        .iter()
        .map(|job| {
            let status = job.last_status.as_deref().unwrap_or("-").to_string();
            let last_run = job.last_run_at.as_deref().unwrap_or("never").to_string();
            let enabled = if job.enabled { "yes" } else { "no" }.to_string();
            vec![
                job.name.clone(),
                job.schedule.clone(),
                job.service_name.clone(),
                enabled,
                status,
                last_run,
            ]
        })
        .collect();

    output::table(
        &[
            "Name",
            "Schedule",
            "Service",
            "Enabled",
            "Last Status",
            "Last Run",
        ],
        &rows,
        None,
    );
}

pub fn show(app_flag: Option<&str>, name: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _app_name) = super::resolve_app_from_config(&client, app_flag);

    let result = match client.get_cron_job(&app_id, name) {
        Ok(r) => r,
        Err(e) => {
            output::error(
                &e.message,
                &ErrorCode::from_api(&e.code),
                not_found_suggestion(&e),
            );
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success("Cron job details.", Some(output::to_value(&result)));
        return;
    }

    let status = result.last_status.as_deref().unwrap_or("-");
    let last_run = result.last_run_at.as_deref().unwrap_or("never");
    let enabled = if result.enabled { "yes" } else { "no" };

    output::table(
        &["Field", "Value"],
        &[
            vec!["Name".to_string(), result.name.clone()],
            vec!["Schedule".to_string(), result.schedule.clone()],
            vec!["Command".to_string(), result.command.clone()],
            vec!["Service".to_string(), result.service_name.clone()],
            vec!["Enabled".to_string(), enabled.to_string()],
            vec!["Last Status".to_string(), status.to_string()],
            vec!["Last Run".to_string(), last_run.to_string()],
        ],
        None,
    );
}

pub fn run(app_flag: Option<&str>, name: &str) {
    // Dry-run stays offline: like every other --dry-run handler in the CLI
    // (apps.rs, domains.rs, env.rs, rollbacks.rs, deploy.rs) it runs BEFORE
    // require_auth() / the API call, so previewing never needs a live API key.
    // A preview should reflect what the real run would do — the real run 404s
    // on an unknown name and exits 1 — so on the config-resolved path we
    // validate the name against the locally-declared [cron.<name>] set instead
    // of echoing a confident "Would trigger" for a job that doesn't exist
    // (#152). We skip this when --app is given: that path resolves the app
    // server-side and never reads floo.app.toml, so the local cron set isn't
    // the right thing to check (it may belong to a different app) and there's
    // nothing we can validate offline.
    if output::is_dry_run_mode() {
        if app_flag.is_none() {
            let cwd = super::read_cwd_or_exit();
            if let Err(e) = validate_dry_run_target(&cwd, name) {
                output::error(&e.message, &e.code, e.suggestion.as_deref());
                process::exit(1);
            }
        }

        let target = app_flag.unwrap_or("(reads from config)");
        let preview = format!("Would trigger cron job '{name}' on {target}.");
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "run_cron_job",
                "app": app_flag,
                "name": name,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _app_name) = super::resolve_app_from_config(&client, app_flag);

    let spinner = output::Spinner::new(&format!("Triggering cron job '{name}'..."));
    let result = match client.run_cron_job(&app_id, name) {
        Ok(r) => {
            spinner.finish();
            r
        }
        Err(e) => {
            spinner.finish();
            output::error(
                &e.message,
                &ErrorCode::from_api(&e.code),
                not_found_suggestion(&e),
            );
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success("Cron job triggered.", Some(output::to_value(&result)));
        return;
    }

    let msg = result
        .message
        .as_deref()
        .unwrap_or("Cron job triggered successfully.");
    output::success(msg, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_app_toml(dir: &TempDir, body: &str) {
        fs::write(
            dir.path().join(crate::project_config::APP_CONFIG_FILE),
            format!("[app]\nname = \"my-app\"\n\n{body}"),
        )
        .unwrap();
    }

    #[test]
    fn not_found_suggestion_fires_on_404() {
        // Regression for #152: the API returns CRON_JOB_NOT_FOUND (not
        // NOT_FOUND), so a code-string gate never matched. Keyed on status,
        // the hint must fire regardless of the code string.
        let err = FlooApiError::new(404, "CRON_JOB_NOT_FOUND", "Cron job not found");
        let suggestion = not_found_suggestion(&err).expect("404 must surface the list hint");
        assert!(suggestion.contains("floo cron list"), "got: {suggestion}");
    }

    #[test]
    fn not_found_suggestion_silent_for_non_404() {
        let err = FlooApiError::new(500, "INTERNAL_ERROR", "boom");
        assert!(not_found_suggestion(&err).is_none());
    }

    #[test]
    fn dry_run_target_ok_for_declared_name() {
        let dir = TempDir::new().unwrap();
        write_app_toml(
            &dir,
            "[cron.daily-report]\nschedule = \"0 9 * * *\"\ncommand = \"./report.sh\"\nservice = \"web\"\n",
        );
        assert!(validate_dry_run_target(dir.path(), "daily-report").is_ok());
    }

    #[test]
    fn dry_run_target_errors_for_unknown_name() {
        // The core of bug #1: a preview of a name that isn't declared must
        // fail (the real run 404s), not echo "Would trigger".
        let dir = TempDir::new().unwrap();
        write_app_toml(
            &dir,
            "[cron.daily-report]\nschedule = \"0 9 * * *\"\ncommand = \"./report.sh\"\nservice = \"web\"\n",
        );
        let err = validate_dry_run_target(dir.path(), "does-not-exist").unwrap_err();
        // Dry-run error code mirrors the API's emitted contract so the preview
        // reflects the real run.
        assert_eq!(err.code, ErrorCode::Other("CRON_JOB_NOT_FOUND".to_string()));
        assert!(
            err.message.contains("does-not-exist"),
            "message should name the missing job: {}",
            err.message
        );
        // The suggestion should point at the declared job(s) for discovery.
        let suggestion = err.suggestion.expect("not-found error must carry a hint");
        assert!(
            suggestion.contains("daily-report"),
            "suggestion should list declared jobs: {suggestion}"
        );
    }

    #[test]
    fn dry_run_target_errors_when_no_cron_declared() {
        let dir = TempDir::new().unwrap();
        write_app_toml(&dir, "[services.web]\ntype = \"web\"\npath = \"./web\"\n");
        let err = validate_dry_run_target(dir.path(), "anything").unwrap_err();
        assert_eq!(err.code, ErrorCode::Other("CRON_JOB_NOT_FOUND".to_string()));
        let suggestion = err.suggestion.expect("not-found error must carry a hint");
        assert!(
            suggestion.contains(crate::project_config::APP_CONFIG_FILE),
            "suggestion should point at the config file when nothing is declared: {suggestion}"
        );
    }

    #[test]
    fn dry_run_target_walks_up_from_subdirectory() {
        // The no-flag real run finds config by walking up from cwd; the preview
        // must resolve the same way so it works from a nested project dir.
        let dir = TempDir::new().unwrap();
        write_app_toml(
            &dir,
            "[cron.nightly]\nschedule = \"0 0 * * *\"\ncommand = \"./n.sh\"\nservice = \"web\"\n",
        );
        let nested = dir.path().join("services/web");
        fs::create_dir_all(&nested).unwrap();
        assert!(validate_dry_run_target(&nested, "nightly").is_ok());
        assert!(validate_dry_run_target(&nested, "missing").is_err());
    }

    #[test]
    fn dry_run_target_errors_when_no_config_found() {
        // No floo.app.toml / floo.service.toml anywhere: resolve_app_context
        // surfaces NoConfigFound, same as the real no-flag run would.
        let dir = TempDir::new().unwrap();
        let err = validate_dry_run_target(dir.path(), "anything").unwrap_err();
        assert_eq!(err.code, ErrorCode::NoConfigFound);
    }
}
