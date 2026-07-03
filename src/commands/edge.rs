use std::process;

use crate::api_types::EdgeRoute;
use crate::errors::ErrorCode;
use crate::output;

pub fn list_routes(app: Option<&str>, env: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);
    let (app_id, app_name) = super::resolve_app_from_config(&client, app);

    let response = match client.list_edge_routes(&app_id, env) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            &format!("Edge routes for {app_name}."),
            Some(output::to_value(&response)),
        );
        return;
    }

    if response.routes.is_empty() {
        let scope = env.map(|value| format!(" in {value}")).unwrap_or_default();
        output::info(&format!("No edge routes for {app_name}{scope}."), None);
        return;
    }

    let rows: Vec<Vec<String>> = response.routes.iter().map(route_row).collect();
    output::table(
        &[
            "Host", "Path", "Env", "Service", "Policy", "Source", "Updated",
        ],
        &rows,
        Some(output::to_value(&response)),
    );
}

fn route_row(route: &EdgeRoute) -> Vec<String> {
    vec![
        route.host.clone(),
        route.path_prefix.clone(),
        route
            .environment_slug
            .as_deref()
            .or(route.environment_name.as_deref())
            .unwrap_or("unscoped")
            .to_string(),
        service_label(route),
        policy_label(route),
        route.source.clone(),
        route.updated_at.clone(),
    ]
}

fn service_label(route: &EdgeRoute) -> String {
    match (route.service_name.as_deref(), route.service_type.as_deref()) {
        (Some(name), Some(service_type)) => format!("{name} ({service_type})"),
        (Some(name), None) => name.to_string(),
        _ => "app".to_string(),
    }
}

fn policy_label(route: &EdgeRoute) -> String {
    match (route.api_key_enabled, route.required_scope.as_deref()) {
        (true, Some(scope)) => format!("{} + key:{scope}", route.access_mode),
        (true, None) => format!("{} + key", route.access_mode),
        (false, Some(scope)) => format!("{}:{scope}", route.access_mode),
        (false, None) => route.access_mode.clone(),
    }
}
