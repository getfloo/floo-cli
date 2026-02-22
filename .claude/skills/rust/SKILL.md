---
name: rust
description: Detailed coding rules for Floo CLI Rust code.
user-invocable: false
---

# Rust / CLI Coding Skill

> Rules for writing Rust code in the Floo CLI (`floo-cli/src/`). These rules are the detailed expansion of `CLAUDE.md`. When this skill is active, follow every rule below.

## Scope

This skill applies to all Rust files in `floo-cli/`. The API is Python — see the `python` skill for API work.

---

## Architecture

```
main.rs                     <- Entry point, calls cli::run()
cli.rs                      <- clap App with derive macros, global --json and --version flags
commands/                   <- One file per command group
  auth.rs                   <- login, logout, whoami
  deploy.rs                 <- deploy command
  apps.rs                   <- list, status, delete
  env.rs                    <- set, list, remove
  domains.rs                <- add, list, remove
  logs.rs                   <- view runtime logs
output.rs                   <- Dual-mode output (critical)
api_client.rs               <- FlooClient wrapping reqwest
config.rs                   <- ~/.floo/config.json management
detection.rs                <- Runtime/framework auto-detection
archive.rs                  <- .tar.gz packing with .flooignore
names.rs                    <- Random app name generation
resolve.rs                  <- resolve_app() — lookup by UUID or name
errors.rs                   <- FlooError + FlooApiError
```

### Global flags

The clap App defines two global flags:
- `--json` — enables JSON output mode (sets `output::JSON_MODE` atomic bool)
- `--version` — prints version and exits

Every command must work correctly in both human and JSON output modes.

---

## Output Contract (`output.rs`) — CRITICAL

This is the most important module in the CLI. It enforces the dual-mode output pattern.

### The rule

- **Colored output** (spinners, tables, progress) → **stderr**
- **JSON output** → **stdout**

This makes `floo deploy --json 2>/dev/null | jq` work for agents.

### Global mode

`JSON_MODE` is an `AtomicBool`. Set once at startup via `set_json_mode()`. Checked by every output function.

### Functions

| Function | Human mode (stderr) | JSON mode (stdout) |
|----------|--------------------|--------------------|
| `success(msg, data)` | `"[check] {msg}"` in green | `{"success": true, "data": data}` |
| `error(msg, code, suggestion)` | `"Error: {msg}\n  -> {suggestion}"` | `{"success": false, "error": {...}}` |
| `info(msg)` | `"{msg}"` to stderr | `{"success": true, "data": null}` |
| `table(headers, rows)` | Formatted table to stderr | N/A (use success with data) |
| `print_json(value)` | N/A | Writes JSON to stdout |

### Spinner

`Spinner` struct with RAII `Drop` trait. Created via `Spinner::new("message")`. Writes to stderr. Auto-clears on drop. In JSON mode, spinners are no-ops.

### CRITICAL: `info()` in JSON mode emits `{"success": true, "data": null}`

If a command calls `info()` then later calls `success()` with real data, stdout has TWO JSON objects — breaking agent parsing.

**Fix:** Guard informational `info()` calls:
```rust
if !is_json_mode() {
    info("Processing...");
}
// ... later ...
success("Done", &data);
```

---

## Error Handling (`errors.rs`)

### `FlooError`

```rust
pub struct FlooError {
    pub code: String,
    pub message: String,
    pub suggestion: Option<String>,
}
```

Uses `thiserror` derive. Constructors use `impl Into<String>` for ergonomics:

```rust
impl FlooError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self { ... }
    pub fn with_suggestion(
        code: impl Into<String>,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self { ... }
}
```

### `FlooApiError`

```rust
pub struct FlooApiError {
    pub status_code: u16,
    pub code: String,
    pub message: String,
}
```

Returned when the API returns an error response. The `FlooClient` converts API error JSON into this type.

### Rules

- Use `?` operator, never `unwrap()` in production paths
- `unwrap()` is only acceptable in tests and `#[allow(dead_code)]` development stubs
- Library functions (archive, detection, resolve) return `Result<T, FlooError>`
- Command functions return `()` and use `output::error()` + `process::exit(1)` on failure
- Use `FlooError::new("CODE", "message")` or `FlooError::with_suggestion("CODE", "message", "try this")` for user-facing errors in library functions

---

## API Client (`api_client.rs`)

### `FlooClient`

All HTTP calls go through `FlooClient`. Never use `reqwest` directly in commands.

```rust
pub struct FlooClient {
    client: reqwest::blocking::Client,
    base_url: String,
    api_key: Option<String>,
}
```

### Rules

- `handle_response()` converts API error JSON to `FlooApiError`
- Auth header injected from config automatically
- Default timeout: 30s. Deploy upload timeout: 300s
- Base URL from config, overridable via `FLOO_API_URL` env var

### Multipart Uploads

Deploy uses `reqwest::blocking::multipart::Form` with `Part::bytes()`:

```rust
let file_part = multipart::Part::bytes(file_bytes)
    .file_name(file_name)
    .mime_str("application/gzip")
    .unwrap();
let form = multipart::Form::new()
    .part("file", file_part)
    .text("runtime", runtime.to_string())
    .text("framework", framework.unwrap_or("").to_string());
```

Upload requests use a 300s timeout (vs 30s default) to handle large tarballs.

---

## Config (`config.rs`)

Manages `~/.floo/config.json`:

```json
{
  "api_key": "floo_...",
  "user_email": "user@example.com",
  "api_url": "https://api.getfloo.com"
}
```

- File permissions set to `0o600`
- `FLOO_API_URL` env var overrides stored URL
- Never hardcode the API URL in commands

---

## Detection (`detection.rs`)

Auto-detects runtime and framework from project files.

Priority order: `Dockerfile` > `package.json` > `pyproject.toml`/`requirements.txt` > `go.mod` > `index.html`

Returns `DetectionResult { runtime, framework, confidence }` where confidence is `"high"`, `"medium"`, or `"low"`.

---

## Archive (`archive.rs`)

Packs source into `.tar.gz`, respects `.flooignore` (fnmatch-style patterns).

Built-in ignores: `.git`, `node_modules`, `__pycache__`, `.venv`, `.env`, `target/`, etc.

500MB size limit. Returns `FlooError` with code `ARCHIVE_TOO_LARGE` if exceeded.

---

## Constants (`constants.rs`)

Shared constants live in `constants.rs`:

```rust
pub const DEFAULT_API_URL: &str = "https://api.getfloo.com";
pub const CONFIG_DIR_NAME: &str = ".floo";
pub const CONFIG_FILE_NAME: &str = "config.json";
pub const MAX_ARCHIVE_SIZE_MB: u64 = 500;
pub const VERSION: &str = env!("FLOO_VERSION");  // Set by build.rs
```

New constants go here. Never hardcode values in command files.

---

## Interactive Prompts

Commands use `dialoguer` for user input:

```rust
use dialoguer::{Input, Password};

let email: String = Input::new()
    .with_prompt("Email")
    .interact_text()
    .unwrap_or_else(|_| process::exit(1));

let password: String = Password::new()
    .with_prompt("Password")
    .interact()
    .unwrap_or_else(|_| process::exit(1));
```

Confirmations use `output::confirm()`:

```rust
if !output::confirm("Delete app? This cannot be undone.") {
    process::exit(0);
}
```

Commands use `process::exit(1)` on errors — they don't return `Result`. The pattern is: display error via `output::error()`, then `process::exit(1)`.

---

## `anyhow` vs `FlooError`

- **`anyhow::Result`** — only in `config.rs` for low-level IO operations (file read/write, JSON parse). These are internal errors not shown to users.
- **`FlooError`** — for user-facing errors that need a code and suggestion. Used in `archive.rs`, `detection.rs`, and similar modules.
- **Commands** — return `()` and use `process::exit(1)` after `output::error()`. Commands do NOT return `Result`.
- **`FlooApiError`** — returned by `FlooClient` methods. Commands match on this to show API error messages.

```rust
// In a command function:
match client.list_apps(1, 20) {
    Ok(data) => output::success("Apps retrieved.", &data),
    Err(e) => {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }
}
```

---

## Patterns

### Constructor ergonomics

Use `impl Into<String>` for string parameters:

```rust
pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
    Self {
        code: code.into(),
        message: message.into(),
    }
}
```

### Optional parameters

Use `Option<&str>` for optional string params:

```rust
pub fn create_app(&self, name: Option<&str>) -> Result<Value, FlooApiError> { ... }
```

### Partial updates

Use `HashMap<String, String>` for update payloads:

```rust
let mut updates = HashMap::new();
updates.insert("name".to_string(), new_name.to_string());
```

### JSON building

Use `serde_json::json!` macro:

```rust
let body = json!({
    "name": name,
    "runtime": runtime,
});
```

### Resolve (`resolve.rs`)

`resolve_app()` looks up an app by UUID or name. Used by commands that accept an app identifier.

---

## Testing

### Unit tests

Inline per file with `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = FlooError::new("TEST", "test message");
        assert_eq!(err.to_string(), "test message");
    }
}
```

### Integration tests

Use `assert_cmd` crate. Tests run the compiled binary as a subprocess.

### JSON mode state leak

The `JSON_MODE` global `AtomicBool` persists across tests run in the same process. Always reset at the start of each test:

```rust
output::set_json_mode(false);
```

---

## Style

### Tooling

| Tool | Purpose | Command |
|------|---------|---------|
| **clippy** | Lint | `cargo clippy -- -D warnings` |
| **cargo fmt** | Format | `cargo fmt --check` |
| **cargo test** | Tests | `cargo test` |

### Installer smoke checks (when installer/update paths change)

If a change touches `install.sh`, updater code, or release-asset wiring, also run:

```bash
cd /path/to/floo-cli && bash -n install.sh
cd /path/to/floo-cli && bash tests/install_script_test.sh
cd /path/to/floo-cli && bash tests/install_script_e2e.sh
```

This remains required even when CI is manual-only (`workflow_dispatch`).

### No dead code warnings

Use `#[allow(dead_code)]` sparingly and only during development. Remove before merging.

---

## Forbidden

- `println!` — use `output` module functions. The only exception is `output::print_json` which writes JSON to stdout.
- `unwrap()` in production paths — use `?` operator or explicit error handling
- `unsafe` without documented justification
- Direct `reqwest` calls outside `FlooClient`
- Hardcoded API URLs — use config or `FLOO_API_URL` env var
- `#[allow(dead_code)]` in merged code (development only)

---

## Known Pitfalls

### `output::info()` emits JSON in `--json` mode

`info(message)` outputs `{"success": true, "data": null}` to stdout in JSON mode. If a command later calls `success()` with real data, stdout has two JSON objects — breaking agents.

**Fix:** Guard with `if !is_json_mode() { info("..."); }` when the command has a final `success()`.

### JSON_MODE global state leaks between tests

The `AtomicBool` persists across test functions. Always call `output::set_json_mode(false)` at the start of every test.

---

## Error Code Reference

All error codes used in the CLI. New error codes must be `UPPER_SNAKE_CASE` and added to this list.

| Code | Source | Meaning |
|------|--------|---------|
| `NOT_AUTHENTICATED` | commands | No API key configured (login required) |
| `INVALID_PATH` | commands/deploy | Deploy path doesn't exist or isn't a directory |
| `NO_RUNTIME_DETECTED` | commands/deploy | Detection couldn't identify a runtime |
| `ARCHIVE_TOO_LARGE` | archive.rs | Source archive exceeds `MAX_ARCHIVE_SIZE_MB` |
| `ARCHIVE_ERROR` | archive.rs | Failed to create/read archive |
| `INVALID_FORMAT` | commands | Invalid hostname, app name, or other input format |
| `APP_NOT_FOUND` | commands | App lookup by name/UUID returned nothing |
| `DEPLOY_NOT_FOUND` | API (404) | Deploy ID not found for app |
| `INVALID_ROLLBACK_TARGET` | API (400) | Rollback target deploy is not LIVE |
| `ROLLBACK_IMAGE_MISSING` | API (400) | Rollback target missing stored image URI |
| `DEPLOY_TIMEOUT` | commands/deploy | Deploy polling exceeded 10-minute timeout |
| `DEPLOY_FAILED` | commands/deploy | Deploy completed but status is FAILED |
| `CONFIG_ERROR` | config.rs | Failed to read/write config file |
| `CONNECTION_ERROR` | api_client.rs | HTTP request failed (network issue) |
| `FILE_ERROR` | api_client.rs | Failed to read file for upload |
| `PARSE_ERROR` | api_client.rs | Failed to parse API response JSON |
| `API_ERROR` | api_client.rs | Generic API error (unrecognized error format) |
| `LOGS_UNAVAILABLE` | API (503) | GCP project not configured (logs require Cloud Run) |
| `LOGS_QUERY_ERROR` | API (400) | Logs query validation error (bad severity/since) |
| `LOGS_SERVICE_ERROR` | API (502) | Cloud Logging backend failure |
| `SERIALIZATION_ERROR` | api_client.rs | Failed to serialize service definitions to JSON |
| `UNSUPPORTED_PLATFORM` | updater/install | Host OS/arch is not supported for auto-install/update |
| `RELEASE_LOOKUP_FAILED` | updater | Release metadata lookup failed or returned non-200 |
| `RELEASE_PARSE_ERROR` | updater | Failed to parse release metadata JSON |
| `RELEASE_ASSET_MISSING` | updater | Expected binary asset not present in release |
| `CHECKSUM_MISSING` | updater/install | Expected checksum asset/file not present |
| `CHECKSUM_PARSE_ERROR` | updater | Checksum file exists but did not contain a valid hash |
| `CHECKSUM_MISMATCH` | updater/install | Downloaded binary hash does not match published checksum |
| `DOWNLOAD_FAILED` | updater/install | Binary or checksum download failed |
| `UPDATE_HTTP_CLIENT_ERROR` | updater | Failed to initialize HTTP client for update checks |
| `UPDATE_INSTALL_PATH_UNRESOLVED` | updater | Could not resolve currently installed CLI path |
| `UPDATE_PERMISSION_DENIED` | updater | No permission to replace installed binary |
| `UPDATE_INSTALL_FAILED` | updater | Generic failure writing or replacing updated binary |
| `DEVICE_PENDING` | api_client.rs | Device code flow — authorization still pending (HTTP 202) |
| `DEVICE_CODE_EXPIRED` | API (410) | Device code flow — device code timed out |
| `DEVICE_AUTH_DENIED` | API (403) | Device code flow — user denied authorization |
| `DEVICE_AUTH_FAILED` | API (502) | Device code flow — WorkOS backend failure |
| `EMAIL_TAKEN` | API (409) | Registration — email already in use |
| `PERMISSION_DENIED` | API (403) | User has VIEWER role; write action denied |
| `INVALID_PROJECT_CONFIG` | project_config.rs | Malformed or invalid `floo.toml` |
