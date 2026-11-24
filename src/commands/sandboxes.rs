//! `floo sandboxes`: find an org's running sandboxes, read what one printed, tag
//! it, and end it. Sandboxes are made by programs with the `floo-sandbox` SDK;
//! the CLI never creates one.

use std::process;

use serde_json::Value;

use crate::api_client::FlooClient;
use crate::confirm::{confirm_tier3, ConfirmOutcome, RiskMetadata, Tier};
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;

/// List the org's sandboxes that have not finished, newest first: of one app
/// with `--app`, and with every `--tag name=value` given.
pub fn list(app: Option<&str>, tags: &[String]) {
    for tag in tags {
        tag_pair(tag);
    }
    let result = call(|client| {
        let app_id = app.map(|name| super::resolve_app_or_exit(client, name).id);
        client.list_sandboxes(app_id.as_deref(), tags)
    });

    if output::is_json_mode() {
        output::success("Sandboxes.", Some(result));
        return;
    }
    let sandboxes = result
        .get("sandboxes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if sandboxes.is_empty() {
        output::info(
            "No running sandboxes. Programs start them with the floo-sandbox SDK.",
            None,
        );
        return;
    }
    let rows: Vec<Vec<String>> = sandboxes
        .iter()
        .map(|s| {
            vec![
                field(s, "id"),
                field(s, "app_name"),
                field(s, "name"),
                field(s, "state"),
                field(s, "size"),
                tags_text(s),
                field(s, "created_at"),
            ]
        })
        .collect();
    output::table(
        &["ID", "App", "Name", "State", "Size", "Tags", "Created"],
        &rows,
        None,
    );
    output::info(
        &format!(
            "{} of {} vCPUs in use.",
            number(&result, "vcpus_in_use"),
            number(&result, "vcpus_limit")
        ),
        None,
    );
}

/// Print what a sandbox's own command printed last, up to 64 KiB: stdout to
/// stdout and stderr to stderr, as the command wrote them.
pub fn logs(sandbox_id: &str) {
    let result = call(|client| client.sandbox_logs(sandbox_id));
    if output::is_json_mode() {
        output::success("Sandbox logs.", Some(result));
        return;
    }
    let chunks = result
        .get("output")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for chunk in &chunks {
        let text = chunk.get("text").and_then(Value::as_str).unwrap_or("");
        let to_stderr = chunk.get("stream").and_then(Value::as_str) == Some("stderr");
        output::printed(text, to_stderr);
    }
    if chunks.is_empty() {
        output::info("The sandbox's command has printed nothing yet.", None);
    }
}

/// Replace a sandbox's tags with `pairs`, or remove them all with `clear`.
pub fn tag(sandbox_id: &str, pairs: &[String], clear: bool) {
    let tags: serde_json::Map<String, Value> = if clear {
        serde_json::Map::new()
    } else {
        pairs
            .iter()
            .map(|pair| {
                let (name, value) = tag_pair(pair);
                (name.to_string(), Value::String(value.to_string()))
            })
            .collect()
    };
    let result = call(|client| client.set_sandbox_tags(sandbox_id, &Value::Object(tags)));
    output::success(&format!("Tagged {sandbox_id}."), Some(result));
}

/// End a sandbox now. Its commands stop and its files are deleted, so this is a
/// tier 3 command: type the sandbox's id, or pass the data-loss flag.
pub fn terminate(sandbox_id: &str, confirmed: bool) {
    let risk: RiskMetadata = Tier::Three.into();
    if output::is_dry_run_mode() {
        let preview = format!(
            "Would terminate sandbox '{sandbox_id}': its commands stop and its files are deleted. This cannot be undone."
        );
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "terminate",
                "sandbox_id": sandbox_id,
                "warning": preview,
                "destructive": risk.destructive,
                "data_loss": risk.data_loss,
                "tier": risk.tier,
            }),
        );
        return;
    }
    let preamble = vec![
        format!("\u{26a0} You are about to terminate sandbox '{sandbox_id}'."),
        "    Its commands stop and its files are deleted.".to_string(),
    ];
    match confirm_tier3(sandbox_id, &preamble, confirmed) {
        ConfirmOutcome::Proceed => {}
        ConfirmOutcome::Aborted => {
            if !output::is_json_mode() {
                output::info("Cancelled. The sandbox runs on.", None);
            }
            process::exit(0);
        }
        ConfirmOutcome::Refused { suggestion } => crate::confirm::exit_refused(
            &format!("Refusing to terminate sandbox '{sandbox_id}' without explicit confirmation."),
            &suggestion,
        ),
    }
    call(|client| client.terminate_sandbox(sandbox_id).map(|()| Value::Null));
    output::success(
        &format!("Terminated sandbox '{sandbox_id}'."),
        Some(serde_json::json!({
            "id": sandbox_id,
            "destructive": risk.destructive,
            "data_loss": risk.data_loss,
            "tier": risk.tier,
        })),
    );
}

/// Run one call, exiting with the API's error on failure.
fn call(request: impl FnOnce(&FlooClient) -> Result<Value, FlooApiError>) -> Value {
    super::require_auth();
    let client = super::init_client(None);
    match request(&client) {
        Ok(value) => value,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    }
}

/// Split `name=value`, or exit: a tag always has both.
fn tag_pair(pair: &str) -> (&str, &str) {
    match pair.split_once('=') {
        Some((name, value)) if !name.is_empty() => (name, value),
        _ => {
            output::error(
                &format!("A tag is written name=value, not '{pair}'."),
                &ErrorCode::InvalidFormat,
                Some("For example: --tag task=build"),
            );
            process::exit(1);
        }
    }
}

fn field(sandbox: &Value, key: &str) -> String {
    sandbox
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("-")
        .to_string()
}

fn number(result: &Value, key: &str) -> String {
    result
        .get(key)
        .and_then(Value::as_u64)
        .map_or_else(|| "-".to_string(), |n| n.to_string())
}

/// A sandbox's tags as `name=value` pairs, in name order.
fn tags_text(sandbox: &Value) -> String {
    let Some(tags) = sandbox.get("tags").and_then(Value::as_object) else {
        return "-".to_string();
    };
    if tags.is_empty() {
        return "-".to_string();
    }
    let mut pairs: Vec<String> = tags
        .iter()
        .map(|(name, value)| format!("{name}={}", value.as_str().unwrap_or("")))
        .collect();
    pairs.sort();
    pairs.join(",")
}
