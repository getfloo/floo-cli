#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use reqwest::blocking::{multipart, Client};
use serde_json::Value;

use crate::config::{load_config, FlooConfig};
use crate::errors::FlooApiError;

pub struct FlooClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
}

impl FlooClient {
    pub fn new(config: Option<FlooConfig>) -> Self {
        let config = config.unwrap_or_else(load_config);
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");

        Self {
            client,
            base_url: config.api_url.clone(),
            api_key: config.api_key.clone(),
        }
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

    fn patch_json(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<reqwest::blocking::Response, FlooApiError> {
        let mut req = self.client.patch(self.url(path)).json(body);
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

    pub fn login(&self, email: &str, password: &str) -> Result<Value, FlooApiError> {
        let body = serde_json::json!({"email": email, "password": password});
        let resp = self.post_json("/v1/auth/login", &body)?;
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

    pub fn update_app(
        &self,
        app_id: &str,
        fields: &HashMap<String, String>,
    ) -> Result<Value, FlooApiError> {
        let body = serde_json::to_value(fields).unwrap_or(Value::Object(Default::default()));
        let resp = self.patch_json(&format!("/v1/apps/{app_id}"), &body)?;
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

        let form = multipart::Form::new()
            .part("file", file_part)
            .text("runtime", runtime.to_string())
            .text("framework", framework.unwrap_or("").to_string());

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

    // --- Logs ---

    pub fn get_logs(
        &self,
        app_id: &str,
        limit: u32,
        since: Option<&str>,
        severity: Option<&str>,
    ) -> Result<Value, FlooApiError> {
        let mut path = format!("/v1/apps/{app_id}/logs?limit={limit}");
        if let Some(s) = since {
            path.push_str(&format!("&since={s}"));
        }
        if let Some(sev) = severity {
            path.push_str(&format!("&severity={sev}"));
        }
        let resp = self.get(&path)?;
        self.handle_response(resp)
    }
}
