use std::process;

use serde_json::Value;

use crate::errors::ErrorCode;
use crate::output;

pub fn query(app_flag: Option<&str>, sql: &str, environment: &str, limit: u32) {
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
    let rows_val = result.get("rows").or_else(|| result.get("results"));
    let Some(rows_arr) = rows_val.and_then(|v| v.as_array()) else {
        output::info("No rows returned.", None);
        return;
    };

    if rows_arr.is_empty() {
        output::info("0 rows", None);
        return;
    }

    // Collect column names from the first row's keys (preserving insertion order).
    let headers: Vec<String> = rows_arr[0]
        .as_object()
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();

    if headers.is_empty() {
        output::info("0 rows", None);
        return;
    }

    let table_rows: Vec<Vec<String>> = rows_arr
        .iter()
        .map(|row| {
            headers
                .iter()
                .map(|h| {
                    row.get(h)
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

pub fn migrate(app_flag: Option<&str>, env: &str) {
    // Dry-run is a pure echo, like cron.rs:run — runs before require_auth()
    // and resolve_app_from_config() so logged-out users and offline agents
    // can preview the action without an API call. The preview reports the
    // app + environment the migration would target; previewing the actual
    // pending-migration set would require a server-side endpoint we don't
    // yet expose.
    if output::is_dry_run_mode() {
        let target = app_flag.unwrap_or("(reads from config)");
        let preview = format!("Would run pending migrations on '{target}' (env: {env}).");
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "db_migrate",
                "app": app_flag,
                "env": env,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);

    if !output::is_json_mode() {
        output::info(
            &format!("Running migrations for {app_name} ({env})..."),
            None,
        );
    }

    let result = match client.db_migrate(&app_id, env) {
        Ok(r) => r,
        Err(e) => {
            let suggestion = match e.code.as_str() {
                "AGENT_MODE_DDL_BLOCKED" => Some(
                    "Migrations run DDL, which requires agent_mode = \"autonomous\". \
                     Set agent_mode in [app] in floo.app.toml (or omit it to default \
                     to autonomous), commit, then push to redeploy before re-running.",
                ),
                "AGENT_MODE_READONLY" => Some(
                    "Agent mode is \"readonly\". Set agent_mode = \"autonomous\" in \
                     [app] in floo.app.toml to run migrations.",
                ),
                "AGENT_MODE_SUPERVISED" => Some(
                    "Agent mode is \"supervised\", which blocks prod migrations. \
                     Run against --env dev, or set agent_mode = \"autonomous\" in \
                     [app] in floo.app.toml.",
                ),
                _ => None,
            };
            output::error(&e.message, &ErrorCode::from_api(&e.code), suggestion);
            process::exit(1);
        }
    };

    // Print streamed output if present.
    if !output::is_json_mode() {
        if let Some(output_str) = result.get("output").and_then(|v| v.as_str()) {
            if !output_str.is_empty() {
                for line in output_str.lines() {
                    output::info(line, None);
                }
            }
        }
    }

    let success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if success {
        output::success("Migrations complete.", Some(result));
    } else {
        let msg = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Migration failed.");
        output::error(msg, &ErrorCode::Other("MIGRATION_FAILED".into()), None);
        process::exit(1);
    }
}

pub fn connections(app_flag: Option<&str>, env: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);

    // Find the app's managed Postgres service.
    let listing = match client.list_managed_services(&app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };
    let postgres_service = listing
        .managed_services
        .iter()
        .find(|s| s.service_type == "postgres");

    let Some(service) = postgres_service else {
        output::error(
            &format!("{app_name} has no managed Postgres service to inspect."),
            &ErrorCode::Other("NO_POSTGRES_SERVICE".into()),
            Some("Provision one with `floo services add postgres --app <name>`."),
        );
        process::exit(1);
    };

    let usage = match client.managed_postgres_connection_usage(&app_id, &service.id, env) {
        Ok(v) => v,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success("Connection usage retrieved.", Some(usage));
        return;
    }

    let used = usage
        .get("active_connections")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let limit = usage
        .get("connection_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let ratio = usage.get("ratio").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let near = usage
        .get("near_capacity")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let support_email = usage
        .get("support_email")
        .and_then(|v| v.as_str())
        .unwrap_or("team@getfloo.com");

    let percent = (ratio * 100.0).round() as u32;

    output::info(
        &format!("{app_name} ({env}): {used}/{limit} Postgres connections in use ({percent}%)"),
        None,
    );

    if near {
        output::info(
            "Heads up — you're near capacity. Most apps that hit this either need \
             connection pooling at the application layer (PgBouncer, SQLAlchemy pool \
             tuning) or more raw capacity.",
            None,
        );
        output::info(
            &format!("Need more? Email {support_email} — we can provision a dedicated instance."),
            None,
        );
    }
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
        assert_eq!(value_to_display(&json!(3.14)), "3.14");
    }

    #[test]
    fn test_value_to_display_string() {
        assert_eq!(value_to_display(&json!("hello")), "hello");
    }
}
