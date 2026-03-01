use std::path::{Path, PathBuf};

use crate::errors::{ErrorCode, FlooError};

use super::app_config::{load_app_config, AppFileConfig};
use super::service_config::{load_service_config, ServiceFileConfig};
use super::{APP_CONFIG_FILE, LEGACY_CONFIG_FILE, MAX_WALK_UP_LEVELS, SERVICE_CONFIG_FILE};

#[derive(Debug)]
pub enum AppSource {
    Flag,
    ServiceFile,
    AppFile,
}

#[derive(Debug)]
pub struct ResolvedApp {
    pub app_name: String,
    pub source: AppSource,
    pub service_config: Option<ServiceFileConfig>,
    pub app_config: Option<AppFileConfig>,
    pub config_dir: PathBuf,
}

pub fn resolve_app_context(cwd: &Path, app_flag: Option<&str>) -> Result<ResolvedApp, FlooError> {
    // 1. --app flag takes precedence
    if let Some(app_name) = app_flag {
        let service_config = load_service_config(cwd)?;
        let app_config = load_app_config(cwd)?;
        return Ok(ResolvedApp {
            app_name: app_name.to_string(),
            source: AppSource::Flag,
            service_config,
            app_config,
            config_dir: cwd.to_path_buf(),
        });
    }

    // 2. Walk up from CWD looking for config files
    let mut current = cwd.to_path_buf();
    for _ in 0..MAX_WALK_UP_LEVELS {
        // Check for legacy floo.toml first
        if current.join(LEGACY_CONFIG_FILE).exists() {
            return Err(FlooError::with_suggestion(
                ErrorCode::LegacyConfig,
                format!(
                    "Found legacy {} in '{}'. This format is no longer supported.",
                    LEGACY_CONFIG_FILE,
                    current.display()
                ),
                format!(
                    "Migrate to {} + {}. See https://getfloo.com/docs/migration for details.",
                    APP_CONFIG_FILE, SERVICE_CONFIG_FILE
                ),
            ));
        }

        // Check for floo.service.toml
        if let Some(svc_config) = load_service_config(&current)? {
            let app_name = svc_config.app.name.clone();
            let app_config = load_app_config(&current)?;
            return Ok(ResolvedApp {
                app_name,
                source: AppSource::ServiceFile,
                service_config: Some(svc_config),
                app_config,
                config_dir: current,
            });
        }

        // Check for floo.app.toml
        if let Some(app_cfg) = load_app_config(&current)? {
            let app_name = app_cfg.app.name.clone();
            return Ok(ResolvedApp {
                app_name,
                source: AppSource::AppFile,
                service_config: None,
                app_config: Some(app_cfg),
                config_dir: current,
            });
        }

        // Walk up one level
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }

    Err(FlooError::with_suggestion(
        ErrorCode::NoConfigFound,
        format!("No {} or {} found.", SERVICE_CONFIG_FILE, APP_CONFIG_FILE),
        format!(
            "Run 'floo deploy' interactively to create config files, or write {} manually.",
            SERVICE_CONFIG_FILE
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_with_app_flag() {
        let dir = TempDir::new().unwrap();
        let result = resolve_app_context(dir.path(), Some("my-app")).unwrap();
        assert_eq!(result.app_name, "my-app");
        assert!(matches!(result.source, AppSource::Flag));
        assert!(result.service_config.is_none());
        assert!(result.app_config.is_none());
    }

    #[test]
    fn test_resolve_with_app_flag_still_loads_configs() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(SERVICE_CONFIG_FILE),
            r#"
[app]
name = "config-app"

[service]
name = "web"
type = "web"
port = 3000
ingress = "public"
"#,
        )
        .unwrap();

        let result = resolve_app_context(dir.path(), Some("flag-app")).unwrap();
        assert_eq!(result.app_name, "flag-app");
        assert!(matches!(result.source, AppSource::Flag));
        assert!(result.service_config.is_some());
    }

    #[test]
    fn test_resolve_from_service_config() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(SERVICE_CONFIG_FILE),
            r#"
[app]
name = "svc-app"

[service]
name = "api"
type = "api"
port = 8000
ingress = "public"
"#,
        )
        .unwrap();

        let result = resolve_app_context(dir.path(), None).unwrap();
        assert_eq!(result.app_name, "svc-app");
        assert!(matches!(result.source, AppSource::ServiceFile));
        assert!(result.service_config.is_some());
    }

    #[test]
    fn test_resolve_from_app_config() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(APP_CONFIG_FILE),
            r#"
[app]
name = "app-only"
"#,
        )
        .unwrap();

        let result = resolve_app_context(dir.path(), None).unwrap();
        assert_eq!(result.app_name, "app-only");
        assert!(matches!(result.source, AppSource::AppFile));
        assert!(result.service_config.is_none());
        assert!(result.app_config.is_some());
    }

    #[test]
    fn test_resolve_service_config_takes_priority_over_app_config() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(SERVICE_CONFIG_FILE),
            r#"
[app]
name = "svc-wins"

[service]
name = "web"
type = "web"
port = 3000
ingress = "public"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join(APP_CONFIG_FILE),
            r#"
[app]
name = "app-loses"
"#,
        )
        .unwrap();

        let result = resolve_app_context(dir.path(), None).unwrap();
        assert_eq!(result.app_name, "svc-wins");
        assert!(matches!(result.source, AppSource::ServiceFile));
    }

    #[test]
    fn test_resolve_walk_up_finds_config() {
        let dir = TempDir::new().unwrap();
        // Write config in parent dir
        fs::write(
            dir.path().join(APP_CONFIG_FILE),
            r#"
[app]
name = "parent-app"
"#,
        )
        .unwrap();

        // Create a child dir
        let child = dir.path().join("subdir");
        fs::create_dir(&child).unwrap();

        let result = resolve_app_context(&child, None).unwrap();
        assert_eq!(result.app_name, "parent-app");
        assert!(matches!(result.source, AppSource::AppFile));
        assert_eq!(result.config_dir, dir.path());
    }

    #[test]
    fn test_resolve_legacy_config_errors() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(LEGACY_CONFIG_FILE),
            r#"
[app]
name = "old-app"

[[services]]
name = "web"
type = "web"
path = "."
port = 3000
ingress = "public"
"#,
        )
        .unwrap();

        let err = resolve_app_context(dir.path(), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::LegacyConfig);
        assert!(err.message.contains("no longer supported"));
    }

    #[test]
    fn test_resolve_legacy_config_walk_up() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(LEGACY_CONFIG_FILE), "[app]\nname = \"old\"").unwrap();

        let child = dir.path().join("subdir");
        fs::create_dir(&child).unwrap();

        let err = resolve_app_context(&child, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::LegacyConfig);
    }

    #[test]
    fn test_resolve_no_config_errors() {
        let dir = TempDir::new().unwrap();
        let err = resolve_app_context(dir.path(), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::NoConfigFound);
    }

    #[test]
    fn test_resolve_walk_up_prefers_closer_config() {
        let dir = TempDir::new().unwrap();
        // Parent has app config
        fs::write(
            dir.path().join(APP_CONFIG_FILE),
            r#"
[app]
name = "parent-app"
"#,
        )
        .unwrap();

        // Child has service config
        let child = dir.path().join("subdir");
        fs::create_dir(&child).unwrap();
        fs::write(
            child.join(SERVICE_CONFIG_FILE),
            r#"
[app]
name = "child-app"

[service]
name = "web"
type = "web"
port = 3000
ingress = "public"
"#,
        )
        .unwrap();

        let result = resolve_app_context(&child, None).unwrap();
        assert_eq!(result.app_name, "child-app");
        assert!(matches!(result.source, AppSource::ServiceFile));
    }

    #[test]
    fn test_resolve_also_loads_app_config_when_service_found() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(SERVICE_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[service]
name = "web"
type = "web"
port = 3000
ingress = "public"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join(APP_CONFIG_FILE),
            r#"
[app]
name = "my-app"

[services.db]
type = "postgres"
"#,
        )
        .unwrap();

        let result = resolve_app_context(dir.path(), None).unwrap();
        assert!(result.service_config.is_some());
        assert!(result.app_config.is_some());
    }
}
