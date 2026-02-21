use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::errors::FlooError;

const SCHEMA_URL: &str = "https://getfloo.com/docs/floo-toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub app: AppConfig,
    #[serde(default)]
    pub services: Vec<ServiceConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    pub name: String,
    #[serde(rename(deserialize = "type", serialize = "service_type"))]
    pub service_type: ServiceType,
    pub path: String,
    pub port: u16,
    pub ingress: ServiceIngress,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    Web,
    Api,
    Worker,
}

impl fmt::Display for ServiceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceType::Web => write!(f, "web"),
            ServiceType::Api => write!(f, "api"),
            ServiceType::Worker => write!(f, "worker"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceIngress {
    Public,
    Internal,
}

impl fmt::Display for ServiceIngress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceIngress::Public => write!(f, "public"),
            ServiceIngress::Internal => write!(f, "internal"),
        }
    }
}

/// Load and validate `floo.toml` from `dir`.
///
/// Returns `Ok(None)` if the file is absent (no-op).
/// Returns `Err(FlooError { code: "INVALID_PROJECT_CONFIG", ... })` on any parse or
/// validation error.
pub fn load_project_config(dir: &Path) -> Result<Option<ProjectConfig>, FlooError> {
    let config_path = dir.join("floo.toml");
    if !config_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&config_path).map_err(|e| {
        FlooError::with_suggestion(
            "INVALID_PROJECT_CONFIG",
            format!("Failed to read floo.toml: {e}"),
            format!("See {SCHEMA_URL} for the schema reference."),
        )
    })?;

    let config: ProjectConfig = toml::from_str(&content).map_err(|e| {
        FlooError::with_suggestion(
            "INVALID_PROJECT_CONFIG",
            format!("Invalid floo.toml: {e}"),
            format!("See {SCHEMA_URL} for the schema reference."),
        )
    })?;

    if config.services.is_empty() {
        return Err(FlooError::with_suggestion(
            "INVALID_PROJECT_CONFIG",
            "floo.toml must define at least one [[services]] entry.",
            format!("See {SCHEMA_URL} for the schema reference."),
        ));
    }

    Ok(Some(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_config(dir: &TempDir, content: &str) {
        fs::write(dir.path().join("floo.toml"), content).unwrap();
    }

    #[test]
    fn test_valid_single_service() {
        let dir = TempDir::new().unwrap();
        write_config(
            &dir,
            r#"
[app]
name = "my-app"

[[services]]
name = "web"
type = "web"
path = "."
port = 3000
ingress = "public"
"#,
        );

        let result = load_project_config(dir.path()).unwrap();
        let config = result.unwrap();
        assert_eq!(config.app.name, "my-app");
        assert_eq!(config.services.len(), 1);
        assert_eq!(config.services[0].name, "web");
        assert_eq!(config.services[0].service_type, ServiceType::Web);
        assert_eq!(config.services[0].port, 3000);
        assert_eq!(config.services[0].ingress, ServiceIngress::Public);
    }

    #[test]
    fn test_valid_multi_service() {
        let dir = TempDir::new().unwrap();
        write_config(
            &dir,
            r#"
[app]
name = "stack"

[[services]]
name = "frontend"
type = "web"
path = "frontend"
port = 3000
ingress = "public"

[[services]]
name = "backend"
type = "api"
path = "backend"
port = 8000
ingress = "internal"
"#,
        );

        let result = load_project_config(dir.path()).unwrap();
        let config = result.unwrap();
        assert_eq!(config.services.len(), 2);
        assert_eq!(config.services[0].service_type, ServiceType::Web);
        assert_eq!(config.services[1].service_type, ServiceType::Api);
        assert_eq!(config.services[1].ingress, ServiceIngress::Internal);
    }

    #[test]
    fn test_missing_floo_toml_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = load_project_config(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_unknown_top_level_field() {
        let dir = TempDir::new().unwrap();
        write_config(
            &dir,
            r#"
[app]
name = "my-app"

[unknown_field]
foo = "bar"

[[services]]
name = "web"
type = "web"
path = "."
port = 3000
ingress = "public"
"#,
        );

        let err = load_project_config(dir.path()).unwrap_err();
        assert_eq!(err.code, "INVALID_PROJECT_CONFIG");
    }

    #[test]
    fn test_invalid_service_type() {
        let dir = TempDir::new().unwrap();
        write_config(
            &dir,
            r#"
[app]
name = "my-app"

[[services]]
name = "db"
type = "database"
path = "."
port = 5432
ingress = "internal"
"#,
        );

        let err = load_project_config(dir.path()).unwrap_err();
        assert_eq!(err.code, "INVALID_PROJECT_CONFIG");
    }

    #[test]
    fn test_invalid_service_ingress() {
        let dir = TempDir::new().unwrap();
        write_config(
            &dir,
            r#"
[app]
name = "my-app"

[[services]]
name = "web"
type = "web"
path = "."
port = 3000
ingress = "private"
"#,
        );

        let err = load_project_config(dir.path()).unwrap_err();
        assert_eq!(err.code, "INVALID_PROJECT_CONFIG");
    }

    #[test]
    fn test_missing_app_name() {
        let dir = TempDir::new().unwrap();
        write_config(
            &dir,
            r#"
[app]

[[services]]
name = "web"
type = "web"
path = "."
port = 3000
ingress = "public"
"#,
        );

        let err = load_project_config(dir.path()).unwrap_err();
        assert_eq!(err.code, "INVALID_PROJECT_CONFIG");
    }

    #[test]
    fn test_service_config_json_serialization() {
        let config = ServiceConfig {
            name: "api".to_string(),
            service_type: ServiceType::Api,
            path: "backend".to_string(),
            port: 8000,
            ingress: ServiceIngress::Internal,
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["name"], "api");
        assert_eq!(json["service_type"], "api");
        assert_eq!(json["path"], "backend");
        assert_eq!(json["port"], 8000);
        assert_eq!(json["ingress"], "internal");
        // Verify "type" key is NOT present (serialized as "service_type")
        assert!(json.get("type").is_none());
    }

    #[test]
    fn test_empty_services_array() {
        let dir = TempDir::new().unwrap();
        write_config(
            &dir,
            r#"
[app]
name = "my-app"
"#,
        );

        let err = load_project_config(dir.path()).unwrap_err();
        assert_eq!(err.code, "INVALID_PROJECT_CONFIG");
        assert!(err.message.contains("at least one"));
    }
}
