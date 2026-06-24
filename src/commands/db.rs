use std::process;

use serde_json::Value;

use crate::api_types::{
    ManagedPostgresBackup, ManagedPostgresRestoreResponse, ManagedServiceSummary,
    PreviewDatabaseBranch, PreviewEnvironment,
};
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
/// so it surfaces as "query is empty" rather than the API's agent-mode DDL gate
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
    // before require_auth() (mirrors cron.rs / db migrate above).
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
        if queries_public_schema(sql) {
            // The API auto-sets `search_path` to the app's namespaced schema
            // (e.g. `app_<id>_dev`), so unqualified table refs work. Explicit
            // `WHERE table_schema = 'public'` queries return empty because
            // the app's tables are not in `public`. Surface the hint before
            // the user copy-pastes the next variant of the same query.
            output::info(
                "This app's tables live in a namespaced schema, not 'public'. \
                 Run `floo db schema` to see the schema name, or use \
                 `current_schema()` / `WHERE table_schema = current_schema()`.",
                None,
            );
        }
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
        // Surface the namespaced schema name up-front so anyone writing raw
        // SQL has the schema to qualify against. The API auto-applies it
        // via `search_path` for /db/query, but introspection queries that
        // hard-code `table_schema = 'public'` return empty without it.
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
    let service = resolve_postgres_service(&client, &app_id, &app_name, "default");

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

pub fn backup(app_flag: Option<&str>, name: &str, env: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let service = resolve_postgres_service(&client, &app_id, &app_name, name);

    let backup = match client.create_managed_postgres_backup(&app_id, &service.id, env) {
        Ok(response) => response,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success("Postgres backup created.", Some(output::to_value(&backup)));
        return;
    }

    output::success(
        &format!("Created Postgres backup for {app_name} (postgres:{name}, env={env})."),
        None,
    );
    render_backup(&backup);
}

pub fn backups(app_flag: Option<&str>, name: &str, env: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let service = resolve_postgres_service(&client, &app_id, &app_name, name);

    let response = match client.list_managed_postgres_backups(&app_id, &service.id, env) {
        Ok(response) => response,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            "Postgres backups retrieved.",
            Some(output::to_value(&response)),
        );
        return;
    }

    let env_label = env.unwrap_or("all");
    output::info(
        &format!("Postgres backups for {app_name} (postgres:{name}, env={env_label}):"),
        None,
    );
    if response.backups.is_empty() {
        output::info("No backups found.", None);
        return;
    }

    let rows: Vec<Vec<String>> = response
        .backups
        .iter()
        .map(|backup| {
            vec![
                backup.id.clone(),
                backup.env.clone(),
                backup.status.clone(),
                backup.size_human.clone(),
                backup.created_at.clone(),
                backup.expires_at.clone(),
                backup
                    .last_restored_at
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();
    output::table(
        &[
            "Backup ID",
            "Env",
            "Status",
            "Size",
            "Created",
            "Expires",
            "Restored",
        ],
        &rows,
        None,
    );
}

pub fn restore(app_flag: Option<&str>, name: &str, env: &str, backup_id: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let service = resolve_postgres_service(&client, &app_id, &app_name, name);

    let response =
        match client.restore_managed_postgres_backup(&app_id, &service.id, backup_id, env) {
            Ok(response) => response,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };

    if output::is_json_mode() {
        output::success(
            "Postgres backup restored.",
            Some(output::to_value(&response)),
        );
        return;
    }

    render_restore(&response, &app_name, name, env);
}

pub fn branches_list(app_flag: Option<&str>, preview_identifier: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let preview_slug = resolve_preview_slug(&client, &app_id, &app_name, preview_identifier);

    let preview = match client.get_preview(&app_id, &preview_slug) {
        Ok(preview) => preview,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };
    let branches = match client.list_preview_database_branches(&app_id, &preview_slug) {
        Ok(response) => response.database_branches,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            "Preview database branches retrieved.",
            Some(serde_json::json!({
                "app": {
                    "id": app_id,
                    "name": app_name,
                },
                "preview": preview_json(&preview),
                "database_branches": output::to_value(&branches),
                "total": branches.len(),
            })),
        );
        return;
    }

    output::info(
        &format!(
            "Preview database branches for {app_name} (preview: {}):",
            preview.slug
        ),
        None,
    );
    render_preview_context(&preview);
    if branches.is_empty() {
        output::info(
            "No preview database branches. This preview has no managed Postgres attachment.",
            None,
        );
        return;
    }
    render_branch_table(&branches);
}

pub fn branches_show(app_flag: Option<&str>, preview_identifier: &str, name: &str) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let preview_slug = resolve_preview_slug(&client, &app_id, &app_name, preview_identifier);

    let branch = match client.get_preview_database_branch(&app_id, &preview_slug, name) {
        Ok(branch) => branch,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            "Preview database branch retrieved.",
            Some(serde_json::json!({
                "app": {
                    "id": app_id,
                    "name": app_name,
                },
                "preview": {
                    "slug": preview_slug,
                    "environment_name": "preview",
                },
                "database_branch": output::to_value(&branch),
            })),
        );
        return;
    }

    output::info(
        &format!(
            "Preview database branch {} for {app_name} (preview: {preview_slug}):",
            branch.name
        ),
        None,
    );
    render_branch_detail(&branch);
}

pub fn branches_reset(app_flag: Option<&str>, preview_identifier: &str, name: &str, yes: bool) {
    use crate::confirm::{confirm_tier2, ConfirmOutcome, RiskMetadata, Tier};

    if output::is_dry_run_mode() {
        let risk: RiskMetadata = Tier::Two.into();
        let preview_slug = normalize_preview_identifier(preview_identifier)
            .unwrap_or_else(|| preview_identifier.trim().to_string());
        output::dry_run_preview(
            &format!(
                "Would reset preview database branch '{name}' on preview '{preview_slug}'. Dev and prod are untouched."
            ),
            serde_json::json!({
                "action": "preview_database_branch_reset",
                "app": app_flag,
                "preview": preview_slug,
                "branch": name,
                "destructive": risk.destructive,
                "data_loss": risk.data_loss,
                "tier": risk.tier,
                "scope": "preview",
                "dev_prod_untouched": true,
            }),
        );
        return;
    }

    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app_flag);
    let preview_slug = resolve_preview_slug(&client, &app_id, &app_name, preview_identifier);

    match confirm_tier2(
        "Reset preview database branch",
        &format!("{name} on {app_name}/{preview_slug} (dev/prod untouched)"),
        yes,
    ) {
        ConfirmOutcome::Proceed => {}
        ConfirmOutcome::Aborted => {
            if output::is_json_mode() {
                output::success("Cancelled.", Some(serde_json::json!({"cancelled": true})));
            } else {
                output::info("Cancelled.", None);
            }
            process::exit(0);
        }
        ConfirmOutcome::Refused { suggestion } => {
            crate::confirm::exit_refused(
                &format!(
                    "Refusing to reset preview database branch '{name}' on {app_name}/{preview_slug} without explicit confirmation; dev/prod untouched."
                ),
                &suggestion,
            );
        }
    }

    let branch = match client.reset_preview_database_branch(&app_id, &preview_slug, name) {
        Ok(branch) => branch,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let risk: RiskMetadata = Tier::Two.into();
    if output::is_json_mode() {
        output::success(
            "Preview database branch reset.",
            Some(serde_json::json!({
                "app": {
                    "id": app_id,
                    "name": app_name,
                },
                "preview": {
                    "slug": preview_slug,
                    "environment_name": "preview",
                },
                "database_branch": output::to_value(&branch),
                "destructive": risk.destructive,
                "data_loss": risk.data_loss,
                "tier": risk.tier,
                "scope": "preview",
                "dev_prod_untouched": true,
            })),
        );
        return;
    }

    output::success(
        &format!(
            "Reset preview database branch {name} for {app_name}/{preview_slug}. Dev and prod were untouched."
        ),
        None,
    );
    render_branch_detail(&branch);
}

fn resolve_postgres_service(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    app_name: &str,
    name: &str,
) -> ManagedServiceSummary {
    let listing = match client.list_managed_services(app_id) {
        Ok(response) => response,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };
    let service = listing
        .managed_services
        .into_iter()
        .find(|service| service.service_type == "postgres" && service.name == name);

    match service {
        Some(service) => service,
        None => {
            output::error(
                &format!("No managed Postgres service named '{name}' on {app_name}."),
                &ErrorCode::ManagedServiceNotFound,
                Some("Run 'floo services list' to see managed Postgres services."),
            );
            process::exit(1);
        }
    }
}

fn resolve_preview_slug(
    client: &crate::api_client::FlooClient,
    app_id: &str,
    app_name: &str,
    preview_identifier: &str,
) -> String {
    if let Some(slug) = normalize_preview_identifier(preview_identifier) {
        return slug;
    }

    let listing = match client.list_previews(app_id) {
        Ok(response) => response,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let matches: Vec<&PreviewEnvironment> = listing
        .previews
        .iter()
        .filter(|preview| {
            preview.slug == preview_identifier
                || preview.source_branch.as_deref() == Some(preview_identifier)
                || preview
                    .pr_number
                    .map(|pr_number| format!("#{pr_number}") == preview_identifier)
                    .unwrap_or(false)
        })
        .collect();

    match matches.as_slice() {
        [preview] => preview.slug.clone(),
        [] => {
            output::error(
                &format!("Preview '{preview_identifier}' not found on {app_name}."),
                &ErrorCode::Other("PREVIEW_NOT_FOUND".to_string()),
                Some("Pass the preview slug from the preview URL, or run the list command with a known slug."),
            );
            process::exit(1);
        }
        _ => {
            output::error(
                &format!("Preview identifier '{preview_identifier}' is ambiguous on {app_name}."),
                &ErrorCode::Other("AMBIGUOUS_PREVIEW_IDENTIFIER".to_string()),
                Some("Pass the exact preview slug, e.g. the part after '-preview-' in the preview URL."),
            );
            process::exit(1);
        }
    }
}

fn normalize_preview_identifier(identifier: &str) -> Option<String> {
    let trimmed = identifier.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    if let Some(prefix_start) = host.find("-preview-") {
        let after_prefix = &host[prefix_start + "-preview-".len()..];
        let label = after_prefix.split('.').next().unwrap_or(after_prefix);
        return Some(label.to_string());
    }
    if trimmed.contains("://")
        || trimmed.contains(".")
        || trimmed.contains('/')
        || trimmed.starts_with('#')
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn preview_json(preview: &PreviewEnvironment) -> serde_json::Value {
    serde_json::json!({
        "id": preview.id,
        "slug": preview.slug,
        "environment_name": "preview",
        "source_branch": preview.source_branch,
        "pr_number": preview.pr_number,
        "url": preview.url,
        "latest_deploy_id": preview.latest_deploy_id,
        "latest_deploy_status": preview.latest_deploy_status,
        "latest_commit_sha": preview.latest_commit_sha,
        "ttl_hours": preview.ttl_hours,
        "expires_at": preview.expires_at,
        "resources": output::to_value(&preview.resources),
    })
}

fn render_preview_context(preview: &PreviewEnvironment) {
    if let Some(source_branch) = &preview.source_branch {
        output::info(&format!("  Source:  {source_branch}"), None);
    }
    if let Some(url) = &preview.url {
        output::info(&format!("  URL:     {url}"), None);
    }
    if let Some(expires_at) = &preview.expires_at {
        output::info(&format!("  Expires: {expires_at}"), None);
    }
}

fn render_branch_table(branches: &[PreviewDatabaseBranch]) {
    let rows: Vec<Vec<String>> = branches
        .iter()
        .map(|branch| {
            vec![
                branch.name.clone(),
                branch.source_environment.clone(),
                branch.resource_status.clone(),
                branch.hydration_mode.clone(),
                branch
                    .schema_name
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                branch.expires_at.clone().unwrap_or_else(|| "-".to_string()),
                reset_status(branch),
            ]
        })
        .collect();
    output::table(
        &[
            "Name",
            "Source",
            "Status",
            "Hydration",
            "Schema",
            "Expires",
            "Reset",
        ],
        &rows,
        None,
    );
}

fn render_branch_detail(branch: &PreviewDatabaseBranch) {
    output::info(&format!("  Name:      {}", branch.name), None);
    output::info(&format!("  Preview:   {}", branch.preview_slug), None);
    output::info(&format!("  Source:    {}", branch.source_environment), None);
    output::info(&format!("  Status:    {}", branch.resource_status), None);
    output::info(&format!("  Hydration: {}", branch.hydration_mode), None);
    output::info(
        &format!(
            "  Schema:    {}",
            branch.schema_name.as_deref().unwrap_or("-")
        ),
        None,
    );
    output::info(
        &format!(
            "  Role:      {}",
            branch.role_name.as_deref().unwrap_or("-")
        ),
        None,
    );
    output::info(
        &format!(
            "  Expires:   {}",
            branch.expires_at.as_deref().unwrap_or("-")
        ),
        None,
    );
    output::info(&format!("  Reset:     {}", reset_status(branch)), None);
}

fn reset_status(branch: &PreviewDatabaseBranch) -> String {
    if branch.reset_eligible {
        "eligible".to_string()
    } else {
        branch
            .reset_blocked_reason
            .clone()
            .unwrap_or_else(|| "blocked".to_string())
    }
}

fn render_backup(backup: &ManagedPostgresBackup) {
    output::info(&format!("  Backup ID: {}", backup.id), None);
    output::info(&format!("  Env: {}", backup.env), None);
    output::info(&format!("  Size: {}", backup.size_human), None);
    output::info(&format!("  Expires: {}", backup.expires_at), None);
}

fn render_restore(
    response: &ManagedPostgresRestoreResponse,
    app_name: &str,
    service_name: &str,
    env: &str,
) {
    output::success(
        &format!("Restored Postgres backup on {app_name} (postgres:{service_name}, env={env})."),
        None,
    );
    output::info(&format!("  Backup ID: {}", response.backup.id), None);
    output::info(&format!("  Restored at: {}", response.restored_at), None);
    output::info(&format!("  Size: {}", response.backup.size_human), None);
}

/// Detect SQL that explicitly filters on `table_schema = 'public'` (or
/// `schemaname = 'public'`, the pg_catalog spelling): the canonical
/// "list my tables" pattern that returns empty against a floo app
/// because tables live in a namespaced schema. Used to gate a one-line
/// hint after a 0-row result.
fn queries_public_schema(sql: &str) -> bool {
    let lower: String = sql
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    lower.contains("table_schema='public'")
        || lower.contains("table_schema=\"public\"")
        || lower.contains("schemaname='public'")
        || lower.contains("schemaname=\"public\"")
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
    fn test_validate_query_args_empty_check_precedes_limit_check() {
        // An empty query with a bad limit reports the empty-query error first —
        // the SQL is the more fundamental problem to surface.
        let err = validate_query_args("", 0).unwrap_err();
        assert_eq!(err.code.as_str(), "EMPTY_QUERY");
    }

    #[test]
    fn test_queries_public_schema_information_schema() {
        assert!(queries_public_schema(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'"
        ));
    }

    #[test]
    fn test_queries_public_schema_double_quotes() {
        assert!(queries_public_schema(
            "SELECT * FROM information_schema.tables WHERE table_schema = \"public\""
        ));
    }

    #[test]
    fn test_queries_public_schema_pg_stat() {
        assert!(queries_public_schema(
            "SELECT * FROM pg_stat_user_tables WHERE schemaname = 'public'"
        ));
    }

    #[test]
    fn test_queries_public_schema_qualified_table() {
        // `public.users` is just an unqualified-from-schema-pov reference,
        // not the introspection pattern we're flagging. A user explicitly
        // querying public schema tables likely knows what they're doing.
        assert!(!queries_public_schema("SELECT * FROM public.users"));
    }

    #[test]
    fn test_queries_public_schema_unrelated() {
        assert!(!queries_public_schema("SELECT * FROM users"));
    }

    #[test]
    fn test_queries_public_schema_case_insensitive() {
        assert!(queries_public_schema(
            "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = 'public'"
        ));
    }
}
