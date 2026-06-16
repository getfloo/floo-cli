use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::Value;

use crate::api_types::*;
use crate::config::{load_config, FlooConfig};
use crate::errors::FlooApiError;
use crate::project_config::ServiceConfig;

pub struct FlooClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    default_org: Option<String>,
}

impl FlooClient {
    pub fn new(config: Option<FlooConfig>) -> Result<Self, FlooApiError> {
        let config = config.unwrap_or_else(load_config);
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| {
                FlooApiError::new(
                    0,
                    "CLIENT_INIT_FAILED",
                    format!("Failed to initialize HTTP client: {e}"),
                )
            })?;

        Ok(Self {
            client,
            base_url: config.api_url.clone(),
            api_key: config.api_key.clone(),
            default_org: config.default_org.clone(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn auth_header(&self) -> Option<String> {
        self.api_key.as_ref().map(|k| format!("Bearer {k}"))
    }

    /// Apply auth and org headers to a request builder.
    fn apply_headers(
        &self,
        mut req: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        if let Some(org_id) = &self.default_org {
            req = req.header("X-Floo-Org-Id", org_id);
        }
        req
    }

    fn handle_error(&self, response: reqwest::blocking::Response) -> FlooApiError {
        let status = response.status().as_u16();
        let text = response.text().unwrap_or_default();
        let (code, message, extra) = if let Ok(body) = serde_json::from_str::<Value>(&text) {
            let detail = body.get("detail").unwrap_or(&body);
            if let Some(obj) = detail.as_object() {
                let code = obj
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("API_ERROR")
                    .to_string();
                let message = obj
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&text)
                    .to_string();
                // Capture any extra fields beyond code/message
                let mut extra_obj = obj.clone();
                extra_obj.remove("code");
                extra_obj.remove("message");
                let extra = if extra_obj.is_empty() {
                    None
                } else {
                    Some(Value::Object(extra_obj))
                };
                (code, message, extra)
            } else {
                (
                    "API_ERROR".to_string(),
                    detail.to_string().trim_matches('"').to_string(),
                    None,
                )
            }
        } else {
            ("API_ERROR".to_string(), text, None)
        };
        let mut err = FlooApiError::new(status, code, message);
        err.extra = extra;
        err
    }

    fn handle_response<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::blocking::Response,
    ) -> Result<T, FlooApiError> {
        let status = response.status().as_u16();
        if status >= 400 {
            return Err(self.handle_error(response));
        }
        response.json::<T>().map_err(|e| {
            FlooApiError::new(500, "PARSE_ERROR", format!("Failed to parse response: {e}"))
        })
    }

    fn handle_response_value(
        &self,
        response: reqwest::blocking::Response,
    ) -> Result<Value, FlooApiError> {
        self.handle_response(response)
    }

    fn get(&self, path: &str) -> Result<reqwest::blocking::Response, FlooApiError> {
        let req = self.apply_headers(self.client.get(self.url(path)));
        req.send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))
    }

    fn get_with_query(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<reqwest::blocking::Response, FlooApiError> {
        let req = self.apply_headers(self.client.get(self.url(path)).query(query));
        req.send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))
    }

    fn post_json(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<reqwest::blocking::Response, FlooApiError> {
        let req = self.apply_headers(self.client.post(self.url(path)).json(body));
        req.send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))
    }

    fn patch_json(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<reqwest::blocking::Response, FlooApiError> {
        let req = self.apply_headers(self.client.patch(self.url(path)).json(body));
        req.send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))
    }

    fn delete(&self, path: &str) -> Result<reqwest::blocking::Response, FlooApiError> {
        let req = self.apply_headers(self.client.delete(self.url(path)));
        req.send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))
    }

    // --- Auth ---

    pub fn register(&self, email: &str) -> Result<AuthTokenResponse, FlooApiError> {
        let body = serde_json::json!({"email": email});
        let resp = self.post_json("/v1/auth/register", &body)?;
        self.handle_response(resp)
    }

    pub fn device_authorize(&self) -> Result<DeviceAuthorizeResponse, FlooApiError> {
        let resp = self.post_json("/v1/auth/device", &serde_json::json!({}))?;
        self.handle_response(resp)
    }

    pub fn device_token(&self, device_code: &str) -> Result<AuthTokenResponse, FlooApiError> {
        let body = serde_json::json!({"device_code": device_code});
        let resp = self.post_json("/v1/auth/device/token", &body)?;
        let status = resp.status().as_u16();
        if status == 202 {
            let resp_body: Value = resp.json().map_err(|e| {
                FlooApiError::new(
                    500,
                    "PARSE_ERROR",
                    format!("Failed to parse 202 response: {e}"),
                )
            })?;
            let poll_status = resp_body
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending");
            let code = if poll_status == "slow_down" {
                "DEVICE_SLOW_DOWN"
            } else {
                "DEVICE_PENDING"
            };
            return Err(FlooApiError::new(
                202,
                code,
                format!("Authorization {poll_status}"),
            ));
        }
        self.handle_response(resp)
    }

    pub fn whoami(&self) -> Result<WhoamiResponse, FlooApiError> {
        let resp = self.get("/v1/auth/whoami")?;
        self.handle_response(resp)
    }

    pub fn update_profile(&self, name: &str) -> Result<ProfileResponse, FlooApiError> {
        let body = serde_json::json!({"name": name});
        let resp = self.patch_json("/v1/auth/me", &body)?;
        self.handle_response(resp)
    }

    // --- Org ---

    pub fn list_orgs(&self) -> Result<ListOrgsResponse, FlooApiError> {
        let resp = self.get("/v1/orgs")?;
        self.handle_response(resp)
    }

    pub fn get_org_me(&self) -> Result<OrgResponse, FlooApiError> {
        let resp = self.get("/v1/orgs/me")?;
        self.handle_response(resp)
    }

    pub fn get_org(&self, org_id: &str) -> Result<OrgResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/orgs/{org_id}"))?;
        self.handle_response(resp)
    }

    pub fn list_members(&self, org_id: &str) -> Result<ListMembersResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/orgs/{org_id}/members"))?;
        self.handle_response(resp)
    }

    pub fn update_member_role(
        &self,
        org_id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<MemberRoleResponse, FlooApiError> {
        let body = serde_json::json!({"role": role});
        let resp = self.patch_json(&format!("/v1/orgs/{org_id}/members/{user_id}"), &body)?;
        self.handle_response(resp)
    }

    // --- Access ---

    pub fn grant_app_access(&self, app_id: &str, email: &str) -> Result<Value, FlooApiError> {
        let body = serde_json::json!({"email": email});
        let resp = self.post_json(&format!("/v1/apps/{app_id}/access"), &body)?;
        self.handle_response(resp)
    }

    // --- Billing ---

    pub fn set_spend_cap(&self, spend_cap: u64) -> Result<Value, FlooApiError> {
        let body = serde_json::json!({"spend_cap": spend_cap});
        let resp = self.post_json("/v1/billing/spend-cap", &body)?;
        self.handle_response(resp)
    }

    pub fn get_billing_limits(&self) -> Result<PlanLimitsResponse, FlooApiError> {
        let resp = self.get("/v1/billing/limits")?;
        self.handle_response(resp)
    }

    pub fn create_billing_checkout(
        &self,
        plan: Option<&str>,
    ) -> Result<BillingCheckoutResponse, FlooApiError> {
        let body = match plan {
            Some(p) => serde_json::json!({"plan": p}),
            None => serde_json::json!({}),
        };
        let resp = self.post_json("/v1/billing/checkout", &body)?;
        self.handle_response(resp)
    }

    // --- Apps ---

    pub fn create_app(&self, name: &str, runtime: Option<&str>) -> Result<App, FlooApiError> {
        let mut body = serde_json::json!({"name": name});
        if let Some(rt) = runtime {
            body.as_object_mut()
                .unwrap()
                .insert("runtime".to_string(), Value::String(rt.to_string()));
        }
        let resp = self.post_json("/v1/apps", &body)?;
        self.handle_response(resp)
    }

    pub fn list_apps(&self, page: u32, per_page: u32) -> Result<ListAppsResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/apps?page={page}&per_page={per_page}"))?;
        self.handle_response(resp)
    }

    pub fn get_app(&self, app_id: &str) -> Result<App, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}"))?;
        self.handle_response(resp)
    }

    pub fn get_app_password(&self, app_id: &str) -> Result<AppPasswordResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/password"))?;
        self.handle_response(resp)
    }

    pub fn delete_app(&self, app_id: &str) -> Result<(), FlooApiError> {
        let resp = self.delete(&format!("/v1/apps/{app_id}"))?;
        if resp.status().as_u16() == 204 {
            return Ok(());
        }
        self.handle_response_value(resp)?;
        Ok(())
    }

    // --- Deploys ---

    #[allow(clippy::too_many_arguments)]
    pub fn create_deploy(
        &self,
        app_id: &str,
        runtime: &str,
        framework: Option<&str>,
        services: Option<&[ServiceConfig]>,
        access_mode: Option<&str>,
        agent_mode: Option<&str>,
        auth_redirect_uris: Option<&[String]>,
        reparo_config: Option<&crate::project_config::ReparoConfig>,
        cron_jobs: Option<&[crate::project_config::CronJobEntry]>,
        github_config: Option<&crate::project_config::GitHubConfig>,
        skip_migrations: bool,
    ) -> Result<Deploy, FlooApiError> {
        let mut body = serde_json::json!({
            "runtime": runtime,
        });
        if skip_migrations {
            body["skip_migrations"] = Value::Bool(true);
        }
        if let Some(fw) = framework {
            body["framework"] = Value::String(fw.to_string());
        }
        if let Some(svcs) = services {
            body["services"] = serde_json::to_value(svcs).map_err(|e| {
                FlooApiError::new(
                    0,
                    "SERIALIZATION_ERROR",
                    format!("Failed to serialize services: {e}"),
                )
            })?;
        }
        if let Some(mode) = access_mode {
            body["access_mode"] = Value::String(mode.to_string());
        }
        if let Some(mode) = agent_mode {
            body["agent_mode"] = Value::String(mode.to_string());
        }
        if let Some(uris) = auth_redirect_uris {
            body["auth_redirect_uris"] = serde_json::to_value(uris).map_err(|e| {
                FlooApiError::new(
                    0,
                    "SERIALIZATION_ERROR",
                    format!("Failed to serialize auth_redirect_uris: {e}"),
                )
            })?;
        }
        if let Some(reparo) = reparo_config {
            body["reparo_config"] = serde_json::to_value(reparo).map_err(|e| {
                FlooApiError::new(
                    0,
                    "SERIALIZATION_ERROR",
                    format!("Failed to serialize reparo_config: {e}"),
                )
            })?;
        }
        if let Some(crons) = cron_jobs {
            if !crons.is_empty() {
                body["cron_jobs"] = serde_json::to_value(crons).map_err(|e| {
                    FlooApiError::new(
                        0,
                        "SERIALIZATION_ERROR",
                        format!("Failed to serialize cron_jobs: {e}"),
                    )
                })?;
            }
        }
        if let Some(gh) = github_config {
            if let Some(v) = gh.deploy_on_push {
                body["deploy_on_push"] = Value::Bool(v);
            }
            if let Some(v) = gh.preview_environments {
                body["preview_environments"] = Value::Bool(v);
            }
            if let Some(v) = gh.preview_ttl_hours {
                body["preview_ttl_hours"] = Value::Number(v.into());
            }
        }
        let resp = self.post_json(&format!("/v1/apps/{app_id}/deploys"), &body)?;
        self.handle_response(resp)
    }

    pub fn list_deploys(&self, app_id: &str) -> Result<ListDeploysResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/deploys"))?;
        self.handle_response(resp)
    }

    // --- Doctor ---

    pub fn diagnose_accounts(
        &self,
        app_id: &str,
    ) -> Result<crate::api_types::AccountsDoctorResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/doctor/accounts"))?;
        self.handle_response(resp)
    }

    // --- Cron Jobs ---

    pub fn list_cron_jobs(&self, app_id: &str) -> Result<CronJobListResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/cron"))?;
        self.handle_response(resp)
    }

    pub fn get_cron_job(&self, app_id: &str, name: &str) -> Result<CronJobResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/cron/{name}"))?;
        self.handle_response(resp)
    }

    pub fn run_cron_job(
        &self,
        app_id: &str,
        name: &str,
    ) -> Result<CronJobRunResponse, FlooApiError> {
        let resp = self.post_json(
            &format!("/v1/apps/{app_id}/cron/{name}/run"),
            &serde_json::json!({}),
        )?;
        self.handle_response(resp)
    }

    pub fn get_deploy(&self, app_id: &str, deploy_id: &str) -> Result<Deploy, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/deploys/{deploy_id}"))?;
        self.handle_response(resp)
    }

    pub fn stream_deploy_logs(
        &self,
        app_id: &str,
        deploy_id: &str,
    ) -> Result<reqwest::blocking::Response, FlooApiError> {
        let streaming_client = Client::builder()
            .timeout(Duration::from_secs(1200))
            .build()
            .map_err(|e| {
                FlooApiError::new(
                    0,
                    "CONNECTION_ERROR",
                    format!("Failed to create streaming client: {e}"),
                )
            })?;

        let url = format!(
            "{}/v1/apps/{}/deploys/{}/logs/stream",
            self.base_url, app_id, deploy_id
        );
        let mut req = streaming_client
            .get(&url)
            .header("Accept", "text/event-stream");
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        if let Some(org_id) = &self.default_org {
            req = req.header("X-Floo-Org-Id", org_id);
        }
        let response = req
            .send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))?;

        if response.status().as_u16() == 404 {
            return Err(FlooApiError::new(
                404,
                "NOT_FOUND",
                "Stream endpoint not available",
            ));
        }
        if !response.status().is_success() {
            return Err(FlooApiError::new(
                response.status().as_u16(),
                "STREAM_ERROR",
                format!("Stream endpoint returned {}", response.status()),
            ));
        }
        Ok(response)
    }

    // --- Env vars ---

    pub fn set_env_var(
        &self,
        app_id: &str,
        key: &str,
        value: &str,
        service_id: Option<&str>,
        env: &str,
    ) -> Result<SetEnvVarResponse, FlooApiError> {
        let mut body = serde_json::json!({"key": key, "value": value});
        if let Some(sid) = service_id {
            body.as_object_mut()
                .unwrap()
                .insert("service_id".to_string(), Value::String(sid.to_string()));
        }
        let resp = self.post_json(&format!("/v1/apps/{app_id}/env?env={env}"), &body)?;
        self.handle_response(resp)
    }

    pub fn list_env_vars(
        &self,
        app_id: &str,
        service_id: Option<&str>,
        env: &str,
    ) -> Result<ListEnvVarsResponse, FlooApiError> {
        let mut path = format!("/v1/apps/{app_id}/env?env={env}");
        if let Some(sid) = service_id {
            path.push_str(&format!("&service_id={sid}"));
        }
        let resp = self.get(&path)?;
        self.handle_response(resp)
    }

    pub fn delete_env_var(
        &self,
        app_id: &str,
        key: &str,
        service_id: Option<&str>,
        env: &str,
    ) -> Result<(), FlooApiError> {
        let mut path = format!("/v1/apps/{app_id}/env/{key}?env={env}");
        if let Some(sid) = service_id {
            path.push_str(&format!("&service_id={sid}"));
        }
        let resp = self.delete(&path)?;
        if resp.status().as_u16() == 204 {
            return Ok(());
        }
        self.handle_response_value(resp)?;
        Ok(())
    }

    pub fn get_env_var(
        &self,
        app_id: &str,
        key: &str,
        service_id: Option<&str>,
        env: &str,
    ) -> Result<EnvVar, FlooApiError> {
        let mut path = format!("/v1/apps/{app_id}/env/{key}?env={env}");
        if let Some(sid) = service_id {
            path.push_str(&format!("&service_id={sid}"));
        }
        let resp = self.get(&path)?;
        self.handle_response(resp)
    }

    pub fn import_env_vars(
        &self,
        app_id: &str,
        env_vars: &[(String, String)],
        service_id: Option<&str>,
        env: &str,
    ) -> Result<Value, FlooApiError> {
        let vars: Vec<Value> = env_vars
            .iter()
            .map(|(k, v)| serde_json::json!({"key": k, "value": v}))
            .collect();
        let mut body = serde_json::json!({"env_vars": vars});
        if let Some(sid) = service_id {
            body.as_object_mut()
                .unwrap()
                .insert("service_id".to_string(), Value::String(sid.to_string()));
        }
        let resp = self.post_json(&format!("/v1/apps/{app_id}/env/import?env={env}"), &body)?;
        self.handle_response(resp)
    }

    pub fn list_services(
        &self,
        app_id: &str,
        environment: Option<&str>,
    ) -> Result<ListServicesResponse, FlooApiError> {
        let mut path = format!("/v1/apps/{app_id}/services?page=1&per_page=100");
        if let Some(env) = environment {
            path.push_str(&format!("&environment={env}"));
        }
        let resp = self.get(&path)?;
        self.handle_response(resp)
    }

    // --- Managed services ---

    pub fn list_managed_services(
        &self,
        app_id: &str,
    ) -> Result<ListManagedServicesResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/managed-services"))?;
        self.handle_response(resp)
    }

    pub fn get_managed_service(
        &self,
        app_id: &str,
        service_id: &str,
    ) -> Result<ManagedServiceDetail, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/managed-services/{service_id}"))?;
        self.handle_response(resp)
    }

    pub fn create_managed_service(
        &self,
        app_id: &str,
        body: &CreateManagedServiceRequest<'_>,
    ) -> Result<ManagedServiceDetail, FlooApiError> {
        let value = serde_json::to_value(body).map_err(|e| {
            FlooApiError::new(
                500,
                "SERIALIZE_ERROR",
                format!("Failed to serialize managed service request: {e}"),
            )
        })?;
        let resp = self.post_json(&format!("/v1/apps/{app_id}/managed-services"), &value)?;
        self.handle_response(resp)
    }

    pub fn delete_managed_service(
        &self,
        app_id: &str,
        service_id: &str,
    ) -> Result<(), FlooApiError> {
        let resp = self.delete(&format!("/v1/apps/{app_id}/managed-services/{service_id}"))?;
        let status = resp.status().as_u16();
        if status >= 400 {
            return Err(self.handle_error(resp));
        }
        Ok(())
    }

    pub fn managed_postgres_connection_usage(
        &self,
        app_id: &str,
        service_id: &str,
        env: &str,
    ) -> Result<Value, FlooApiError> {
        let resp = self.get(&format!(
            "/v1/apps/{app_id}/managed-services/{service_id}/connection-usage?env={env}"
        ))?;
        self.handle_response(resp)
    }

    // --- Preflight ---

    pub fn preflight(
        &self,
        app_id: &str,
        declared: &DeclaredState,
    ) -> Result<PreflightPlan, FlooApiError> {
        let body = serde_json::to_value(declared).map_err(|e| {
            FlooApiError::new(
                500,
                "SERIALIZE_ERROR",
                format!("Failed to serialize preflight declared state: {e}"),
            )
        })?;
        let resp = self.post_json(&format!("/v1/apps/{app_id}/preflight"), &body)?;
        self.handle_response(resp)
    }

    // --- Domains ---

    pub fn add_domain(
        &self,
        app_id: &str,
        hostname: &str,
        service: Option<&str>,
    ) -> Result<AddDomainResponse, FlooApiError> {
        let mut body = serde_json::json!({"hostname": hostname});
        if let Some(svc) = service {
            body["service"] = serde_json::Value::String(svc.to_string());
        }
        let resp = self.post_json(&format!("/v1/apps/{app_id}/domains"), &body)?;
        self.handle_response(resp)
    }

    pub fn list_domains(&self, app_id: &str) -> Result<ListDomainsResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/domains"))?;
        self.handle_response(resp)
    }

    pub fn verify_domain(
        &self,
        app_id: &str,
        hostname: &str,
    ) -> Result<AddDomainResponse, FlooApiError> {
        let resp = self.post_json(
            &format!("/v1/apps/{app_id}/domains/{hostname}/verify"),
            &serde_json::json!({}),
        )?;
        self.handle_response(resp)
    }

    pub fn delete_domain(&self, app_id: &str, hostname: &str) -> Result<(), FlooApiError> {
        let resp = self.delete(&format!("/v1/apps/{app_id}/domains/{hostname}"))?;
        if resp.status().as_u16() == 204 {
            return Ok(());
        }
        self.handle_response_value(resp)?;
        Ok(())
    }

    // --- Rollbacks ---

    pub fn rollback_deploy(&self, app_id: &str, deploy_id: &str) -> Result<Deploy, FlooApiError> {
        let body = serde_json::json!({"deploy_id": deploy_id});
        let mut req = self
            .client
            .post(self.url(&format!("/v1/apps/{app_id}/rollback")))
            .json(&body)
            .timeout(Duration::from_secs(120));
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        if let Some(org_id) = &self.default_org {
            req = req.header("X-Floo-Org-Id", org_id);
        }
        let resp = req
            .send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))?;
        self.handle_response(resp)
    }

    // --- Logs ---

    #[allow(clippy::too_many_arguments)]
    pub fn get_logs(
        &self,
        app_id: &str,
        limit: u32,
        since: Option<&str>,
        severity: Option<&str>,
        service: Option<&str>,
        search: Option<&str>,
        deployment: Option<&str>,
        environment: Option<&str>,
    ) -> Result<LogsResponse, FlooApiError> {
        let limit_str = limit.to_string();
        let mut params: Vec<(&str, &str)> = vec![("limit", &limit_str)];
        if let Some(s) = since {
            params.push(("since", s));
        }
        if let Some(sev) = severity {
            params.push(("severity", sev));
        }
        if let Some(svc) = service {
            params.push(("service", svc));
        }
        if let Some(q) = search {
            params.push(("search", q));
        }
        if let Some(dep) = deployment {
            params.push(("deployment", dep));
        }
        if let Some(env) = environment {
            params.push(("environment", env));
        }
        let resp = self.get_with_query(&format!("/v1/apps/{app_id}/logs"), &params)?;
        self.handle_response(resp)
    }

    pub fn get_request_logs(
        &self,
        app_id: &str,
        limit: u32,
        since: Option<&str>,
    ) -> Result<RequestLogsResponse, FlooApiError> {
        let limit_str = limit.to_string();
        let mut params: Vec<(&str, &str)> = vec![("limit", &limit_str)];
        if let Some(s) = since {
            params.push(("since", s));
        }
        let resp = self.get_with_query(&format!("/v1/apps/{app_id}/requests"), &params)?;
        self.handle_response(resp)
    }

    // --- GitHub ---

    pub fn github_setup_begin(&self) -> Result<Value, FlooApiError> {
        let resp = self.post_json("/v1/github/setup/begin", &serde_json::json!({}))?;
        self.handle_response(resp)
    }

    pub fn github_setup_poll(&self) -> Result<GitHubSetupPollResponse, FlooApiError> {
        let resp = self.get("/v1/github/setup/poll")?;
        self.handle_response(resp)
    }

    #[allow(dead_code)]
    pub fn github_installation_repos(&self, installation_id: u64) -> Result<Value, FlooApiError> {
        let resp = self.get(&format!("/v1/github/installations/{installation_id}/repos"))?;
        self.handle_response(resp)
    }

    pub fn github_check_repo_access(&self, repo: &str) -> Result<Value, FlooApiError> {
        let encoded = repo.replace('/', "%2F");
        let resp = self.get(&format!("/v1/github/check-repo-access?repo={encoded}"))?;
        self.handle_response(resp)
    }

    pub fn github_connect(
        &self,
        app_id: &str,
        repo_full_name: &str,
        branch: Option<&str>,
        skip_env_var_check: bool,
    ) -> Result<GitHubConnectResponse, FlooApiError> {
        let mut body = serde_json::json!({
            "repo_full_name": repo_full_name,
        });
        if let Some(b) = branch {
            body.as_object_mut()
                .unwrap()
                .insert("default_branch".to_string(), Value::String(b.to_string()));
        }
        if skip_env_var_check {
            body.as_object_mut()
                .unwrap()
                .insert("skip_env_var_check".to_string(), Value::Bool(true));
        }
        let resp = self.post_json(&format!("/v1/apps/{app_id}/github/connection"), &body)?;
        self.handle_response(resp)
    }

    pub fn github_disconnect(&self, app_id: &str) -> Result<(), FlooApiError> {
        let resp = self.delete(&format!("/v1/apps/{app_id}/github/connection"))?;
        self.handle_response_value(resp)?;
        Ok(())
    }

    pub fn github_status(&self, app_id: &str) -> Result<GitHubStatusResponse, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/github/connection"))?;
        self.handle_response(resp)
    }

    // --- Releases ---

    pub fn promote_app(
        &self,
        app_id: &str,
        tag: Option<&str>,
    ) -> Result<PromoteResponse, FlooApiError> {
        let mut body = serde_json::json!({});
        if let Some(t) = tag {
            body.as_object_mut()
                .unwrap()
                .insert("tag".to_string(), Value::String(t.to_string()));
        }
        let resp = self.post_json(&format!("/v1/apps/{app_id}/promote"), &body)?;
        self.handle_response(resp)
    }

    pub fn restart_app(
        &self,
        app_id: &str,
        services: Option<&[String]>,
    ) -> Result<Value, FlooApiError> {
        let body = match services {
            Some(svcs) => serde_json::json!({"services": svcs}),
            None => serde_json::json!({}),
        };
        let resp = self.post_json(&format!("/v1/apps/{app_id}/restart"), &body)?;
        self.handle_response(resp)
    }

    pub fn rebuild_app(
        &self,
        app_id: &str,
        runtime: &str,
        services: Option<&[String]>,
        skip_migrations: bool,
    ) -> Result<Deploy, FlooApiError> {
        let mut body = serde_json::json!({
            "runtime": runtime,
        });
        if let Some(svcs) = services {
            body["services_filter"] = serde_json::to_value(svcs).unwrap_or_default();
        }
        // The platform defaults skip_migrations=false on the deploy row;
        // only send the field when the caller opted in so the wire stays
        // tight on the common path.
        if skip_migrations {
            body["skip_migrations"] = Value::Bool(true);
        }
        let resp = self.post_json(&format!("/v1/apps/{app_id}/deploys"), &body)?;
        self.handle_response(resp)
    }

    pub fn list_releases(
        &self,
        app_id: &str,
        page: u32,
        per_page: u32,
    ) -> Result<ListReleasesResponse, FlooApiError> {
        let resp = self.get(&format!(
            "/v1/apps/{app_id}/releases?page={page}&per_page={per_page}"
        ))?;
        self.handle_response(resp)
    }

    pub fn get_release(&self, app_id: &str, release_id: &str) -> Result<Release, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/releases/{release_id}"))?;
        self.handle_response(resp)
    }

    // --- Dev Sessions ---

    pub fn create_dev_session(
        &self,
        app_id: &str,
        services: &[crate::api_types::DevSessionService],
    ) -> Result<crate::api_types::DevSessionResponse, FlooApiError> {
        let body = serde_json::json!({ "services": services });
        let resp = self.post_json(&format!("/v1/apps/{app_id}/dev-session"), &body)?;
        self.handle_response(resp)
    }

    pub fn delete_dev_session(&self, app_id: &str, session_id: &str) -> Result<(), FlooApiError> {
        let resp = self.delete(&format!("/v1/apps/{app_id}/dev-session/{session_id}"))?;
        if resp.status().as_u16() == 204 || resp.status().is_success() {
            return Ok(());
        }
        self.handle_response_value(resp)?;
        Ok(())
    }

    // --- Billing usage ---

    pub fn get_org_cost_breakdown(
        &self,
        period: &str,
    ) -> Result<OrgCostBreakdownResponse, FlooApiError> {
        let resp =
            self.get_with_query("/v1/billing/orgs/me/cost-breakdown", &[("period", period)])?;
        self.handle_response(resp)
    }

    // --- Analytics ---

    pub fn get_app_analytics(
        &self,
        app_id: &str,
        period: &str,
    ) -> Result<AppAnalyticsResponse, FlooApiError> {
        let resp = self.get_with_query(
            &format!("/v1/apps/{app_id}/analytics"),
            &[("period", period)],
        )?;
        self.handle_response(resp)
    }

    pub fn get_org_analytics(&self, period: &str) -> Result<AppAnalyticsResponse, FlooApiError> {
        let resp = self.get_with_query("/v1/orgs/analytics", &[("period", period)])?;
        self.handle_response(resp)
    }

    // --- Database query/schema/migrate ---

    pub fn db_query(
        &self,
        app_id: &str,
        sql: &str,
        environment: &str,
        limit: u32,
    ) -> Result<Value, FlooApiError> {
        let body = serde_json::json!({
            "sql": sql,
            "environment": environment,
            "limit": limit,
        });
        let resp = self.post_json(&format!("/v1/apps/{app_id}/db/query"), &body)?;
        self.handle_response_value(resp)
    }

    pub fn db_schema(&self, app_id: &str) -> Result<Value, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/db/schema"))?;
        self.handle_response_value(resp)
    }

    pub fn db_migrate(&self, app_id: &str, environment: &str) -> Result<Value, FlooApiError> {
        let resp = self.post_json(
            &format!("/v1/apps/{app_id}/db/migrate"),
            &serde_json::json!({ "environment": environment }),
        )?;
        self.handle_response_value(resp)
    }

    // --- Reparo ---

    pub fn reparo_events(&self, app_id: &str, status: Option<&str>) -> Result<Value, FlooApiError> {
        let path = format!("/v1/apps/{app_id}/reparo/events");
        let resp = if let Some(s) = status {
            self.get_with_query(&path, &[("status", s)])?
        } else {
            self.get(&path)?
        };
        self.handle_response_value(resp)
    }

    // --- Feedback ---

    pub fn submit_feedback(
        &self,
        category: &str,
        message: &str,
        source: &str,
        context: Option<&str>,
        app_name: Option<&str>,
    ) -> Result<(), FlooApiError> {
        let mut body = serde_json::json!({
            "category": category,
            "message": message,
            "source": source,
        });
        if let Some(ctx) = context {
            body["context"] = Value::String(ctx.to_owned());
        }
        if let Some(app) = app_name {
            body["app_name"] = Value::String(app.to_owned());
        }
        let resp = self.post_json("/v1/feedback", &body)?;
        self.handle_response_value(resp)?;
        Ok(())
    }
}
