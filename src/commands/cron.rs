use std::process;

use crate::errors::ErrorCode;
use crate::output;

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

pub fn run(app_flag: Option<&str>, name: &str) {
    // Dry-run is a pure echo: every other --dry-run handler in the CLI
    // (apps.rs, domains.rs, env.rs, rollbacks.rs, deploy.rs) checks
    // is_dry_run_mode() BEFORE require_auth() / resolve_app_from_config(),
    // so previewing a mutating command never requires a live API key or
    // an existing app row. Keep that contract here so logged-out users
    // and offline agents get a consistent preview shape across commands.
    if output::is_dry_run_mode() {
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
            let suggestion = if e.code == "NOT_FOUND" {
                Some("Run `floo cron list` to see available cron jobs.")
            } else {
                None
            };
            output::error(&e.message, &ErrorCode::from_api(&e.code), suggestion);
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
