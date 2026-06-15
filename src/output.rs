use std::io::{self, IsTerminal, Write};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};

use colored::Colorize;
use comfy_table::{Cell, Table};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use serde_json::Value;

use crate::errors::ErrorCode;
use crate::redact;

static JSON_MODE: AtomicBool = AtomicBool::new(false);
static DRY_RUN: AtomicBool = AtomicBool::new(false);

/// Serializes the handful of unit tests that mutate the process-global
/// JSON/dry-run mode (here and in `update.rs`). They share one `AtomicBool`
/// each, so running in parallel they intermittently clobber one another's
/// assertions — a test asserting `!is_json_mode()` can observe a `true` set by
/// a concurrent test. Each such test takes this lock for its duration.
#[cfg(test)]
pub(crate) static GLOBAL_MODE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn set_json_mode(enabled: bool) {
    JSON_MODE.store(enabled, Ordering::SeqCst);
}

pub fn is_json_mode() -> bool {
    JSON_MODE.load(Ordering::SeqCst)
}

pub fn set_dry_run_mode(enabled: bool) {
    DRY_RUN.store(enabled, Ordering::SeqCst);
}

pub fn is_dry_run_mode() -> bool {
    DRY_RUN.load(Ordering::SeqCst)
}

/// Emit a dry-run preview.
///
/// Human mode (stderr):
///
/// ```text
/// → Dry run — no changes will be made.
///   <line 1 of human_preview>
///   <line 2 of human_preview>
/// ```
///
/// JSON mode (stdout): `{"success": true, "data": data}` — same payload as
/// `success(..., Some(data))`. Agents reading `--json` see the structured
/// preview; humans see the same preview as a multi-line summary on stderr.
///
/// `human_preview` is multi-line text — newlines split into bullet lines.
/// Empty lines are dropped. Pass `""` only if the data payload is genuinely
/// self-explanatory; in practice every mutator should describe the action.
pub fn dry_run_preview(human_preview: &str, data: Value) {
    if is_json_mode() {
        print_json(&serde_json::json!({"success": true, "data": data}));
        return;
    }
    eprintln!(
        "{} {}",
        "\u{2192}".cyan(),
        "Dry run — no changes will be made.".bold()
    );
    for line in human_preview.lines() {
        if !line.trim().is_empty() {
            eprintln!("  {line}");
        }
    }
}

/// Returns true when stdin is a TTY and JSON mode is off — i.e. a human is
/// at the keyboard and can answer prompts.
pub fn is_interactive() -> bool {
    !is_json_mode() && io::stdin().is_terminal()
}

/// Single chokepoint for everything the CLI writes to stdout in JSON
/// mode.
///
/// Every `--json` payload runs through `redact::process_in_place` here.
/// If the payload contains any credential-shaped value the top-level
/// object also gets a `contains_secrets: true` marker — fired
/// regardless of whether the value was redacted or revealed, so agent
/// harnesses can refuse the payload before it lands in a transcript.
/// This is the secret-leakage doctrine for the CLI: callers do not need
/// to remember to redact, the boundary does it.
pub fn print_json(data: &Value) {
    let mut owned = data.clone();
    let contains_secrets = redact::process_in_place(&mut owned);
    if contains_secrets {
        if let Value::Object(map) = &mut owned {
            map.entry(redact::CONTAINS_SECRETS_KEY.to_string())
                .or_insert(Value::Bool(true));
        }
    }
    println!("{}", serde_json::to_string(&owned).unwrap_or_default());
}

pub fn success(message: &str, data: Option<Value>) {
    if is_json_mode() {
        print_json(&serde_json::json!({"success": true, "data": data}));
    } else {
        eprintln!("{} {message}", "\u{2713}".green());
    }
}

fn build_error_json(code: &ErrorCode, message: &str, suggestion: Option<&str>) -> Value {
    let mut err = serde_json::json!({"code": code.as_str(), "message": message});
    if let Some(sug) = suggestion {
        err.as_object_mut()
            .unwrap()
            .insert("suggestion".to_string(), Value::String(sug.to_string()));
    }
    err
}

pub fn error(message: &str, code: &ErrorCode, suggestion: Option<&str>) {
    if is_json_mode() {
        let err = build_error_json(code, message, suggestion);
        print_json(&serde_json::json!({"success": false, "error": err}));
    } else {
        eprintln!("{} {message}", "Error:".red());
        if let Some(sug) = suggestion {
            eprintln!("  \u{2192} {sug}");
        }
    }
}

pub fn error_with_data(
    message: &str,
    code: &ErrorCode,
    suggestion: Option<&str>,
    data: Option<Value>,
) {
    if is_json_mode() {
        let err = build_error_json(code, message, suggestion);
        print_json(&serde_json::json!({"success": false, "error": err, "data": data}));
    } else {
        eprintln!("{} {message}", "Error:".red());
        if let Some(sug) = suggestion {
            eprintln!("  \u{2192} {sug}");
        }
    }
}

pub fn info(message: &str, data: Option<Value>) {
    if is_json_mode() {
        print_json(&serde_json::json!({"success": true, "data": data}));
    } else {
        eprintln!("{message}");
    }
}

pub fn table(headers: &[&str], rows: &[Vec<String>], data: Option<Value>) {
    if is_json_mode() {
        print_json(&serde_json::json!({"success": true, "data": data}));
    } else {
        let mut t = Table::new();
        t.set_header(headers.iter().map(Cell::new).collect::<Vec<_>>());
        for row in rows {
            t.add_row(row.iter().map(Cell::new).collect::<Vec<_>>());
        }
        eprintln!("{t}");
    }
}

pub struct Spinner {
    bar: Option<ProgressBar>,
}

impl Spinner {
    pub fn new(message: &str) -> Self {
        if is_json_mode() {
            Self { bar: None }
        } else {
            let bar = ProgressBar::new_spinner();
            bar.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .unwrap(),
            );
            bar.set_message(message.to_string());
            bar.enable_steady_tick(std::time::Duration::from_millis(80));
            Self { bar: Some(bar) }
        }
    }

    pub fn finish(&self) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }
}

pub fn confirm(message: &str) -> bool {
    if is_json_mode() {
        return true;
    }
    if !is_interactive() {
        return false;
    }
    eprint!("{message} [y/N] ");
    let _ = io::stderr().flush();
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => matches!(input.trim().to_lowercase().as_str(), "y" | "yes"),
        Err(_) => false,
    }
}

pub fn prompt_with_default(prompt: &str, default: &str) -> String {
    if !is_interactive() {
        return default.to_string();
    }
    eprint!("  ? {prompt}: ({default}) ");
    let _ = io::stderr().flush();
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                default.to_string()
            } else {
                trimmed.to_string()
            }
        }
        Err(e) => {
            eprintln!("Failed to read input: {e}");
            process::exit(1);
        }
    }
}

/// Print a raw value to stdout for piping. Used by `env get` in human mode.
/// In JSON mode, callers should use `success()` instead.
/// Emit a bare scalar value to stdout in human mode. Used by commands like
/// `floo env get <KEY>` (which wants the value piped into shell scripts) and
/// `floo version` (which needs the tag on stdout for install.sh + Unix
/// convention) to produce machine-readable output without breaking the
/// dual-mode contract.
///
/// In JSON mode this is a no-op — the structured `output::success(...)`
/// call in the same command already puts the value into the JSON payload
/// on stdout, so emitting a bare line would corrupt the JSON response that
/// agents pipe through `jq`. The debug_assert catches misuse during dev;
/// the early return makes release builds safe even if a future caller
/// forgets the human-mode guard.
pub fn raw_value(value: &str) {
    debug_assert!(!is_json_mode(), "raw_value called in JSON mode");
    if is_json_mode() {
        return;
    }
    println!("{value}");
}

pub fn warn(message: &str) {
    if !is_json_mode() {
        eprintln!("  {} {}", "\u{26a0}".yellow(), message);
    }
}

pub fn dim_line(line: &str) {
    if !is_json_mode() {
        eprintln!("  {}", line.dimmed());
    }
}

pub fn bold_line(line: &str) {
    if !is_json_mode() {
        eprintln!("  {}", line.bold());
    }
}

/// Helper to serialize any Serialize type to a Value.
pub fn to_value<T: Serialize>(val: &T) -> Value {
    serde_json::to_value(val).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_mode_toggle() {
        let _guard = GLOBAL_MODE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_json_mode(false);
        assert!(!is_json_mode());
        set_json_mode(true);
        assert!(is_json_mode());
        set_json_mode(false);
    }

    #[test]
    fn test_is_interactive_false_in_json_mode() {
        let _guard = GLOBAL_MODE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_json_mode(true);
        assert!(!is_interactive());
        set_json_mode(false);
    }

    #[test]
    fn test_dry_run_mode_toggle() {
        let _guard = GLOBAL_MODE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_dry_run_mode(false);
        assert!(!is_dry_run_mode());
        set_dry_run_mode(true);
        assert!(is_dry_run_mode());
        set_dry_run_mode(false);
    }
}
