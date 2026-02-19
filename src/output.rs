use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use colored::Colorize;
use comfy_table::{Cell, Table};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use serde_json::Value;

static JSON_MODE: AtomicBool = AtomicBool::new(false);

pub fn set_json_mode(enabled: bool) {
    JSON_MODE.store(enabled, Ordering::SeqCst);
}

pub fn is_json_mode() -> bool {
    JSON_MODE.load(Ordering::SeqCst)
}

fn print_json(data: &Value) {
    println!("{}", serde_json::to_string(data).unwrap_or_default());
}

pub fn success(message: &str, data: Option<Value>) {
    if is_json_mode() {
        print_json(&serde_json::json!({"success": true, "data": data}));
    } else {
        eprintln!("{} {message}", "\u{2713}".green());
    }
}

pub fn error(message: &str, code: &str, suggestion: Option<&str>) {
    if is_json_mode() {
        let mut err = serde_json::json!({"code": code, "message": message});
        if let Some(sug) = suggestion {
            err.as_object_mut()
                .unwrap()
                .insert("suggestion".to_string(), Value::String(sug.to_string()));
        }
        print_json(&serde_json::json!({"success": false, "error": err}));
    } else {
        eprintln!("{} {message}", "Error:".red());
        if let Some(sug) = suggestion {
            eprintln!("  \u{2192} {sug}");
        }
    }
}

pub fn error_with_data(message: &str, code: &str, suggestion: Option<&str>, data: Option<Value>) {
    if is_json_mode() {
        let mut err = serde_json::json!({"code": code, "message": message});
        if let Some(sug) = suggestion {
            err.as_object_mut()
                .unwrap()
                .insert("suggestion".to_string(), Value::String(sug.to_string()));
        }
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
    eprint!("{message} [y/N] ");
    let _ = io::stderr().flush();
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => matches!(input.trim().to_lowercase().as_str(), "y" | "yes"),
        Err(_) => false,
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
#[allow(dead_code)]
pub fn to_value<T: Serialize>(val: &T) -> Value {
    serde_json::to_value(val).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_mode_toggle() {
        set_json_mode(false);
        assert!(!is_json_mode());
        set_json_mode(true);
        assert!(is_json_mode());
        set_json_mode(false);
    }
}
