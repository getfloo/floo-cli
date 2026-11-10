use crate::api_client::FlooClient;
use crate::api_types::{App, ListAppsResponse};
use crate::errors::FlooApiError;

trait AppResolverClient {
    fn get_app(&self, app_id: &str) -> Result<App, FlooApiError>;
    fn list_apps(&self, page: u32, per_page: u32) -> Result<ListAppsResponse, FlooApiError>;
}

impl AppResolverClient for FlooClient {
    fn get_app(&self, app_id: &str) -> Result<App, FlooApiError> {
        FlooClient::get_app(self, app_id)
    }

    fn list_apps(&self, page: u32, per_page: u32) -> Result<ListAppsResponse, FlooApiError> {
        FlooClient::list_apps(self, page, per_page)
    }
}

pub(crate) fn is_uuid_identifier(identifier: &str) -> bool {
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

/// The API's largest app page.
const APP_PAGE_SIZE: u32 = 100;

fn resolve_app_with_client<C: AppResolverClient>(
    client: &C,
    identifier: &str,
) -> Result<App, FlooApiError> {
    // Only hit the /apps/{id} endpoint for UUID identifiers.
    if is_uuid_identifier(identifier) {
        return client.get_app(identifier);
    }

    // A name is looked up in the org's app list, page by page until the list ends.
    for page in 1.. {
        let apps = client.list_apps(page, APP_PAGE_SIZE)?.apps;
        let last_page = apps.len() < APP_PAGE_SIZE as usize;
        if let Some(app) = apps.into_iter().find(|app| app.name == identifier) {
            return Ok(app);
        }
        if last_page {
            break;
        }
    }
    Err(FlooApiError::new(404, "APP_NOT_FOUND", "App not found."))
}

pub fn resolve_app(client: &FlooClient, identifier: &str) -> Result<App, FlooApiError> {
    resolve_app_with_client(client, identifier)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    struct FakeClient {
        app_lookup_result: Result<App, (u16, String, String)>,
        list_lookup_result: Result<ListAppsResponse, (u16, String, String)>,
        app_lookup_calls: RefCell<Vec<String>>,
        list_lookup_calls: Cell<u32>,
    }

    impl FakeClient {
        fn new(
            app_lookup_result: Result<App, (u16, String, String)>,
            list_lookup_result: Result<ListAppsResponse, (u16, String, String)>,
        ) -> Self {
            Self {
                app_lookup_result,
                list_lookup_result,
                app_lookup_calls: RefCell::new(Vec::new()),
                list_lookup_calls: Cell::new(0),
            }
        }

        fn ok_app(app: App) -> Result<App, (u16, String, String)> {
            Ok(app)
        }

        fn ok_list(response: ListAppsResponse) -> Result<ListAppsResponse, (u16, String, String)> {
            Ok(response)
        }

        fn err_app(
            status_code: u16,
            code: &str,
            message: &str,
        ) -> Result<App, (u16, String, String)> {
            Err((status_code, code.to_string(), message.to_string()))
        }

        fn err_list(
            status_code: u16,
            code: &str,
            message: &str,
        ) -> Result<ListAppsResponse, (u16, String, String)> {
            Err((status_code, code.to_string(), message.to_string()))
        }
    }

    impl AppResolverClient for FakeClient {
        fn get_app(&self, app_id: &str) -> Result<App, FlooApiError> {
            self.app_lookup_calls.borrow_mut().push(app_id.to_string());
            match &self.app_lookup_result {
                Ok(app) => Ok(app.clone()),
                Err((status_code, code, message)) => Err(FlooApiError::new(
                    *status_code,
                    code.clone(),
                    message.clone(),
                )),
            }
        }

        fn list_apps(&self, _page: u32, _per_page: u32) -> Result<ListAppsResponse, FlooApiError> {
            self.list_lookup_calls.set(self.list_lookup_calls.get() + 1);
            match &self.list_lookup_result {
                Ok(response) => Ok(response.clone()),
                Err((status_code, code, message)) => Err(FlooApiError::new(
                    *status_code,
                    code.clone(),
                    message.clone(),
                )),
            }
        }
    }

    fn make_app(id: &str, name: &str) -> App {
        App {
            lifecycle_state: None,
            id: id.to_string(),
            name: name.to_string(),
            org_id: None,
            status: None,
            url: None,
            runtime_url: None,
            created_at: None,
            envs: vec![],
            source: None,
        }
    }

    fn make_list(apps: Vec<App>) -> ListAppsResponse {
        ListAppsResponse { apps, total: None }
    }

    #[test]
    fn non_uuid_identifier_skips_id_endpoint_lookup() {
        let client = FakeClient::new(
            FakeClient::err_app(404, "APP_NOT_FOUND", "App not found."),
            FakeClient::ok_list(make_list(vec![make_app(
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "my-app",
            )])),
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
            FakeClient::ok_app(make_app(app_id, "uuid-app")),
            FakeClient::ok_list(make_list(vec![make_app(
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                "fallback-app",
            )])),
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
            FakeClient::err_app(404, "APP_NOT_FOUND", "App not found."),
            FakeClient::ok_list(make_list(vec![make_app(identifier, identifier)])),
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
            FakeClient::err_app(500, "INTERNAL_ERROR", "Server error"),
            FakeClient::ok_list(make_list(vec![make_app(identifier, identifier)])),
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
            FakeClient::err_app(422, "VALIDATION_ERROR", "Invalid UUID"),
            FakeClient::ok_list(make_list(vec![make_app(identifier, "uuid-app")])),
        );

        let error = resolve_app_with_client(&client, identifier).unwrap_err();

        assert_eq!(error.status_code, 422);
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert_eq!(client.list_lookup_calls.get(), 0);
    }

    #[test]
    fn non_uuid_identifier_propagates_list_errors() {
        let client = FakeClient::new(
            FakeClient::err_app(404, "APP_NOT_FOUND", "App not found."),
            FakeClient::err_list(401, "UNAUTHORIZED", "Unauthorized"),
        );

        let error = resolve_app_with_client(&client, "my-app").unwrap_err();

        assert_eq!(error.status_code, 401);
        assert_eq!(error.code, "UNAUTHORIZED");
    }

    #[test]
    fn returns_app_not_found_when_name_lookup_misses() {
        crate::output::set_json_mode(false);
        crate::output::set_dry_run_mode(false);
        let client = FakeClient::new(
            FakeClient::err_app(404, "APP_NOT_FOUND", "App not found."),
            FakeClient::ok_list(make_list(vec![make_app("id-1", "other-app")])),
        );

        let error = resolve_app_with_client(&client, "my-app").unwrap_err();

        assert_eq!(error.status_code, 404);
        assert_eq!(error.code, "APP_NOT_FOUND");
        assert_eq!(error.message, "App not found.");
        assert!(error.extra.is_none());
    }
    /// Serves its apps in pages, like the API.
    struct PagedClient(Vec<App>);

    impl AppResolverClient for PagedClient {
        fn get_app(&self, _app_id: &str) -> Result<App, FlooApiError> {
            Err(FlooApiError::new(404, "APP_NOT_FOUND", "App not found."))
        }

        fn list_apps(&self, page: u32, per_page: u32) -> Result<ListAppsResponse, FlooApiError> {
            let apps = self
                .0
                .chunks(per_page as usize)
                .nth(page as usize - 1)
                .map(<[App]>::to_vec)
                .unwrap_or_default();
            Ok(make_list(apps))
        }
    }

    #[test]
    fn name_lookup_reads_past_the_first_page_and_stops_at_the_end() {
        let apps: Vec<App> = (0..APP_PAGE_SIZE + 1)
            .map(|n| make_app(&format!("id-{n}"), &format!("app-{n}")))
            .collect();
        let client = PagedClient(apps);

        let found = resolve_app_with_client(&client, "app-100").unwrap();
        assert_eq!(found.id, "id-100");

        let error = resolve_app_with_client(&client, "missing").unwrap_err();
        assert_eq!(error.code, "APP_NOT_FOUND");

        // A list that ends exactly on a full page still ends.
        let full_page = PagedClient(client.0[..APP_PAGE_SIZE as usize].to_vec());
        let error = resolve_app_with_client(&full_page, "missing").unwrap_err();
        assert_eq!(error.code, "APP_NOT_FOUND");
    }
}
