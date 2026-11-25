//! `floo sandboxes`: create a sandbox, run commands in it, copy files in and
//! out, find the org's running sandboxes, read what one printed, tag it, and end
//! it. Programs can do the same with the `floo-sandbox` SDK.

use std::io::{Read, Write};
use std::process;

use base64::Engine;

use serde_json::Value;

use crate::api_client::FlooClient;
use crate::confirm::{confirm_tier3, ConfirmOutcome, RiskMetadata, Tier};
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;

/// What `floo sandboxes create` asks for.
pub struct CreateArgs<'a> {
    pub app: &'a str,
    pub image_file: Option<&'a str>,
    pub size: &'a str,
    pub timeout: u64,
    pub env: &'a [String],
    pub name: Option<&'a str>,
    pub tags: &'a [String],
}

/// Create a sandbox and print its ID: on stdout alone, so scripts can take it.
pub fn create(args: &CreateArgs) {
    let image = args.image_file.map(|file| {
        std::fs::read_to_string(file).unwrap_or_else(|e| {
            output::error(
                &format!("Could not read the Dockerfile '{file}': {e}"),
                &ErrorCode::FileError,
                None,
            );
            process::exit(1);
        })
    });
    let body = serde_json::json!({
        "app": args.app,
        "image": image,
        "size": args.size,
        "timeout": args.timeout,
        "env": pairs(args.env, "--env", "PORT=8000"),
        "name": args.name,
        "tags": pairs(args.tags, "--tag", "task=build"),
    });
    if !output::is_json_mode() {
        output::info(
            "Creating the sandbox. A new image's first build takes a few minutes.",
            None,
        );
    }
    let created = call(|client| client.create_sandbox(&body));
    if output::is_json_mode() {
        output::success("Created the sandbox.", Some(created));
        return;
    }
    output::raw_value(&field(&created, "id"));
}

/// Run a command in a sandbox and wait for it. Its stdout and stderr are printed
/// where it wrote them, and the CLI exits with its exit code.
pub fn exec(
    sandbox_id: &str,
    command: &[String],
    workdir: Option<&str>,
    env: &[String],
    timeout: u64,
) {
    let body = serde_json::json!({
        "command": command,
        "workdir": workdir,
        "env": pairs(env, "--env", "DEBUG=1"),
        "timeout": timeout,
    });
    let result = call(|client| client.exec_in_sandbox(sandbox_id, &body, timeout));
    let code = result.get("exit_code").and_then(Value::as_i64).unwrap_or(1);
    if output::is_json_mode() {
        output::success("Ran the command.", Some(result));
    } else {
        output::printed(
            result.get("stdout").and_then(Value::as_str).unwrap_or(""),
            false,
        );
        output::printed(
            result.get("stderr").and_then(Value::as_str).unwrap_or(""),
            true,
        );
        if result.get("timed_out").and_then(Value::as_bool) == Some(true) {
            output::warn(&format!(
                "The command ran past its {timeout} s timeout and was ended."
            ));
        }
        if result.get("truncated").and_then(Value::as_bool) == Some(true) {
            output::warn("It printed more than 1 MiB to a stream: only the last MiB is shown.");
        }
    }
    let _ = std::io::stdout().flush();
    // A shell takes an exit code modulo 256: -1 (timed out) exits 255.
    process::exit(i32::try_from(code.rem_euclid(256)).unwrap_or(1));
}

/// List what a directory in a sandbox holds.
pub fn ls(sandbox_id: &str, path: &str) {
    let result = call(|client| client.list_sandbox_files(sandbox_id, path));
    if output::is_json_mode() {
        output::success("Files.", Some(result));
        return;
    }
    let files = result
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if files.is_empty() {
        output::info(&format!("{path} is empty."), None);
        return;
    }
    let rows: Vec<Vec<String>> = files
        .iter()
        .map(|f| {
            let name = field(f, "name");
            let kind = field(f, "type");
            let shown = if kind == "dir" {
                format!("{name}/")
            } else {
                name
            };
            vec![shown, kind, number(f, "size")]
        })
        .collect();
    output::table(&["Name", "Type", "Bytes"], &rows, None);
}

/// Copy one file into or out of a sandbox. The sandbox side is written
/// `sb-ID:/path`; the other is a local path, or `-` for stdin or stdout.
pub fn cp(source: &str, destination: &str) {
    match (remote(source), remote(destination)) {
        (Some((sandbox_id, path)), None) => download(sandbox_id, path, destination),
        (None, Some((sandbox_id, path))) => upload(source, sandbox_id, path),
        _ => {
            output::error(
                "One side of `floo sandboxes cp` is a sandbox path and the other a local one.",
                &ErrorCode::InvalidFormat,
                Some("For example: floo sandboxes cp ./app.py sb-abc123:/work/app.py"),
            );
            process::exit(1);
        }
    }
}

/// `sb-ID:/path` as the sandbox's ID and the path in it.
fn remote(spec: &str) -> Option<(&str, &str)> {
    let (id, path) = spec.split_once(':')?;
    (id.starts_with("sb-") && path.starts_with('/')).then_some((id, path))
}

fn download(sandbox_id: &str, path: &str, destination: &str) {
    let result = call(|client| client.read_sandbox_file(sandbox_id, path));
    let content = result.get("content").and_then(Value::as_str).unwrap_or("");
    let bytes = if result.get("encoding").and_then(Value::as_str) == Some("base64") {
        base64::engine::general_purpose::STANDARD
            .decode(content)
            .unwrap_or_default()
    } else {
        content.as_bytes().to_vec()
    };
    let written = if destination == "-" {
        std::io::stdout().lock().write_all(&bytes)
    } else {
        std::fs::write(destination, &bytes)
    };
    if let Err(e) = written {
        output::error(
            &format!("Could not write '{destination}': {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    }
    if destination != "-" {
        output::success(
            &format!("Copied {sandbox_id}:{path} to {destination}."),
            Some(serde_json::json!({ "bytes": bytes.len() })),
        );
    }
}

fn upload(source: &str, sandbox_id: &str, path: &str) {
    let mut bytes = Vec::new();
    let read = if source == "-" {
        std::io::stdin().lock().read_to_end(&mut bytes).map(|_| ())
    } else {
        std::fs::read(source).map(|b| bytes = b)
    };
    if let Err(e) = read {
        output::error(
            &format!("Could not read '{source}': {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    }
    let body = match String::from_utf8(bytes) {
        Ok(text) => serde_json::json!({ "content": text, "encoding": "utf-8" }),
        Err(e) => serde_json::json!({
            "content": base64::engine::general_purpose::STANDARD.encode(e.into_bytes()),
            "encoding": "base64",
        }),
    };
    call(|client| {
        client
            .write_sandbox_file(sandbox_id, path, &body)
            .map(|()| Value::Null)
    });
    output::success(&format!("Copied {source} to {sandbox_id}:{path}."), None);
}

/// `NAME=VALUE` pairs as a JSON object, or exit naming the flag.
fn pairs(given: &[String], flag: &str, example: &str) -> Value {
    let mut map = serde_json::Map::new();
    for pair in given {
        match pair.split_once('=') {
            Some((name, value)) if !name.is_empty() => {
                map.insert(name.to_string(), Value::String(value.to_string()));
            }
            _ => {
                output::error(
                    &format!("{flag} is written NAME=VALUE, not '{pair}'."),
                    &ErrorCode::InvalidFormat,
                    Some(&format!("For example: {flag} {example}")),
                );
                process::exit(1);
            }
        }
    }
    Value::Object(map)
}

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
            "No running sandboxes. Start one with `floo sandboxes create --app NAME`.",
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
