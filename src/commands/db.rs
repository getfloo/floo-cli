use std::path::{Path, PathBuf};
use std::process;

use percent_encoding::percent_decode_str;
use serde_json::Value;
use url::Url;

use crate::errors::{ErrorCode, FlooError};
use crate::output;

/// The inclusive `--limit` bound the API enforces. Mirrors
/// `DbQueryRequest.limit = Field(ge=1, le=10000)` in `api/app/routes/db.py` —
/// that endpoint is the single source of truth; keep these in sync. Validating
/// it client-side turns the API's framework-level 422 validation blob into a
/// clean, `--json`-aware floo error before any request is built.
const QUERY_LIMIT_RANGE: std::ops::RangeInclusive<u32> = 1..=10_000;

/// Validate `db query` arguments offline (no auth, no network).
///
/// A malformed request must fail with a floo-shaped error that honors `--json`
/// — never reach the API to bounce back a raw Pydantic/FastAPI validation blob,
/// and never execute under `--dry-run`. Empty/whitespace SQL is caught here too
/// so it surfaces as "query is empty" rather than the API's key-scope DDL check
/// (an empty string classifies as DDL server-side). Pure and unit-tested so the
/// preview and the real run share one notion of "valid".
fn validate_query_args(sql: &str, limit: u32) -> Result<(), FlooError> {
    if sql.trim().is_empty() {
        return Err(FlooError::with_suggestion(
            ErrorCode::Other("EMPTY_QUERY".to_string()),
            "Query is empty.",
            "Pass a SQL statement, e.g. `floo db query \"SELECT 1\"`.",
        ));
    }
    if !QUERY_LIMIT_RANGE.contains(&limit) {
        return Err(FlooError::with_suggestion(
            ErrorCode::Other("INVALID_LIMIT".to_string()),
            format!(
                "--limit must be between {} and {} (got {limit}).",
                QUERY_LIMIT_RANGE.start(),
                QUERY_LIMIT_RANGE.end(),
            ),
            "Re-run with a --limit inside that range.",
        ));
    }
    Ok(())
}

pub fn query(app_flag: Option<&str>, sql: &str, environment: &str, limit: u32) {
    // Validate offline first so a bad request fails cleanly (honoring --json)
    // before any auth/network — and so a --dry-run of an invalid query reports
    // the same error the real run would, not a confident "would run".
    if let Err(e) = validate_query_args(sql, limit) {
        output::error(&e.message, &e.code, e.suggestion.as_deref());
        process::exit(1);
    }

    // Dry-run stays offline and side-effect-free. `db query` executes ARBITRARY
    // SQL (INSERT/UPDATE/DELETE/DDL), so it is NOT a read-only command — a dry
    // run must never reach the API. Like every other --dry-run handler it runs
    // before require_auth() (mirrors cron.rs).
    if output::is_dry_run_mode() {
        let target = app_flag.unwrap_or("(reads from config)");
        let preview = format!(
            "Would run this SQL against '{target}' (env: {environment}, limit: {limit}). \
             No query is executed in dry-run mode.\nSQL: {sql}"
        );
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "db_query",
                "app": app_flag,
                "env": environment,
                "limit": limit,
                "sql": sql,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _app_name) = super::resolve_app_from_config(&client, app_flag);

    let result = match client.db_query(&app_id, sql, environment, limit) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success("Query executed.", Some(result));
        return;
    }

    // Human mode: render a table from the result rows.
    //
    // The API returns rows as an array of arrays, with the column names
    // carried separately in `columns` (the `DbQueryResponse { columns, rows }`
    // contract). Each inner array is one row, positionally aligned to
    // `columns`. This must mirror what `--json` exposes — one row contract,
    // consumed identically by both output modes.
    let rows_val = result.get("rows").or_else(|| result.get("results"));
    let Some(rows_arr) = rows_val.and_then(|v| v.as_array()) else {
        output::info("No rows returned.", None);
        return;
    };

    if rows_arr.is_empty() {
        output::info("0 rows", None);
        return;
    }

    // Column names from `columns` drive the header row and per-cell alignment.
    let mut headers: Vec<String> = result
        .get("columns")
        .and_then(|v| v.as_array())
        .map(|cols| {
            cols.iter()
                .map(|c| {
                    c.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| c.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    if headers.is_empty() {
        // `columns` was absent or empty but we have rows: synthesize positional
        // headers from the widest row so genuine data never collapses to a
        // bogus "0 rows" report (the failure mode of #153).
        let width = rows_arr
            .iter()
            .filter_map(|r| r.as_array().map(|a| a.len()))
            .max()
            .unwrap_or(0);
        headers = (1..=width).map(|i| format!("column_{i}")).collect();
    }

    let table_rows: Vec<Vec<String>> = rows_arr
        .iter()
        .map(|row| {
            let cells = row.as_array();
            (0..headers.len())
                .map(|i| {
                    cells
                        .and_then(|c| c.get(i))
                        .map(value_to_display)
                        .unwrap_or_else(|| "-".to_string())
                })
                .collect()
        })
        .collect();

    let header_refs: Vec<&str> = headers.iter().map(|s| s.as_str()).collect();
    let count = table_rows.len();
    output::table(&header_refs, &table_rows, None);
    output::info(
        &format!("{count} row{}", if count == 1 { "" } else { "s" }),
        None,
    );

    // The API caps the result set and flags `truncated` when more rows match
    // (it reads limit+1 to detect the overflow). Surface it so a capped result
    // never silently looks complete. Report the actual returned `count`, not the
    // requested limit, so the message stays honest if the contract changes.
    if result
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        output::info(
            &format!("Showing the first {count} rows; more rows match. Raise --limit to see more."),
            None,
        );
    }
}

pub fn schema(app_flag: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, _app_name) = super::resolve_app_from_config(&client, app_flag);

    let result = match client.db_schema(&app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success("Schema retrieved.", Some(result));
        return;
    }

    // Human mode: print tables with columns and types.
    if let Some(schema_name) = result.get("schema_name").and_then(|v| v.as_str()) {
        output::info(&format!("Schema: {schema_name}"), None);
    }

    let tables_val = result.get("tables");
    let Some(tables_arr) = tables_val.and_then(|v| v.as_array()) else {
        output::info("No schema information available.", None);
        return;
    };

    if tables_arr.is_empty() {
        output::info("No tables found.", None);
        return;
    }

    for table in tables_arr {
        let table_name = table
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed)");
        output::info(&format!("Table: {table_name}"), None);

        let columns = table.get("columns").and_then(|v| v.as_array());
        let Some(cols) = columns else {
            output::info("  (no columns)", None);
            continue;
        };

        let rows: Vec<Vec<String>> = cols
            .iter()
            .map(|col| {
                let name = col
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string();
                let col_type = col
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string();
                let nullable = col
                    .get("nullable")
                    .and_then(|v| v.as_bool())
                    .map(|b| if b { "YES" } else { "NO" })
                    .unwrap_or("-")
                    .to_string();
                vec![name, col_type, nullable]
            })
            .collect();

        output::table(&["Column", "Type", "Nullable"], &rows, None);
    }
}

const PG_DUMP_INSTALL_HINT: &str = "Install the PostgreSQL 17+ client tools: \
macOS `brew install libpq && brew link --force libpq`, Debian/Ubuntu `apt install postgresql-client-17`, \
or https://www.postgresql.org/download/";

/// The `pg_dump` command line for one dump. The password travels in
/// `PGPASSWORD`, never in `args`, so it does not show up in `ps`.
#[derive(Debug)]
struct PgDumpInvocation {
    args: Vec<String>,
    password: String,
}

/// Build the `pg_dump` invocation from the environment's `DATABASE_URL`.
///
/// `--no-owner --no-privileges` keep the dump restorable into any Postgres:
/// the managed role is named `app`, which the destination will not have.
fn pg_dump_invocation(
    database_url: &str,
    file: &Path,
    format: &str,
) -> Result<PgDumpInvocation, FlooError> {
    let invalid = || {
        FlooError::new(
            ErrorCode::Other("INVALID_DATABASE_URL".to_string()),
            "DATABASE_URL is not a postgres:// connection URL.",
        )
    };
    let mut url = Url::parse(database_url).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        return Err(invalid());
    }
    let password = percent_decode_str(url.password().unwrap_or(""))
        .decode_utf8()
        .map_err(|_| invalid())?
        .into_owned();
    url.set_password(None).map_err(|_| invalid())?;
    let format_flag = if format == "custom" { "c" } else { "p" };
    Ok(PgDumpInvocation {
        args: vec![
            format!("--dbname={url}"),
            format!("--file={}", file.display()),
            format!("--format={format_flag}"),
            "--no-owner".to_string(),
            "--no-privileges".to_string(),
            "--no-password".to_string(),
        ],
        password,
    })
}

/// The local `pg_dump` version line, or `None` when it is not on PATH.
fn pg_dump_version() -> Option<String> {
    let out = process::Command::new("pg_dump")
        .arg("--version")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Map a failed `pg_dump` run to a floo error. pg_dump refuses to dump a
/// server newer than itself; that case gets the install hint.
fn pg_dump_failure(stderr: &str) -> FlooError {
    let detail = stderr.trim();
    if detail.contains("server version mismatch") {
        return FlooError::with_suggestion(
            ErrorCode::Other("PG_DUMP_VERSION_MISMATCH".to_string()),
            format!("pg_dump is older than the database server. {detail}"),
            PG_DUMP_INSTALL_HINT,
        );
    }
    FlooError::new(
        ErrorCode::Other("PG_DUMP_FAILED".to_string()),
        format!("pg_dump failed. {detail}"),
    )
}

fn exit_with(e: &FlooError) -> ! {
    output::error(&e.message, &e.code, e.suggestion.as_deref());
    process::exit(1);
}

pub fn dump(app_flag: Option<&str>, environment: &str, file: &Path, format: &str) {
    // Check the local tool before reading the credential, so a missing
    // pg_dump never costs an audited secret read.
    let Some(version) = pg_dump_version() else {
        exit_with(&FlooError::with_suggestion(
            ErrorCode::Other("PG_DUMP_NOT_FOUND".to_string()),
            "pg_dump was not found on PATH. `floo db dump` runs your local pg_dump.",
            PG_DUMP_INSTALL_HINT,
        ));
    };

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);

    let database_url = match client.get_env_var(&app_id, "DATABASE_URL", None, environment) {
        Ok(var) => var.value.unwrap_or_default(),
        Err(e) if e.code == "ENV_VAR_NOT_FOUND" => exit_with(&FlooError::with_suggestion(
            ErrorCode::Other("DATABASE_NOT_FOUND".to_string()),
            format!("'{app_name}' has no DATABASE_URL in {environment}, so there is no database to dump."),
            format!("Run `floo env list --app {app_name} --env {environment}` to see what is set."),
        )),
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    // Write to a sibling `.partial` file and rename on success, so a failed
    // dump never leaves a truncated file that looks complete, and never
    // clobbers an earlier good dump at the same path.
    let mut partial = file.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);

    let invocation = match pg_dump_invocation(&database_url, &partial, format) {
        Ok(i) => i,
        Err(e) => exit_with(&e),
    };

    if !output::is_json_mode() {
        output::info(
            &format!("Dumping the {environment} database of '{app_name}' with {version}..."),
            None,
        );
    }

    let run = process::Command::new("pg_dump")
        .args(&invocation.args)
        .env("PGPASSWORD", &invocation.password)
        // Applies only when DATABASE_URL does not set sslmode itself.
        .env("PGSSLMODE", "require")
        .stdin(process::Stdio::null())
        .output();

    let failure = match run {
        Ok(out) if out.status.success() => std::fs::rename(&partial, file).err().map(|e| {
            FlooError::new(
                ErrorCode::Other("DUMP_WRITE_FAILED".to_string()),
                format!("Could not write {}: {e}", file.display()),
            )
        }),
        Ok(out) => Some(pg_dump_failure(&String::from_utf8_lossy(&out.stderr))),
        Err(e) => Some(pg_dump_failure(&e.to_string())),
    };
    if let Some(e) = failure {
        let _ = std::fs::remove_file(&partial);
        exit_with(&e);
    }

    let bytes = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    output::success(
        &format!(
            "Wrote the {environment} database of '{app_name}' to {} ({bytes} bytes).",
            file.display()
        ),
        Some(serde_json::json!({
            "app": app_name,
            "env": environment,
            "file": file.display().to_string(),
            "format": format,
            "bytes": bytes,
        })),
    );
}

fn value_to_display(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_value_to_display_null() {
        assert_eq!(value_to_display(&Value::Null), "NULL");
    }

    #[test]
    fn test_value_to_display_bool() {
        assert_eq!(value_to_display(&json!(true)), "true");
        assert_eq!(value_to_display(&json!(false)), "false");
    }

    #[test]
    fn test_value_to_display_number() {
        assert_eq!(value_to_display(&json!(42)), "42");
        assert_eq!(value_to_display(&json!(2.5)), "2.5");
    }

    #[test]
    fn test_value_to_display_string() {
        assert_eq!(value_to_display(&json!("hello")), "hello");
    }

    #[test]
    fn test_validate_query_args_accepts_valid() {
        assert!(validate_query_args("SELECT 1", 1).is_ok());
        assert!(validate_query_args("SELECT 1", 1000).is_ok());
        // Inclusive bounds: both ends are valid.
        assert!(validate_query_args("SELECT 1", 10_000).is_ok());
    }

    #[test]
    fn test_validate_query_args_rejects_empty_sql() {
        for sql in ["", "   ", "\t\n"] {
            let err = validate_query_args(sql, 100).unwrap_err();
            assert_eq!(err.code.as_str(), "EMPTY_QUERY", "sql={sql:?}");
        }
    }

    #[test]
    fn test_validate_query_args_rejects_out_of_range_limit() {
        // 0 is the only invalid low value reachable through the clap u32 arg.
        assert_eq!(
            validate_query_args("SELECT 1", 0)
                .unwrap_err()
                .code
                .as_str(),
            "INVALID_LIMIT"
        );
        assert_eq!(
            validate_query_args("SELECT 1", 10_001)
                .unwrap_err()
                .code
                .as_str(),
            "INVALID_LIMIT"
        );
    }

    #[test]
    fn test_pg_dump_invocation_moves_password_out_of_args() {
        let inv = pg_dump_invocation(
            "postgresql://app:p%40ss%2Fw0rd@ep-x.us-east-2.aws.neon.tech:5432/app?sslmode=require",
            Path::new("out.sql.partial"),
            "plain",
        )
        .unwrap();
        assert_eq!(inv.password, "p@ss/w0rd");
        assert_eq!(
            inv.args,
            vec![
                "--dbname=postgresql://app@ep-x.us-east-2.aws.neon.tech:5432/app?sslmode=require",
                "--file=out.sql.partial",
                "--format=p",
                "--no-owner",
                "--no-privileges",
                "--no-password",
            ]
        );
        assert!(inv.args.iter().all(|a| !a.contains("w0rd")));
    }

    #[test]
    fn test_pg_dump_invocation_custom_format() {
        let inv =
            pg_dump_invocation("postgres://app:pw@h/app", Path::new("x.dump"), "custom").unwrap();
        assert!(inv.args.contains(&"--format=c".to_string()));
    }

    #[test]
    fn test_pg_dump_invocation_rejects_non_postgres_url() {
        for url in ["", "mysql://u:p@h/db", "not a url"] {
            let err = pg_dump_invocation(url, Path::new("x"), "plain").unwrap_err();
            assert_eq!(err.code.as_str(), "INVALID_DATABASE_URL", "url={url:?}");
        }
    }

    #[test]
    fn test_pg_dump_failure_flags_version_mismatch() {
        let err = pg_dump_failure(
            "pg_dump: error: aborting because of server version mismatch\n\
             pg_dump: detail: server version: 17.5; pg_dump version: 16.4\n",
        );
        assert_eq!(err.code.as_str(), "PG_DUMP_VERSION_MISMATCH");
        assert!(err.message.contains("17.5"));
        assert!(err.suggestion.is_some());
        assert_eq!(
            pg_dump_failure("connection refused").code.as_str(),
            "PG_DUMP_FAILED"
        );
    }

    #[test]
    fn test_validate_query_args_empty_check_precedes_limit_check() {
        // An empty query with a bad limit reports the empty-query error first —
        // the SQL is the more fundamental problem to surface.
        let err = validate_query_args("", 0).unwrap_err();
        assert_eq!(err.code.as_str(), "EMPTY_QUERY");
    }
}
