use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, FlooError};

use super::service_config::ServiceIngress;
use super::SCHEMA_URL;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppFileConfig {
    pub app: AppFileAppSection,
    #[serde(default)]
    pub services: HashMap<String, AppServiceEntry>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppAccessMode {
    Public,
    Password,
    FlooAccounts,
}

impl AppAccessMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AppAccessMode::Public => "public",
            AppAccessMode::Password => "password",
            AppAccessMode::FlooAccounts => "floo_accounts",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppFileAppSection {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_mode: Option<AppAccessMode>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppServiceEntry {
    #[serde(rename = "type")]
    pub service_type: AppServiceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingress: Option<ServiceIngress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AppServiceType {
    Web,
    Api,
    Worker,
    Postgres,
    Redis,
}

pub fn load_app_config(dir: &Path) -> Result<Option<AppFileConfig>, FlooError> {
    let config_path = dir.join(super::APP_CONFIG_FILE);
    if !config_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&config_path).map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::InvalidProjectConfig,
            format!("Failed to read {}: {e}", super::APP_CONFIG_FILE),
            format!("See {SCHEMA_URL} for the schema reference."),
        )
    })?;

    let config: AppFileConfig = toml::from_str(&content).map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::InvalidProjectConfig,
            format!("Invalid {}: {e}", super::APP_CONFIG_FILE),
            format!("See {SCHEMA_URL} for the schema reference."),
        )
    })?;

    validate_app_config(&config)?;

    Ok(Some(config))
}

fn validate_app_config(config: &AppFileConfig) -> Result<(), FlooError> {
    for (name, entry) in &config.services {
        if entry.path.is_some() && entry.repo.is_some() {
            return Err(FlooError::with_suggestion(
                ErrorCode::InvalidProjectConfig,
                format!(
                    "Service '{name}' in {} has both 'path' and 'repo' — these are mutually exclusive.",
                    super::APP_CONFIG_FILE
                ),
                "Use 'path' for monorepo services or 'repo' for multi-repo services, not both.",
            ));
        }
    }
    Ok(())
}

pub fn write_app_config(dir: &Path, config: &AppFileConfig) -> Result<(), FlooError> {
    let config_path = dir.join(super::APP_CONFIG_FILE);
    let content = toml::to_string_pretty(config).map_err(|e| {
        FlooError::new(
            ErrorCode::ConfigWriteError,
            format!("Failed to serialize {}: {e}", super::APP_CONFIG_FILE),
        )
    })?;
    std::fs::write(&config_path, content).map_err(|e| {
        FlooError::new(
            ErrorCode::ConfigWriteError,
            format!("Failed to write {}: {e}", super::APP_CONFIG_FILE),
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
    fn test_load_app_config_minimal() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.app.name, "my-app");
        assert!(config.services.is_empty());
    }

    #[test]
    fn test_load_app_config_with_floo_managed_service() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[services.db]
type = "postgres"
version = "16"
plan = "hobby"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.services.len(), 1);
        let db = &config.services["db"];
        assert_eq!(db.service_type, AppServiceType::Postgres);
        assert_eq!(db.version.as_deref(), Some("16"));
        assert_eq!(db.plan.as_deref(), Some("hobby"));
    }

    #[test]
    fn test_load_app_config_with_user_managed_service_path() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[services.api]
type = "api"
path = "./backend"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        let api = &config.services["api"];
        assert_eq!(api.service_type, AppServiceType::Api);
        assert_eq!(api.path.as_deref(), Some("./backend"));
        assert!(api.repo.is_none());
    }

    #[test]
    fn test_load_app_config_with_user_managed_service_repo() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[services.api]
type = "api"
repo = "myorg/my-api"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        let api = &config.services["api"];
        assert_eq!(api.repo.as_deref(), Some("myorg/my-api"));
        assert!(api.path.is_none());
    }

    #[test]
    fn test_load_app_config_rejects_path_and_repo() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[services.api]
type = "api"
path = "./backend"
repo = "myorg/my-api"
"#,
        )
        .unwrap();

        let err = load_app_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
        assert!(err.message.contains("mutually exclusive"));
    }

    #[test]
    fn test_load_app_config_missing_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = load_app_config(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_load_app_config_unknown_field_rejected() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"
unknown = "bad"
"#,
        )
        .unwrap();

        let err = load_app_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
    }

    #[test]
    fn test_load_app_config_invalid_service_type() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[services.db]
type = "mysql"
"#,
        )
        .unwrap();

        let err = load_app_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
    }

    #[test]
    fn test_write_and_reload_app_config() {
        let dir = TempDir::new().unwrap();
        let config = AppFileConfig {
            app: AppFileAppSection {
                name: "roundtrip-app".to_string(),
                access_mode: None,
            },
            services: HashMap::new(),
        };

        write_app_config(dir.path(), &config).unwrap();
        let loaded = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.app.name, "roundtrip-app");
        assert!(loaded.services.is_empty());
    }

    #[test]
    fn test_load_app_config_mixed_services() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "full-stack"

[services.db]
type = "postgres"

[services.cache]
type = "redis"

[services.web]
type = "web"
path = "./frontend"

[services.api]
type = "api"
path = "./backend"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.services.len(), 4);
        assert_eq!(config.services["db"].service_type, AppServiceType::Postgres);
        assert_eq!(config.services["cache"].service_type, AppServiceType::Redis);
        assert_eq!(config.services["web"].service_type, AppServiceType::Web);
        assert_eq!(config.services["api"].service_type, AppServiceType::Api);
    }

    #[test]
    fn test_load_app_config_with_access_mode_public() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"
access_mode = "public"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.app.access_mode, Some(AppAccessMode::Public));
    }

    #[test]
    fn test_load_app_config_with_access_mode_floo_accounts() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"
access_mode = "floo_accounts"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.app.access_mode, Some(AppAccessMode::FlooAccounts));
    }

    #[test]
    fn test_load_app_config_with_access_mode_password() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"
access_mode = "password"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.app.access_mode, Some(AppAccessMode::Password));
    }

    #[test]
    fn test_load_app_config_without_access_mode_defaults_none() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert!(config.app.access_mode.is_none());
    }

    #[test]
    fn test_load_app_config_with_service_ingress() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[services.worker]
type = "worker"
path = "./worker"
ingress = "internal"

[services.api]
type = "api"
path = "./backend"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        let worker = &config.services["worker"];
        assert_eq!(
            worker.ingress,
            Some(super::super::service_config::ServiceIngress::Internal)
        );
        let api = &config.services["api"];
        assert!(api.ingress.is_none());
    }

    #[test]
    fn test_load_app_config_invalid_access_mode() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"
access_mode = "private"
"#,
        )
        .unwrap();

        let err = load_app_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
    }
}
