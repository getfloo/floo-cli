use serde_json::Value;

use crate::api_client::FlooClient;

pub fn resolve_app(client: &FlooClient, identifier: &str) -> Option<Value> {
    // Try UUID lookup first
    if let Ok(app) = client.get_app(identifier) {
        return Some(app);
    }

    // Fall back to name match via list
    if let Ok(resp) = client.list_apps(1, 100) {
        if let Some(apps) = resp.get("apps").and_then(|v| v.as_array()) {
            for app in apps {
                if app.get("name").and_then(|v| v.as_str()) == Some(identifier) {
                    return Some(app.clone());
                }
            }
        }
    }

    None
}
