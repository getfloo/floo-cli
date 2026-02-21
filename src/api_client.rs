use std::path::Path;
use std::time::Duration;

use reqwest::blocking::{multipart, Client};
use serde_json::Value;

use crate::config::{load_config, FlooConfig};
use crate::errors::FlooApiError;
use crate::project_config::ServiceConfig;

pub struct FlooClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
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
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn auth_header(&self) -> Option<String> {
        self.api_key.as_ref().map(|k| format!("Bearer {k}"))
    }

    fn handle_response(
        &self,
        response: reqwest::blocking::Response,
    ) -> Result<Value, FlooApiError> {
        let status = response.status().as_u16();
        if status >= 400 {
            let text = response.text().unwrap_or_default();
            let (code, message) = if let Ok(body) = serde_json::from_str::<Value>(&text) {
                let detail = body.get("detail").unwrap_or(&body);
                if let Some(obj) = detail.as_object() {
                    (
                        obj.get("code")
                            .and_then(|v| v.as_str())
                            .unwrap_or("API_ERROR")
                            .to_string(),
                        obj.get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&text)
                            .to_string(),
                    )
                } else {
                    (
                        "API_ERROR".to_string(),
                        detail.to_string().trim_matches('"').to_string(),
                    )
                }
            } else {
                ("API_ERROR".to_string(), text)
            };
            return Err(FlooApiError::new(status, code, message));
        }
        let body: Value = response.json().map_err(|e| {
            FlooApiError::new(500, "PARSE_ERROR", format!("Failed to parse response: {e}"))
        })?;
        Ok(body)
    }

    fn get(&self, path: &str) -> Result<reqwest::blocking::Response, FlooApiError> {
        let mut req = self.client.get(self.url(path));
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        req.send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))
    }

    fn post_json(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<reqwest::blocking::Response, FlooApiError> {
        let mut req = self.client.post(self.url(path)).json(body);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        req.send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))
    }

    fn delete(&self, path: &str) -> Result<reqwest::blocking::Response, FlooApiError> {
        let mut req = self.client.delete(self.url(path));
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        req.send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))
    }

    // --- Auth ---

    pub fn register(&self, email: &str, password: &str) -> Result<Value, FlooApiError> {
        let body = serde_json::json!({"email": email, "password": password});
        let resp = self.post_json("/v1/auth/register", &body)?;
        self.handle_response(resp)
    }

    pub fn device_authorize(&self) -> Result<Value, FlooApiError> {
        let resp = self.post_json("/v1/auth/device", &serde_json::json!({}))?;
        self.handle_response(resp)
    }

    pub fn device_token(&self, device_code: &str) -> Result<Value, FlooApiError> {
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

    // --- Apps ---

    pub fn create_app(&self, name: &str, runtime: Option<&str>) -> Result<Value, FlooApiError> {
        let mut body = serde_json::json!({"name": name});
        if let Some(rt) = runtime {
            body.as_object_mut()
                .unwrap()
                .insert("runtime".to_string(), Value::String(rt.to_string()));
        }
        let resp = self.post_json("/v1/apps", &body)?;
        self.handle_response(resp)
    }

    pub fn list_apps(&self, page: u32, per_page: u32) -> Result<Value, FlooApiError> {
        let resp = self.get(&format!("/v1/apps?page={page}&per_page={per_page}"))?;
        self.handle_response(resp)
    }

    pub fn get_app(&self, app_id: &str) -> Result<Value, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}"))?;
        self.handle_response(resp)
    }

    pub fn delete_app(&self, app_id: &str) -> Result<(), FlooApiError> {
        let resp = self.delete(&format!("/v1/apps/{app_id}"))?;
        if resp.status().as_u16() == 204 {
            return Ok(());
        }
        self.handle_response(resp)?;
        Ok(())
    }

    // --- Deploys ---

    pub fn create_deploy(
        &self,
        app_id: &str,
        tarball_path: &Path,
        runtime: &str,
        framework: Option<&str>,
        services: Option<&[ServiceConfig]>,
    ) -> Result<Value, FlooApiError> {
        let file_bytes = std::fs::read(tarball_path).map_err(|e| {
            FlooApiError::new(0, "FILE_ERROR", format!("Failed to read archive: {e}"))
        })?;
        let file_name = tarball_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("source.tar.gz")
            .to_string();

        let file_part = multipart::Part::bytes(file_bytes)
            .file_name(file_name)
            .mime_str("application/gzip")
            .unwrap();

        let mut form = multipart::Form::new()
            .part("file", file_part)
            .text("runtime", runtime.to_string())
            .text("framework", framework.unwrap_or("").to_string());

        if let Some(svcs) = services {
            let json = serde_json::to_string(svcs).map_err(|e| {
                FlooApiError::new(
                    0,
                    "SERIALIZATION_ERROR",
                    format!("Failed to serialize services: {e}"),
                )
            })?;
            form = form.text("services", json);
        }

        let mut req = self
            .client
            .post(self.url(&format!("/v1/apps/{app_id}/deploys")))
            .multipart(form)
            .timeout(Duration::from_secs(300));
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req
            .send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))?;
        self.handle_response(resp)
    }

    pub fn list_deploys(&self, app_id: &str) -> Result<Value, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/deploys"))?;
        self.handle_response(resp)
    }

    pub fn get_deploy(&self, app_id: &str, deploy_id: &str) -> Result<Value, FlooApiError> {
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

    pub fn set_env_var(&self, app_id: &str, key: &str, value: &str) -> Result<Value, FlooApiError> {
        let body = serde_json::json!({"key": key, "value": value});
        let resp = self.post_json(&format!("/v1/apps/{app_id}/env"), &body)?;
        self.handle_response(resp)
    }

    pub fn list_env_vars(&self, app_id: &str) -> Result<Value, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/env"))?;
        self.handle_response(resp)
    }

    pub fn delete_env_var(&self, app_id: &str, key: &str) -> Result<(), FlooApiError> {
        let resp = self.delete(&format!("/v1/apps/{app_id}/env/{key}"))?;
        if resp.status().as_u16() == 204 {
            return Ok(());
        }
        self.handle_response(resp)?;
        Ok(())
    }

    // --- Databases ---

    fn parse_database_response(&self, response: &Value) -> Result<Value, FlooApiError> {
        if response.get("host").is_some()
            && response.get("port").is_some()
            && response.get("database").is_some()
        {
            return Ok(response.clone());
        }

        if let Some(database) = response.get("database") {
            if database.is_object() {
                return Ok(database.clone());
            }
        }

        Err(FlooApiError::new(
            500,
            "PARSE_ERROR",
            "Failed to parse database info response from API.",
        ))
    }

    fn parse_database_from_list_response(&self, response: &Value) -> Result<Value, FlooApiError> {
        let databases = response
            .get("databases")
            .and_then(|value| value.as_array())
            .ok_or_else(|| {
                FlooApiError::new(
                    500,
                    "PARSE_ERROR",
                    "Failed to parse database list response from API.",
                )
            })?;

        let database = databases
            .iter()
            .find(|value| value.get("name").and_then(|v| v.as_str()) == Some("default"))
            .or_else(|| databases.first())
            .ok_or_else(|| {
                FlooApiError::new(
                    404,
                    "DATABASE_NOT_FOUND",
                    "Database not found for this app.",
                )
            })?;

        Ok(database.clone())
    }

    pub fn get_database_info(&self, app_id: &str) -> Result<Value, FlooApiError> {
        let db_info_response = self.get(&format!("/v1/apps/{app_id}/db"))?;
        match self.handle_response(db_info_response) {
            Ok(response) => self.parse_database_response(&response),
            Err(error) if error.status_code == 404 => {
                let list_response = self.get(&format!("/v1/apps/{app_id}/databases"))?;
                let list_body = self.handle_response(list_response)?;
                self.parse_database_from_list_response(&list_body)
            }
            Err(error) => Err(error),
        }
    }

    // --- Domains ---

    pub fn add_domain(&self, app_id: &str, hostname: &str) -> Result<Value, FlooApiError> {
        let body = serde_json::json!({"hostname": hostname});
        let resp = self.post_json(&format!("/v1/apps/{app_id}/domains"), &body)?;
        self.handle_response(resp)
    }

    pub fn list_domains(&self, app_id: &str) -> Result<Value, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/domains"))?;
        self.handle_response(resp)
    }

    pub fn delete_domain(&self, app_id: &str, hostname: &str) -> Result<(), FlooApiError> {
        let resp = self.delete(&format!("/v1/apps/{app_id}/domains/{hostname}"))?;
        if resp.status().as_u16() == 204 {
            return Ok(());
        }
        self.handle_response(resp)?;
        Ok(())
    }

    // --- Rollbacks ---

    pub fn rollback_deploy(&self, app_id: &str, deploy_id: &str) -> Result<Value, FlooApiError> {
        let body = serde_json::json!({"deploy_id": deploy_id});
        let mut req = self
            .client
            .post(self.url(&format!("/v1/apps/{app_id}/rollback")))
            .json(&body)
            .timeout(Duration::from_secs(120));
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req
            .send()
            .map_err(|e| FlooApiError::new(0, "CONNECTION_ERROR", e.to_string()))?;
        self.handle_response(resp)
    }

    // --- Logs ---

    pub fn get_logs(
        &self,
        app_id: &str,
        limit: u32,
        since: Option<&str>,
        severity: Option<&str>,
        service: Option<&str>,
    ) -> Result<Value, FlooApiError> {
        let mut path = format!("/v1/apps/{app_id}/logs?limit={limit}");
        if let Some(s) = since {
            path.push_str(&format!("&since={s}"));
        }
        if let Some(sev) = severity {
            path.push_str(&format!("&severity={sev}"));
        }
        if let Some(svc) = service {
            path.push_str(&format!("&service={svc}"));
        }
        let resp = self.get(&path)?;
        self.handle_response(resp)
    }

    // --- GitHub ---

    pub fn github_connect(
        &self,
        app_id: &str,
        repo_full_name: &str,
        installation_id: u64,
        branch: Option<&str>,
    ) -> Result<Value, FlooApiError> {
        let mut body = serde_json::json!({
            "repo_full_name": repo_full_name,
            "installation_id": installation_id,
        });
        if let Some(b) = branch {
            body.as_object_mut()
                .unwrap()
                .insert("branch".to_string(), Value::String(b.to_string()));
        }
        let resp = self.post_json(&format!("/v1/apps/{app_id}/github/connect"), &body)?;
        self.handle_response(resp)
    }

    pub fn github_disconnect(&self, app_id: &str) -> Result<(), FlooApiError> {
        let resp = self.delete(&format!("/v1/apps/{app_id}/github/disconnect"))?;
        if resp.status().as_u16() == 200 {
            let _ = self.handle_response(resp)?;
            return Ok(());
        }
        self.handle_response(resp)?;
        Ok(())
    }

    // --- Releases ---

    pub fn promote_app(&self, app_id: &str, tag: Option<&str>) -> Result<Value, FlooApiError> {
        let mut body = serde_json::json!({});
        if let Some(t) = tag {
            body.as_object_mut()
                .unwrap()
                .insert("tag".to_string(), Value::String(t.to_string()));
        }
        let resp = self.post_json(&format!("/v1/apps/{app_id}/promote"), &body)?;
        self.handle_response(resp)
    }

    pub fn list_releases(
        &self,
        app_id: &str,
        page: u32,
        per_page: u32,
    ) -> Result<Value, FlooApiError> {
        let resp = self.get(&format!(
            "/v1/apps/{app_id}/releases?page={page}&per_page={per_page}"
        ))?;
        self.handle_response(resp)
    }

    pub fn get_release(&self, app_id: &str, release_id: &str) -> Result<Value, FlooApiError> {
        let resp = self.get(&format!("/v1/apps/{app_id}/releases/{release_id}"))?;
        self.handle_response(resp)
    }
}
