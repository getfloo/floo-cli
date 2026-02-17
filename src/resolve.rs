use serde_json::Value;

use crate::api_client::FlooClient;
use crate::errors::FlooApiError;

trait AppResolverClient {
    fn get_app(&self, app_id: &str) -> Result<Value, FlooApiError>;
    fn list_apps(&self, page: u32, per_page: u32) -> Result<Value, FlooApiError>;
}

impl AppResolverClient for FlooClient {
    fn get_app(&self, app_id: &str) -> Result<Value, FlooApiError> {
        FlooClient::get_app(self, app_id)
    }

    fn list_apps(&self, page: u32, per_page: u32) -> Result<Value, FlooApiError> {
        FlooClient::list_apps(self, page, per_page)
    }
}

fn is_uuid_identifier(identifier: &str) -> bool {
    if identifier.len() != 36 {
        return false;
    }

    for (idx, ch) in identifier.chars().enumerate() {
        match idx {
            8 | 13 | 18 | 23 => {
                if ch != '-' {
                    return false;
                }
            }
            _ => {
                if !ch.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }

    true
}

fn resolve_app_with_client<C: AppResolverClient>(client: &C, identifier: &str) -> Option<Value> {
    // Only hit the /apps/{id} endpoint for UUID identifiers.
    if is_uuid_identifier(identifier) {
        if let Ok(app) = client.get_app(identifier) {
            return Some(app);
        }
    }

    // Fall back to name match via list.
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

pub fn resolve_app(client: &FlooClient, identifier: &str) -> Option<Value> {
    resolve_app_with_client(client, identifier)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use serde_json::json;

    use super::*;

    struct FakeClient {
        app_lookup_success: bool,
        app_response: Value,
        list_response: Value,
        app_lookup_calls: RefCell<Vec<String>>,
        list_lookup_calls: Cell<u32>,
    }

    impl FakeClient {
        fn new(app_lookup_success: bool, app_response: Value, list_response: Value) -> Self {
            Self {
                app_lookup_success,
                app_response,
                list_response,
                app_lookup_calls: RefCell::new(Vec::new()),
                list_lookup_calls: Cell::new(0),
            }
        }
    }

    impl AppResolverClient for FakeClient {
        fn get_app(&self, app_id: &str) -> Result<Value, FlooApiError> {
            self.app_lookup_calls.borrow_mut().push(app_id.to_string());
            if self.app_lookup_success {
                Ok(self.app_response.clone())
            } else {
                Err(FlooApiError::new(404, "APP_NOT_FOUND", "App not found."))
            }
        }

        fn list_apps(&self, _page: u32, _per_page: u32) -> Result<Value, FlooApiError> {
            self.list_lookup_calls.set(self.list_lookup_calls.get() + 1);
            Ok(self.list_response.clone())
        }
    }

    #[test]
    fn non_uuid_identifier_skips_id_endpoint_lookup() {
        let client = FakeClient::new(
            false,
            json!({"id":"11111111-1111-1111-1111-111111111111","name":"unused"}),
            json!({"apps":[{"id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","name":"my-app"}]}),
        );

        let resolved = resolve_app_with_client(&client, "my-app");

        assert!(resolved.is_some());
        assert!(client.app_lookup_calls.borrow().is_empty());
        assert_eq!(client.list_lookup_calls.get(), 1);
    }

    #[test]
    fn uuid_identifier_uses_id_endpoint_first() {
        let app_id = "11111111-1111-1111-1111-111111111111";
        let client = FakeClient::new(
            true,
            json!({"id":app_id,"name":"uuid-app"}),
            json!({"apps":[{"id":"bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb","name":"fallback-app"}]}),
        );

        let resolved = resolve_app_with_client(&client, app_id);

        assert!(resolved.is_some());
        assert_eq!(
            client.app_lookup_calls.borrow().as_slice(),
            &[app_id.to_string()]
        );
        assert_eq!(client.list_lookup_calls.get(), 0);
    }

    #[test]
    fn uuid_identifier_falls_back_to_name_lookup_when_id_lookup_fails() {
        let identifier = "22222222-2222-2222-2222-222222222222";
        let client = FakeClient::new(
            false,
            json!({"id":"unused","name":"unused"}),
            json!({"apps":[{"id":identifier,"name":identifier}]}),
        );

        let resolved = resolve_app_with_client(&client, identifier);

        assert!(resolved.is_some());
        assert_eq!(
            client.app_lookup_calls.borrow().as_slice(),
            &[identifier.to_string()]
        );
        assert_eq!(client.list_lookup_calls.get(), 1);
    }
}
