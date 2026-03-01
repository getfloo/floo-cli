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
project_config/             <- Config file system
  mod.rs                    <- Re-exports, constants (SERVICE_CONFIG_FILE, APP_CONFIG_FILE)
  service_config.rs         <- floo.service.toml structs + ServiceConfig API wire type
  app_config.rs             <- floo.app.toml structs
  resolve.rs                <- Walk-up directory resolution (resolve_app_context)
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
- Use `FlooError::new(ErrorCode::Variant, "message")` or `FlooError::with_suggestion(ErrorCode::Variant, "message", "try this")` for user-facing errors in library functions
- `output::error()` takes `&ErrorCode` (not `&str`) — use `&ErrorCode::Variant` for literals, or `&ErrorCode::from_api(&e.code)` when forwarding a `FlooApiError`

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
        output::error(&e.message, &ErrorCode::from_api(&e.code), None);
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

All error codes used in the CLI. Codes in the `ErrorCode` enum are compile-time typed — use `ErrorCode::Variant` (not string literals). API-sourced codes (from `FlooApiError`) are runtime strings forwarded via `ErrorCode::from_api(&e.code)`.

New error codes must be `UPPER_SNAKE_CASE`, added to the `ErrorCode` enum in `src/errors.rs` (with `as_str()` and `from_api()` arms), and added to this table.

| Code | `ErrorCode` variant | Source | Meaning |
|------|---------------------|--------|---------|
| `APP_NAME_MISMATCH` | `AppNameMismatch` | project_config | App name in config doesn't match the deployed app |
| `APP_NOT_FOUND` | `AppNotFound` | commands | App lookup by name/UUID returned nothing |
| `ARCHIVE_ERROR` | `ArchiveError` | archive.rs | Failed to create/read archive |
| `ARCHIVE_TOO_LARGE` | `ArchiveTooLarge` | archive.rs | Source archive exceeds `MAX_ARCHIVE_SIZE_MB` |
| `CHECKSUM_MISMATCH` | `ChecksumMismatch` | updater/install | Downloaded binary hash does not match published checksum |
| `CHECKSUM_MISSING` | `ChecksumMissing` | updater/install | Expected checksum asset/file not present |
| `CHECKSUM_PARSE_ERROR` | `ChecksumParseError` | updater | Checksum file exists but did not contain a valid hash |
| `CONFIG_ERROR` | `ConfigError` | config.rs | Failed to read/write config file |
| `CONFIG_EXISTS` | `ConfigExists` | commands/init | Config file already exists at target path |
| `CONFIG_INVALID` | `ConfigInvalid` | commands/check | Config file is present but cannot be parsed |
| `CONFIG_WRITE_ERROR` | `ConfigWriteError` | project_config/ | Failed to serialize or write config file |
| `CWD_ERROR` | `CwdError` | commands | Failed to determine the current working directory |
| `DATABASE_NOT_FOUND` | `DatabaseNotFound` | commands | Named database service not found in config or API |
| `DEPLOY_FAILED` | `DeployFailed` | commands/deploy | Deploy completed but status is FAILED |
| `DEPLOY_TIMEOUT` | `DeployTimeout` | commands/deploy | Deploy polling exceeded 10-minute timeout |
| `DEVICE_AUTH_DENIED` | `DeviceAuthDenied` | API (403) | Device code flow — user denied authorization |
| `DEVICE_CODE_EXPIRED` | `DeviceCodeExpired` | API (410) | Device code flow — device code timed out |
| `DOWNLOAD_FAILED` | `DownloadFailed` | updater/install | Binary or checksum download failed |
| `DUPLICATE_SERVICE` | `DuplicateService` | commands/service_mgmt | Service name already exists in config |
| `DUPLICATE_SERVICE_NAMES` | `DuplicateServiceNames` | project_config | Two services share the same name in floo.app.toml |
| `EMAIL_TAKEN` | `EmailTaken` | API (409) | Registration — email already in use |
| `ENV_FILE_NOT_FOUND` | `EnvFileNotFound` | commands/env | Specified .env file path does not exist |
| `ENV_PARSE_ERROR` | `EnvParseError` | commands/env | Failed to parse .env file contents |
| `FILE_ERROR` | `FileError` | commands | Failed to read a file for processing or upload |
| `FLOOIGNORE_READ_FAILED` | `FlooignoreReadFailed` | archive.rs | Failed to read `.flooignore` file |
| `INTERNAL_ERROR` | `InternalError` | commands | Unexpected internal state (should not normally occur) |
| `INVALID_AMOUNT` | `InvalidAmount` | commands/billing | Spend cap amount is out of valid range |
| `INVALID_FORMAT` | `InvalidFormat` | commands | Invalid hostname, app name, or other input format |
| `INVALID_INGRESS` | `InvalidIngress` | commands/service_mgmt | Ingress value is not `public` or `private` |
| `INVALID_PATH` | `InvalidPath` | commands/deploy | Deploy path doesn't exist or isn't a directory |
| `INVALID_PROJECT_CONFIG` | `InvalidProjectConfig` | project_config/ | Malformed or invalid `floo.service.toml` or `floo.app.toml` |
| `INVALID_RESPONSE` | `InvalidResponse` | commands | API response shape was unexpected |
| `INVALID_ROLE` | `InvalidRole` | commands/orgs | Role string is not one of the allowed org member roles |
| `INVALID_SERVICE_NAME` | `InvalidServiceName` | commands/service_mgmt | Service name contains invalid characters or is reserved |
| `INVALID_TYPE` | `InvalidType` | commands/service_mgmt | Service type is not a recognized value |
| `LEGACY_CONFIG` | `LegacyConfig` | project_config/resolve.rs | Found old `floo.toml`, must migrate to new config files |
| `MISSING_APP_NAME` | `MissingAppName` | project_config | App name is required but not set in config or flag |
| `MISSING_ARGUMENT` | `MissingArgument` | commands/skills | Required CLI argument not provided (e.g. --path or --print) |
| `MISSING_PORT` | `MissingPort` | commands/service_mgmt | Worker service definition missing required port |
| `MISSING_TYPE` | `MissingType` | commands/service_mgmt | Service definition missing required type field |
| `MULTIPLE_SERVICES` | `MultipleServices` | project_config | Multiple services exist but no target was specified |
| `MULTIPLE_SERVICES_NO_TARGET` | `MultipleServicesNoTarget` | commands | Multiple services and no `--service` flag provided |
| `NO_CONFIG_FOUND` | `NoConfigFound` | project_config/resolve.rs | No config files found and no `--app` flag |
| `NO_DEPLOYABLE_SERVICES` | `NoDeployableServices` | commands/deploy | App config has no services eligible for deploy |
| `NO_ENV_FILES` | `NoEnvFiles` | commands/env | No .env files found to import |
| `NO_PUBLIC_SERVICES` | `NoPublicServices` | commands | App has no public-facing services (needed for domains) |
| `NO_RUNTIME_DETECTED` | `NoRuntimeDetected` | commands/deploy | Detection couldn't identify a runtime |
| `NOT_AUTHENTICATED` | `NotAuthenticated` | commands | No API key configured (login required) |
| `PARSE_ERROR` | `ParseError` | commands/api_client | Failed to parse API response or config JSON |
| `RELEASE_ASSET_MISSING` | `ReleaseAssetMissing` | updater | Expected binary asset not present in release |
| `RELEASE_LOOKUP_FAILED` | `ReleaseLookupFailed` | updater | Release metadata lookup failed or returned non-200 |
| `RELEASE_NOT_FOUND` | `ReleaseNotFound` | commands/releases | Specified release ID or tag not found |
| `RELEASE_PARSE_ERROR` | `ReleaseParseError` | updater | Failed to parse release metadata JSON |
| `RESTART_FAILED` | `RestartFailed` | commands/service_mgmt | Service restart operation failed |
| `SERVICE_CONFIG_MISSING` | `ServiceConfigMissing` | project_config | Expected service config block is absent |
| `SERVICE_NOT_FOUND` | `ServiceNotFound` | commands | Named service not found in config or API |
| `SIGNUP_DISABLED` | `SignupDisabled` | API | New registrations are disabled |
| `STREAM_ERROR` | `StreamError` | commands/logs | Error reading from a streaming log response |
| `UNKNOWN_SERVICE` | `UnknownService` | commands | `--service` flag references a service not in config |
| `UNSUPPORTED_PLATFORM` | `UnsupportedPlatform` | updater/install | Host OS/arch is not supported for auto-install/update |
| `UPDATE_HTTP_CLIENT_ERROR` | `UpdateHttpClientError` | updater | Failed to initialize HTTP client for update checks |
| `UPDATE_INSTALL_FAILED` | `UpdateInstallFailed` | updater | Generic failure writing or replacing updated binary |
| `UPDATE_INSTALL_PATH_UNRESOLVED` | `UpdateInstallPathUnresolved` | updater | Could not resolve currently installed CLI path |
| `UPDATE_PERMISSION_DENIED` | `UpdatePermissionDenied` | updater | No permission to replace installed binary |

### API-only codes (runtime strings via `ErrorCode::Other`)

These codes come from `FlooApiError.code` at runtime and are not enum variants. They are forwarded with `ErrorCode::from_api(&e.code)`.

| Code | Source | Meaning |
|------|--------|---------|
| `API_ERROR` | api_client.rs | Generic API error (unrecognized error format) |
| `CONNECTION_ERROR` | api_client.rs | HTTP request failed (network issue) |
| `DEPLOY_NOT_FOUND` | API (404) | Deploy ID not found for app |
| `DEVICE_AUTH_FAILED` | API (502) | Device code flow — WorkOS backend failure |
| `DEVICE_PENDING` | api_client.rs | Device code flow — authorization still pending (HTTP 202) |
| `INVALID_ROLLBACK_TARGET` | API (400) | Rollback target deploy is not LIVE |
| `LOGS_QUERY_ERROR` | API (400) | Logs query validation error (bad severity/since) |
| `LOGS_SERVICE_ERROR` | API (502) | Cloud Logging backend failure |
| `LOGS_UNAVAILABLE` | API (503) | GCP project not configured (logs require Cloud Run) |
| `PERMISSION_DENIED` | API (403) | User has VIEWER role; write action denied |
| `ROLLBACK_IMAGE_MISSING` | API (400) | Rollback target missing stored image URI |
| `SERIALIZATION_ERROR` | api_client.rs | Failed to serialize service definitions to JSON |
