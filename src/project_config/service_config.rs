use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, FlooError};

use super::app_config::AppAccessMode;
use super::SCHEMA_URL;

// --- API wire format (sent to the API as JSON) ---

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

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq)]
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

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq)]
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

// --- floo.service.toml structs ---

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceFileConfig {
    pub app: ServiceFileAppSection,
    pub service: ServiceSection,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceFileAppSection {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_mode: Option<AppAccessMode>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ServiceSection {
    pub name: String,
    #[serde(rename = "type")]
    pub service_type: ServiceType,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingress: Option<ServiceIngress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_file: Option<String>,
}

impl ServiceSection {
    /// Resolve the effective ingress mode: explicit value if set,
    /// otherwise `Internal` for workers, `Public` for everything else.
    pub fn resolved_ingress(&self) -> ServiceIngress {
        self.ingress.unwrap_or(match self.service_type {
            ServiceType::Worker => ServiceIngress::Internal,
            _ => ServiceIngress::Public,
        })
    }

    pub fn to_api_service_config(&self, path: &str) -> ServiceConfig {
        ServiceConfig {
            name: self.name.clone(),
            service_type: self.service_type,
            path: path.to_string(),
            port: self.port,
            ingress: self.resolved_ingress(),
        }
    }
}

pub fn load_service_config(dir: &Path) -> Result<Option<ServiceFileConfig>, FlooError> {
    let config_path = dir.join(super::SERVICE_CONFIG_FILE);
    if !config_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&config_path).map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::InvalidProjectConfig,
            format!("Failed to read {}: {e}", super::SERVICE_CONFIG_FILE),
            format!("See {SCHEMA_URL} for the schema reference."),
        )
    })?;

    let config: ServiceFileConfig = toml::from_str(&content).map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::InvalidProjectConfig,
            format!("Invalid {}: {e}", super::SERVICE_CONFIG_FILE),
            format!("See {SCHEMA_URL} for the schema reference."),
        )
    })?;

    Ok(Some(config))
}

pub fn write_service_config(dir: &Path, config: &ServiceFileConfig) -> Result<(), FlooError> {
    let config_path = dir.join(super::SERVICE_CONFIG_FILE);
    let content = toml::to_string_pretty(config).map_err(|e| {
        FlooError::new(
            ErrorCode::ConfigWriteError,
            format!("Failed to serialize {}: {e}", super::SERVICE_CONFIG_FILE),
        )
    })?;
    std::fs::write(&config_path, content).map_err(|e| {
        FlooError::new(
            ErrorCode::ConfigWriteError,
            format!("Failed to write {}: {e}", super::SERVICE_CONFIG_FILE),
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_service_config_valid() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::SERVICE_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[service]
name = "api"
type = "api"
port = 8000
ingress = "public"
"#,
        )
        .unwrap();

        let config = load_service_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.app.name, "my-app");
        assert_eq!(config.service.name, "api");
        assert_eq!(config.service.service_type, ServiceType::Api);
        assert_eq!(config.service.port, 8000);
        assert_eq!(config.service.ingress, Some(ServiceIngress::Public));
        assert!(config.service.env_file.is_none());
    }

    #[test]
    fn test_load_service_config_with_env_file() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::SERVICE_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[service]
name = "web"
type = "web"
port = 3000
ingress = "public"
env_file = ".env"
"#,
        )
        .unwrap();

        let config = load_service_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.service.env_file.as_deref(), Some(".env"));
    }

    #[test]
    fn test_load_service_config_missing_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = load_service_config(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_load_service_config_unknown_field_rejected() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::SERVICE_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[service]
name = "api"
type = "api"
port = 8000
ingress = "public"
unknown = "bad"
"#,
        )
        .unwrap();

        let err = load_service_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
    }

    #[test]
    fn test_load_service_config_invalid_type() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::SERVICE_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[service]
name = "db"
type = "database"
port = 5432
ingress = "internal"
"#,
        )
        .unwrap();

        let err = load_service_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
    }

    #[test]
    fn test_write_and_reload_service_config() {
        let dir = TempDir::new().unwrap();
        let config = ServiceFileConfig {
            app: ServiceFileAppSection {
                name: "roundtrip-app".to_string(),
                access_mode: None,
            },
            service: ServiceSection {
                name: "web".to_string(),
                service_type: ServiceType::Web,
                port: 3000,
                ingress: Some(ServiceIngress::Public),
                env_file: None,
            },
        };

        write_service_config(dir.path(), &config).unwrap();
        let loaded = load_service_config(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.app.name, "roundtrip-app");
        assert_eq!(loaded.service.name, "web");
        assert_eq!(loaded.service.port, 3000);
    }

    #[test]
    fn test_service_section_to_api_service_config() {
        let section = ServiceSection {
            name: "api".to_string(),
            service_type: ServiceType::Api,
            port: 8000,
            ingress: Some(ServiceIngress::Internal),
            env_file: None,
        };

        let api_config = section.to_api_service_config("backend");
        assert_eq!(api_config.name, "api");
        assert_eq!(api_config.service_type, ServiceType::Api);
        assert_eq!(api_config.path, "backend");
        assert_eq!(api_config.port, 8000);
        assert_eq!(api_config.ingress, ServiceIngress::Internal);
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
        assert!(json.get("type").is_none());
    }

    #[test]
    fn test_load_service_config_with_access_mode() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::SERVICE_CONFIG_FILE),
            r#"
[app]
name = "my-app"
access_mode = "floo_accounts"

[service]
name = "api"
type = "api"
port = 8000
ingress = "public"
"#,
        )
        .unwrap();

        let config = load_service_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.app.access_mode, Some(AppAccessMode::FlooAccounts));
    }

    #[test]
    fn test_load_service_config_without_access_mode() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::SERVICE_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[service]
name = "api"
type = "api"
port = 8000
ingress = "public"
"#,
        )
        .unwrap();

        let config = load_service_config(dir.path()).unwrap().unwrap();
        assert!(config.app.access_mode.is_none());
    }

    #[test]
    fn test_worker_defaults_to_internal_ingress() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::SERVICE_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[service]
name = "bg"
type = "worker"
port = 8000
"#,
        )
        .unwrap();

        let config = load_service_config(dir.path()).unwrap().unwrap();
        assert!(config.service.ingress.is_none());
        assert_eq!(config.service.resolved_ingress(), ServiceIngress::Internal);

        let api_cfg = config.service.to_api_service_config(".");
        assert_eq!(api_cfg.ingress, ServiceIngress::Internal);
    }

    #[test]
    fn test_worker_explicit_public_overrides_default() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::SERVICE_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[service]
name = "bg"
type = "worker"
port = 8000
ingress = "public"
"#,
        )
        .unwrap();

        let config = load_service_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.service.ingress, Some(ServiceIngress::Public));
        assert_eq!(config.service.resolved_ingress(), ServiceIngress::Public);
    }

    #[test]
    fn test_api_defaults_to_public_ingress() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::SERVICE_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[service]
name = "api"
type = "api"
port = 8000
"#,
        )
        .unwrap();

        let config = load_service_config(dir.path()).unwrap().unwrap();
        assert!(config.service.ingress.is_none());
        assert_eq!(config.service.resolved_ingress(), ServiceIngress::Public);
    }
}
