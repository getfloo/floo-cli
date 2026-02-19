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

fn match_app_from_list_response(
    list_response: &Value,
    identifier: &str,
) -> Result<Value, FlooApiError> {
    let apps = list_response
        .get("apps")
        .and_then(|value| value.as_array())
        .ok_or_else(|| {
            FlooApiError::new(
                500,
                "PARSE_ERROR",
                "Failed to parse app list response from API.",
            )
        })?;

    for app in apps {
        if app.get("name").and_then(|value| value.as_str()) == Some(identifier) {
            return Ok(app.clone());
        }
    }

    Err(FlooApiError::new(404, "APP_NOT_FOUND", "App not found."))
}

fn resolve_app_with_client<C: AppResolverClient>(
    client: &C,
    identifier: &str,
) -> Result<Value, FlooApiError> {
    // Only hit the /apps/{id} endpoint for UUID identifiers.
    if is_uuid_identifier(identifier) {
        return client.get_app(identifier);
    }

    // Name lookup via list for non-UUID identifiers.
    let response = client.list_apps(1, 100)?;
    match_app_from_list_response(&response, identifier)
}

pub fn resolve_app(client: &FlooClient, identifier: &str) -> Result<Value, FlooApiError> {
    resolve_app_with_client(client, identifier)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use serde_json::json;

    use super::*;

    struct FakeClient {
        app_lookup_result: Result<Value, (u16, String, String)>,
        list_lookup_result: Result<Value, (u16, String, String)>,
        app_lookup_calls: RefCell<Vec<String>>,
        list_lookup_calls: Cell<u32>,
    }

    impl FakeClient {
        fn new(
            app_lookup_result: Result<Value, (u16, String, String)>,
            list_lookup_result: Result<Value, (u16, String, String)>,
        ) -> Self {
            Self {
                app_lookup_result,
                list_lookup_result,
                app_lookup_calls: RefCell::new(Vec::new()),
                list_lookup_calls: Cell::new(0),
            }
        }

        fn ok_app(app: Value) -> Result<Value, (u16, String, String)> {
            Ok(app)
        }

        fn err(
            status_code: u16,
            code: &str,
            message: &str,
        ) -> Result<Value, (u16, String, String)> {
            Err((status_code, code.to_string(), message.to_string()))
        }
    }

    impl AppResolverClient for FakeClient {
        fn get_app(&self, app_id: &str) -> Result<Value, FlooApiError> {
            self.app_lookup_calls.borrow_mut().push(app_id.to_string());
            match &self.app_lookup_result {
                Ok(value) => Ok(value.clone()),
                Err((status_code, code, message)) => Err(FlooApiError::new(
                    *status_code,
                    code.clone(),
                    message.clone(),
                )),
            }
        }

        fn list_apps(&self, _page: u32, _per_page: u32) -> Result<Value, FlooApiError> {
            self.list_lookup_calls.set(self.list_lookup_calls.get() + 1);
            match &self.list_lookup_result {
                Ok(value) => Ok(value.clone()),
                Err((status_code, code, message)) => Err(FlooApiError::new(
                    *status_code,
                    code.clone(),
                    message.clone(),
                )),
            }
        }
    }

    #[test]
    fn non_uuid_identifier_skips_id_endpoint_lookup() {
        let client = FakeClient::new(
            FakeClient::err(404, "APP_NOT_FOUND", "App not found."),
            FakeClient::ok_app(
                json!({"apps":[{"id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","name":"my-app"}]}),
            ),
        );

        let resolved = resolve_app_with_client(&client, "my-app");

        assert!(resolved.is_ok());
        assert!(client.app_lookup_calls.borrow().is_empty());
        assert_eq!(client.list_lookup_calls.get(), 1);
    }

    #[test]
    fn uuid_identifier_uses_id_endpoint_first() {
        let app_id = "11111111-1111-1111-1111-111111111111";
        let client = FakeClient::new(
            FakeClient::ok_app(json!({"id":app_id,"name":"uuid-app"})),
            FakeClient::ok_app(
                json!({"apps":[{"id":"bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb","name":"fallback-app"}]}),
            ),
        );

        let resolved = resolve_app_with_client(&client, app_id);

        assert!(resolved.is_ok());
        assert_eq!(
            client.app_lookup_calls.borrow().as_slice(),
            &[app_id.to_string()]
        );
        assert_eq!(client.list_lookup_calls.get(), 0);
    }

    #[test]
    fn uuid_identifier_does_not_fall_back_when_id_lookup_fails() {
        let identifier = "22222222-2222-2222-2222-222222222222";
        let client = FakeClient::new(
            FakeClient::err(404, "APP_NOT_FOUND", "App not found."),
            FakeClient::ok_app(json!({"apps":[{"id":identifier,"name":identifier}]})),
        );

        let error = resolve_app_with_client(&client, identifier).unwrap_err();

        assert_eq!(error.status_code, 404);
        assert_eq!(error.code, "APP_NOT_FOUND");
        assert_eq!(
            client.app_lookup_calls.borrow().as_slice(),
            &[identifier.to_string()]
        );
        assert_eq!(client.list_lookup_calls.get(), 0);
    }

    #[test]
    fn uuid_identifier_propagates_non_404_lookup_errors() {
        let identifier = "33333333-3333-3333-3333-333333333333";
        let client = FakeClient::new(
            FakeClient::err(500, "INTERNAL_ERROR", "Server error"),
            FakeClient::ok_app(json!({"apps":[{"id":identifier,"name":identifier}]})),
        );

        let error = resolve_app_with_client(&client, identifier).unwrap_err();

        assert_eq!(error.status_code, 500);
        assert_eq!(error.code, "INTERNAL_ERROR");
        assert_eq!(client.list_lookup_calls.get(), 0);
    }

    #[test]
    fn uuid_identifier_propagates_validation_errors_without_fallback() {
        let identifier = "44444444-4444-4444-4444-444444444444";
        let client = FakeClient::new(
            FakeClient::err(422, "VALIDATION_ERROR", "Invalid UUID"),
            FakeClient::ok_app(json!({"apps":[{"id":identifier,"name":"uuid-app"}]})),
        );

        let error = resolve_app_with_client(&client, identifier).unwrap_err();

        assert_eq!(error.status_code, 422);
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert_eq!(client.list_lookup_calls.get(), 0);
    }

    #[test]
    fn non_uuid_identifier_propagates_list_errors() {
        let client = FakeClient::new(
            FakeClient::err(404, "APP_NOT_FOUND", "App not found."),
            FakeClient::err(401, "UNAUTHORIZED", "Unauthorized"),
        );

        let error = resolve_app_with_client(&client, "my-app").unwrap_err();

        assert_eq!(error.status_code, 401);
        assert_eq!(error.code, "UNAUTHORIZED");
    }

    #[test]
    fn returns_app_not_found_when_name_lookup_misses() {
        let client = FakeClient::new(
            FakeClient::err(404, "APP_NOT_FOUND", "App not found."),
            FakeClient::ok_app(json!({"apps":[{"id":"id-1","name":"other-app"}]})),
        );

        let error = resolve_app_with_client(&client, "my-app").unwrap_err();

        assert_eq!(error.status_code, 404);
        assert_eq!(error.code, "APP_NOT_FOUND");
    }

    #[test]
    fn returns_parse_error_when_list_response_is_malformed() {
        let client = FakeClient::new(
            FakeClient::err(404, "APP_NOT_FOUND", "App not found."),
            FakeClient::ok_app(json!({"unexpected":"payload"})),
        );

        let error = resolve_app_with_client(&client, "my-app").unwrap_err();

        assert_eq!(error.status_code, 500);
        assert_eq!(error.code, "PARSE_ERROR");
    }
}
