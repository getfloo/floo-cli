use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, FlooError};

use super::service_config::{ResourceConfig, ServiceIngress};
use super::SCHEMA_URL;

/// A single scheduled cron job declared in `[cron.<name>]`.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CronJobConfig {
    /// Cron expression (e.g. `"0 9 * * *"` for 9am daily).
    pub schedule: String,
    /// Shell command to execute inside the container.
    pub command: String,
    /// Which service's image to run the command in.
    pub service: String,
    /// Maximum execution time in seconds (default 300).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AppFileConfig {
    pub app: AppFileAppSection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postgres: Option<ManagedServiceSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis: Option<ManagedServiceSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<ManagedServiceSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reparo: Option<ReparoConfig>,
    #[serde(default)]
    pub services: HashMap<String, AppServiceEntry>,
    #[serde(default)]
    pub environments: HashMap<String, AppEnvironmentOverrides>,
    /// Scheduled cron jobs declared as `[cron.<name>]` sections.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cron: HashMap<String, CronJobConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct ReparoConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_threshold: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldown_minutes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_deploy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ManagedServiceSection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AuthSection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uris: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AppEnvironmentOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_mode: Option<AppAccessMode>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppAccessMode {
    Public,
    Password,
    #[serde(alias = "floo_accounts")]
    Accounts,
    Sso,
}

impl AppAccessMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AppAccessMode::Public => "public",
            AppAccessMode::Password => "password",
            AppAccessMode::Accounts => "accounts",
            AppAccessMode::Sso => "sso",
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppAgentMode {
    Readonly,
    Supervised,
    Autonomous,
}

impl AppAgentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AppAgentMode::Readonly => "readonly",
            AppAgentMode::Supervised => "supervised",
            AppAgentMode::Autonomous => "autonomous",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppFileAppSection {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_mode: Option<AppAccessMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_mode: Option<AppAgentMode>,
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
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingress: Option<ServiceIngress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instances: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_instances: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migrate_command: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AppServiceType {
    Web,
    Api,
    Worker,
}

impl fmt::Display for AppServiceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Web => write!(f, "web"),
            Self::Api => write!(f, "api"),
            Self::Worker => write!(f, "worker"),
        }
    }
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
    // Detect inline mode: any user-managed service has `port` set
    let has_inline = config.services.values().any(|e| e.port.is_some());

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

        // In inline mode, all services require port and path
        if has_inline {
            if entry.port.is_none() {
                return Err(FlooError::with_suggestion(
                    ErrorCode::InvalidProjectConfig,
                    format!(
                        "Service '{name}' in {} is missing 'port'. All user-managed services must declare a port in inline mode.",
                        super::APP_CONFIG_FILE
                    ),
                    format!("Add port = <number> to [services.{name}]."),
                ));
            }
            if entry.path.is_none() {
                return Err(FlooError::with_suggestion(
                    ErrorCode::InvalidProjectConfig,
                    format!(
                        "Service '{name}' in {} is missing 'path'. All user-managed services must declare a path in inline mode.",
                        super::APP_CONFIG_FILE
                    ),
                    format!("Add path = \"./subdir\" to [services.{name}]."),
                ));
            }
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

[postgres]
tier = "hobby"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert!(config.services.is_empty());
        let pg = config.postgres.unwrap();
        assert_eq!(pg.tier.as_deref(), Some("hobby"));
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
                agent_mode: None,
            },
            auth: None,
            postgres: None,
            redis: None,
            storage: None,
            resources: None,
            reparo: None,
            cron: HashMap::new(),
            services: HashMap::new(),
            environments: HashMap::new(),
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

[postgres]

[redis]

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
        assert_eq!(config.services.len(), 2);
        assert!(config.postgres.is_some());
        assert!(config.redis.is_some());
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
    fn test_load_app_config_with_access_mode_accounts() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"
access_mode = "accounts"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.app.access_mode, Some(AppAccessMode::Accounts));
    }

    #[test]
    fn test_load_app_config_with_access_mode_floo_accounts_alias() {
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
        assert_eq!(config.app.access_mode, Some(AppAccessMode::Accounts));
    }

    #[test]
    fn test_load_app_config_with_access_mode_sso() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"
access_mode = "sso"
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.app.access_mode, Some(AppAccessMode::Sso));
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

    #[test]
    fn test_load_app_config_inline_services_with_port() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[resources]
cpu = "1"
memory = "2Gi"

[services.api]
type = "api"
path = "./backend"
port = 8000
ingress = "public"
env_file = ".env"
cpu = "2"
max_instances = 5

[services.web]
type = "web"
path = "./frontend"
port = 3000
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.services.len(), 2);

        let api = &config.services["api"];
        assert_eq!(api.port, Some(8000));
        assert_eq!(api.env_file.as_deref(), Some(".env"));
        assert_eq!(api.cpu.as_deref(), Some("2"));
        assert_eq!(api.max_instances, Some(5));

        let web = &config.services["web"];
        assert_eq!(web.port, Some(3000));
        assert!(web.cpu.is_none());

        let res = config.resources.unwrap();
        assert_eq!(res.cpu.as_deref(), Some("1"));
        assert_eq!(res.memory.as_deref(), Some("2Gi"));
    }

    #[test]
    fn test_load_app_config_inline_requires_port_for_all_user_managed() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[services.api]
type = "api"
path = "./backend"
port = 8000

[services.web]
type = "web"
path = "./frontend"
"#,
        )
        .unwrap();

        // api has port, web doesn't — error
        let err = load_app_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
        assert!(err.message.contains("web"));
        assert!(err.message.contains("port"));
    }

    #[test]
    fn test_load_app_config_with_auth_redirect_uris() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"
access_mode = "accounts"

[auth]
redirect_uris = ["http://localhost:3000/callback", "https://myapp.com/callback"]
"#,
        )
        .unwrap();

        let config = load_app_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.app.access_mode, Some(AppAccessMode::Accounts));
        let auth = config.auth.unwrap();
        let uris = auth.redirect_uris.unwrap();
        assert_eq!(uris.len(), 2);
        assert_eq!(uris[0], "http://localhost:3000/callback");
    }

    #[test]
    fn test_load_app_config_without_auth_section() {
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
        assert!(config.auth.is_none());
    }

    #[test]
    fn test_load_app_config_auth_unknown_field_rejected() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(super::super::APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[auth]
bad_field = true
"#,
        )
        .unwrap();

        let err = load_app_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
    }
}
