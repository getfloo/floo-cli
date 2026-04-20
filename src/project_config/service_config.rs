use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, FlooError};

use super::app_config::AppAccessMode;
use super::SCHEMA_URL;

// --- Resource limits ---

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct ResourceConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_instances: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_instances: Option<u32>,
}

// --- Env var contract ---

/// Per-service declaration of which env vars the service requires.
///
/// Declares names and kinds only — values always live in `.env` (local) or
/// encrypted EnvVar rows (server-side). The server-side deploy pipeline
/// gates on `required`: if any named key is missing or empty in the target
/// environment, the deploy fails with `MISSING_REQUIRED_ENV_VAR`. `optional`
/// is purely documentary.
///
/// Attached under `[services.<name>.env]` in `floo.app.toml` (inline mode)
/// or `[env]` at the top level of `floo.service.toml` (delegated mode).
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct ServiceEnvContract {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub optional: Vec<String>,
}

impl ServiceEnvContract {
    /// Validate that names are well-formed, non-duplicated, and non-overlapping.
    ///
    /// `where_` is a human-readable location string used in error messages
    /// (e.g. `"[services.api.env]"` or `"[env] in floo.service.toml"`).
    pub fn validate(&self, where_: &str) -> Result<(), FlooError> {
        let mut seen = HashSet::new();
        for (bucket, names) in [("required", &self.required), ("optional", &self.optional)] {
            for name in names {
                if !is_valid_env_name(name) {
                    return Err(FlooError::with_suggestion(
                        ErrorCode::InvalidProjectConfig,
                        format!("{where_} {bucket} contains invalid env var name '{name}'.",),
                        "Env var names must match /^[A-Za-z_][A-Za-z0-9_]*$/.".to_string(),
                    ));
                }
                if !seen.insert(name.clone()) {
                    return Err(FlooError::with_suggestion(
                        ErrorCode::InvalidProjectConfig,
                        format!(
                            "{where_} declares '{name}' more than once (across required and optional).",
                        ),
                        "Each env var name may appear in at most one of required/optional.".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

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
    pub migrate_command: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<ServiceEnvContract>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_instances: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrate_command: Option<String>,
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
            domain: self.domain.clone(),
            cpu: None,
            memory: None,
            max_instances: None,
            min_instances: self.min_instances,
            migrate_command: self.migrate_command.clone(),
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

    if let Some(ref env) = config.env {
        env.validate(&format!("[env] in {}", super::SERVICE_CONFIG_FILE))?;
    }

    Ok(Some(config))
}

#[cfg(test)]
fn write_service_config(dir: &Path, config: &ServiceFileConfig) -> Result<(), FlooError> {
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
                domain: None,
                min_instances: None,
                dev_command: None,
                migrate_command: None,
            },
            resources: None,
            env: None,
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
            domain: None,
            min_instances: None,
            dev_command: None,
            migrate_command: None,
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
            domain: None,
            cpu: None,
            memory: None,
            max_instances: None,
            min_instances: None,
            migrate_command: None,
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
        assert_eq!(config.app.access_mode, Some(AppAccessMode::Accounts));
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

    #[test]
    fn test_load_service_config_with_domain() {
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
domain = "getfloo.com"
"#,
        )
        .unwrap();

        let config = load_service_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.service.domain.as_deref(), Some("getfloo.com"));
    }

    #[test]
    fn test_load_service_config_without_domain() {
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
        assert!(config.service.domain.is_none());
    }

    #[test]
    fn test_to_api_service_config_passes_domain() {
        let section = ServiceSection {
            name: "web".to_string(),
            service_type: ServiceType::Web,
            port: 3000,
            ingress: Some(ServiceIngress::Public),
            env_file: None,
            domain: Some("getfloo.com".to_string()),
            min_instances: None,
            dev_command: None,
            migrate_command: None,
        };

        let api_config = section.to_api_service_config(".");
        assert_eq!(api_config.domain.as_deref(), Some("getfloo.com"));
    }

    #[test]
    fn test_service_config_json_domain_omitted_when_none() {
        let config = ServiceConfig {
            name: "api".to_string(),
            service_type: ServiceType::Api,
            path: "backend".to_string(),
            port: 8000,
            ingress: ServiceIngress::Public,
            domain: None,
            cpu: None,
            memory: None,
            max_instances: None,
            min_instances: None,
            migrate_command: None,
        };
        let json = serde_json::to_value(&config).unwrap();
        assert!(json.get("domain").is_none());
    }

    #[test]
    fn test_service_config_json_domain_included_when_set() {
        let config = ServiceConfig {
            name: "web".to_string(),
            service_type: ServiceType::Web,
            path: ".".to_string(),
            port: 3000,
            ingress: ServiceIngress::Public,
            domain: Some("getfloo.com".to_string()),
            cpu: None,
            memory: None,
            max_instances: None,
            min_instances: None,
            migrate_command: None,
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["domain"], "getfloo.com");
    }

    #[test]
    fn test_write_and_reload_service_config_with_domain() {
        let dir = TempDir::new().unwrap();
        let config = ServiceFileConfig {
            app: ServiceFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            service: ServiceSection {
                name: "web".to_string(),
                service_type: ServiceType::Web,
                port: 3000,
                ingress: Some(ServiceIngress::Public),
                env_file: None,
                domain: Some("getfloo.com".to_string()),
                min_instances: None,
                dev_command: None,
                migrate_command: None,
            },
            resources: None,
            env: None,
        };

        write_service_config(dir.path(), &config).unwrap();
        let loaded = load_service_config(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.service.domain.as_deref(), Some("getfloo.com"));
    }

    #[test]
    fn test_load_service_config_with_resources() {
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

[resources]
cpu = "2"
memory = "4Gi"
max_instances = 5
"#,
        )
        .unwrap();

        let config = load_service_config(dir.path()).unwrap().unwrap();
        let res = config.resources.unwrap();
        assert_eq!(res.cpu.as_deref(), Some("2"));
        assert_eq!(res.memory.as_deref(), Some("4Gi"));
        assert_eq!(res.max_instances, Some(5));
    }

    #[test]
    fn test_load_service_config_without_resources() {
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
        assert!(config.resources.is_none());
    }

    #[test]
    fn test_service_config_json_resources_omitted_when_none() {
        let config = ServiceConfig {
            name: "api".to_string(),
            service_type: ServiceType::Api,
            path: ".".to_string(),
            port: 8000,
            ingress: ServiceIngress::Public,
            domain: None,
            cpu: None,
            memory: None,
            max_instances: None,
            min_instances: None,
            migrate_command: None,
        };
        let json = serde_json::to_value(&config).unwrap();
        assert!(json.get("cpu").is_none());
        assert!(json.get("memory").is_none());
        assert!(json.get("max_instances").is_none());
    }

    #[test]
    fn test_service_config_json_resources_included_when_set() {
        let config = ServiceConfig {
            name: "api".to_string(),
            service_type: ServiceType::Api,
            path: ".".to_string(),
            port: 8000,
            ingress: ServiceIngress::Public,
            domain: None,
            cpu: Some("2".to_string()),
            memory: Some("4Gi".to_string()),
            max_instances: Some(5),
            min_instances: None,
            migrate_command: None,
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["cpu"], "2");
        assert_eq!(json["memory"], "4Gi");
        assert_eq!(json["max_instances"], 5);
    }

    #[test]
    fn test_load_service_config_with_env_contract() {
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

[env]
required = ["STRIPE_SECRET_KEY", "JWT_SECRET"]
optional = ["SENTRY_DSN"]
"#,
        )
        .unwrap();

        let config = load_service_config(dir.path()).unwrap().unwrap();
        let env = config.env.expect("env contract present");
        assert_eq!(env.required, vec!["STRIPE_SECRET_KEY", "JWT_SECRET"]);
        assert_eq!(env.optional, vec!["SENTRY_DSN"]);
    }

    #[test]
    fn test_load_service_config_env_contract_absent_is_none() {
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
        assert!(config.env.is_none());
    }

    #[test]
    fn test_load_service_config_env_contract_rejects_invalid_name() {
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

[env]
required = ["1INVALID"]
"#,
        )
        .unwrap();

        let err = load_service_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
        assert!(err.message.contains("'1INVALID'"));
    }

    #[test]
    fn test_load_service_config_env_contract_rejects_duplicate_within_bucket() {
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

[env]
required = ["JWT_SECRET", "JWT_SECRET"]
"#,
        )
        .unwrap();

        let err = load_service_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
        assert!(err.message.contains("'JWT_SECRET'"));
    }

    #[test]
    fn test_load_service_config_env_contract_rejects_overlap_across_buckets() {
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

[env]
required = ["JWT_SECRET"]
optional = ["JWT_SECRET"]
"#,
        )
        .unwrap();

        let err = load_service_config(dir.path()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
        assert!(err.message.contains("'JWT_SECRET'"));
    }

    #[test]
    fn test_env_contract_valid_names_accepted() {
        let contract = ServiceEnvContract {
            required: vec!["STRIPE_KEY".into(), "_INTERNAL".into(), "A1_B2".into()],
            optional: vec![],
        };
        contract.validate("[env]").unwrap();
    }

    #[test]
    fn test_env_contract_name_with_dash_rejected() {
        let contract = ServiceEnvContract {
            required: vec!["MY-KEY".into()],
            optional: vec![],
        };
        let err = contract.validate("[env]").unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
    }
}
