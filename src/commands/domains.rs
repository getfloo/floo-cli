use std::process;
use std::thread;
use std::time::{Duration, Instant};

use crate::errors::ErrorCode;
use crate::output;

pub fn list(app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let result = match client.list_domains(&app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if result.domains.is_empty() {
        if !output::is_json_mode() {
            output::info(
                &format!(
                    "No custom domains on {app_name}. Declare [domains.\"<host>\"] in floo.app.toml and release to prod."
                ),
                None,
            );
        } else {
            output::success("No domains.", Some(serde_json::json!({"domains": []})));
        }
        return;
    }

    if output::is_json_mode() {
        output::success("Domains retrieved.", Some(output::to_value(&result)));
        return;
    }
    for domain in &result.domains {
        output::info(
            &format!(
                "{}: {}",
                domain.hostname,
                domain_status(domain.status.as_deref())
            ),
            None,
        );
        if let Some(dns) = domain.dns_instructions.as_deref() {
            output::info(dns, None);
        }
    }
}

pub fn status(hostname: &str, app: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, _app_name) = super::resolve_app_from_config(&client, app);

    let result = match client.list_domains(&app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let domain = match result.domains.iter().find(|d| d.hostname == hostname) {
        Some(d) => d,
        None => {
            output::error(
                &format!("Domain '{hostname}' not found."),
                &ErrorCode::DomainNotFound,
                Some("Run 'floo domains list' to see available domains."),
            );
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            &format!("Domain {hostname}"),
            Some(output::to_value(domain)),
        );
        return;
    }

    let status = domain_status(domain.status.as_deref());
    let ssl = domain.ssl_status.as_deref().unwrap_or("-");
    let verified = domain
        .verified
        .map(|v| if v { "yes" } else { "no" })
        .unwrap_or("-");
    let service = domain.service_name.as_deref().unwrap_or("app default");

    output::info(&format!("Domain:   {hostname}"), None);
    output::info(&format!("Status:   {status}"), None);
    output::info(&format!("SSL:      {ssl}"), None);
    output::info(&format!("Verified: {verified}"), None);
    output::info(&format!("Service:  {service}"), None);
    if let Some(dns) = domain.dns_instructions.as_deref() {
        output::info(dns, None);
    }
}

/// Poll until the domain is active, failed, or timeout expires.
/// Reads domain state; the platform owns DNS checks and activation.
pub fn watch(hostname: &str, app: Option<&str>, timeout_secs: u64) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, _app_name) = super::resolve_app_from_config(&client, app);

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_secs(5);

    if !output::is_json_mode() {
        output::info(
            &format!("Watching {hostname} (timeout: {timeout_secs}s, polling every 5s)..."),
            None,
        );
    }

    loop {
        let domains = match client.list_domains(&app_id) {
            Ok(r) => r,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };

        let result = match domains.domains.into_iter().find(|d| d.hostname == hostname) {
            Some(domain) => domain,
            None => {
                output::error(
                    &format!("Domain '{hostname}' not found."),
                    &ErrorCode::DomainNotFound,
                    Some("Run 'floo domains list' to see available domains."),
                );
                process::exit(1);
            }
        };
        let current_status = result.status.as_deref().unwrap_or("unknown");

        match current_status {
            "active" => {
                if output::is_json_mode() {
                    output::success(
                        &format!("Domain {hostname} is active."),
                        Some(output::to_value(&result)),
                    );
                } else {
                    output::success(
                        &format!("Domain {hostname} is now active."),
                        Some(serde_json::Value::Null),
                    );
                }
                return;
            }
            "removed" => {
                output::error(
                    &format!("Domain {hostname} is {}.", domain_status(Some("removed"))),
                    &ErrorCode::DomainNotFound,
                    Some("Re-declare the domain in floo.app.toml and release to prod to restore."),
                );
                process::exit(1);
            }
            "failed" => {
                output::error(
                    &format!("Domain {hostname} verification failed."),
                    &ErrorCode::DomainVerificationFailed,
                    Some("Inspect DNS records with 'floo domains show', then re-run 'floo domains watch'."),
                );
                process::exit(1);
            }
            _ => {
                if !output::is_json_mode() {
                    output::info(&format!("  Status: {current_status} — waiting..."), None);
                }
            }
        }

        if Instant::now() >= deadline {
            output::error(
                &format!("Timed out waiting for {hostname} to become active."),
                &ErrorCode::DomainWatchTimeout,
                Some("DNS changes can take up to 24 hours. Re-run 'floo domains watch' to resume."),
            );
            process::exit(1);
        }

        thread::sleep(poll_interval);
    }
}

/// Human labels for API domain states; JSON retains the original status.
fn domain_status(status: Option<&str>) -> &str {
    match status {
        Some("pending") => "waiting on DNS",
        Some("removed") => {
            "retired (certificate kept 7 days; re-declare in floo.app.toml to restore)"
        }
        Some(status) => status,
        None => "-",
    }
}
