use std::process;

use crate::api_client::FlooClient;
use crate::config::load_config;
use crate::output;
use crate::resolve::resolve_app;

fn require_auth() {
    let config = load_config();
    if config.api_key.is_none() {
        output::error(
            "Not logged in.",
            "NOT_AUTHENTICATED",
            Some("Run 'floo login' to authenticate."),
        );
        process::exit(1);
    }
}

pub fn create(app_name: &str, db_name: &str) {
    require_auth();
    let client = FlooClient::new(None);
    let app_data = match resolve_app(&client, app_name) {
        Some(a) => a,
        None => {
            output::error(
                &format!("App '{app_name}' not found."),
                "APP_NOT_FOUND",
                Some("Check the app name or ID and try again."),
            );
            process::exit(1);
        }
    };

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    let spinner = output::Spinner::new(&format!("Provisioning database '{db_name}' for {name}..."));

    match client.create_database(app_id, db_name) {
        Ok(result) => {
            spinner.finish();
            let connection_url = result
                .get("connection_url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let schema_name = result
                .get("schema_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let username = result
                .get("username")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            output::success(
                &format!("Database '{db_name}' provisioned for {name}."),
                Some(result.clone()),
            );

            if !output::is_json_mode() {
                output::dim_line(&format!("Schema:   {schema_name}"));
                output::dim_line(&format!("Username: {username}"));
                output::dim_line(&format!("URL:      {connection_url}"));
                output::dim_line("");
                output::dim_line("DATABASE_URL has been set automatically.");
                output::dim_line("Your next deploy will pick it up.");
            }
        }
        Err(e) => {
            spinner.finish();
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    }
}

pub fn info(app_name: &str) {
    require_auth();
    let client = FlooClient::new(None);
    let app_data = match resolve_app(&client, app_name) {
        Some(a) => a,
        None => {
            output::error(
                &format!("App '{app_name}' not found."),
                "APP_NOT_FOUND",
                Some("Check the app name or ID and try again."),
            );
            process::exit(1);
        }
    };

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    let result = match client.list_databases(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let databases = result
        .get("databases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if databases.is_empty() {
        if !output::is_json_mode() {
            output::info(
                &format!(
                    "No databases provisioned for {name}. Create one with floo db create --app {name}."
                ),
                None,
            );
        } else {
            output::success("No databases.", Some(serde_json::json!({"databases": []})));
        }
        return;
    }

    // For each database, get the full connection details
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut full_details = Vec::new();

    for db in &databases {
        let db_id = db.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let db_name = db
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string();
        let status = db
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string();
        let schema = db
            .get("schema_name")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string();
        let username = db
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string();

        rows.push(vec![db_name, status, schema, username]);

        // Get full details with connection URL for JSON mode
        if output::is_json_mode() {
            if let Ok(detail) = client.get_database(app_id, db_id) {
                full_details.push(detail);
            }
        }
    }

    if output::is_json_mode() {
        output::success(
            "Databases listed.",
            Some(serde_json::json!({"databases": full_details})),
        );
    } else {
        output::table(
            &["Name", "Status", "Schema", "Username"],
            &rows,
            Some(serde_json::json!({"databases": databases})),
        );
    }
}

pub fn delete(app_name: &str, db_name: &str, force: bool) {
    require_auth();
    let client = FlooClient::new(None);
    let app_data = match resolve_app(&client, app_name) {
        Some(a) => a,
        None => {
            output::error(
                &format!("App '{app_name}' not found."),
                "APP_NOT_FOUND",
                Some("Check the app name or ID and try again."),
            );
            process::exit(1);
        }
    };

    let app_id = app_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = app_data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(app_name);

    // Find the database by name via list
    let list_result = match client.list_databases(app_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let databases = list_result
        .get("databases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let db_entry = databases
        .iter()
        .find(|d| d.get("name").and_then(|v| v.as_str()) == Some(db_name));

    let db_id = match db_entry {
        Some(d) => d
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        None => {
            output::error(
                &format!("Database '{db_name}' not found on {name}."),
                "DATABASE_NOT_FOUND",
                Some(&format!("Check with floo db info --app {name}.")),
            );
            process::exit(1);
        }
    };

    if !force
        && !output::confirm(&format!(
            "Delete database '{db_name}' from {name}? This will drop all data."
        ))
    {
        if !output::is_json_mode() {
            output::info("Cancelled.", None);
        }
        return;
    }

    let spinner = output::Spinner::new(&format!("Deprovisioning database '{db_name}'..."));

    if let Err(e) = client.delete_database(app_id, &db_id) {
        spinner.finish();
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }

    spinner.finish();
    output::success(
        &format!("Database '{db_name}' deleted from {name}."),
        Some(serde_json::json!({"name": db_name})),
    );
}
