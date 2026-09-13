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

/// Find the nearest manifest within the git boundary; the flag overrides only its app name.
pub fn resolve_app_context(cwd: &Path, app_flag: Option<&str>) -> Result<ResolvedApp, FlooError> {
    for current in cwd.ancestors().take(MAX_WALK_UP_LEVELS) {
        if current.join(LEGACY_CONFIG_FILE).exists() {
            return Err(FlooError::with_suggestion(
                ErrorCode::LegacyConfig,
                format!("Found legacy {LEGACY_CONFIG_FILE} in '{}'. This format is no longer supported.", current.display()),
                format!("Migrate to {APP_CONFIG_FILE} + {SERVICE_CONFIG_FILE}. See https://getfloo.com/docs/reference/config-spec for details."),
            ));
        }
        let service_config = load_service_config(current)?;
        let app_config = load_app_config(current)?;
        let identity = match (&service_config, &app_config) {
            (Some(service), Some(app)) if service.app.name != app.app.name => {
                return Err(FlooError::with_suggestion(
                    ErrorCode::AppNameMismatch,
                    format!("{SERVICE_CONFIG_FILE} declares '{}', but {APP_CONFIG_FILE} declares '{}' in '{}'.", service.app.name, app.app.name, current.display()),
                    "Set [app].name to the same value in both files.".to_string(),
                ));
            }
            (Some(service), _) => Some((&service.app.name, AppSource::ServiceFile)),
            (_, Some(app)) => Some((&app.app.name, AppSource::AppFile)),
            (None, None) => None,
        };
        if let Some((name, source)) = identity {
            return Ok(ResolvedApp {
                app_name: app_flag.unwrap_or(name).to_string(),
                source: app_flag.map_or(source, |_| AppSource::Flag),
                service_config,
                app_config,
                config_dir: current.to_path_buf(),
            });
        }
        // Both a repository's directory and a worktree/submodule's git file stop discovery.
        if current.join(".git").try_exists().map_err(|err| {
            FlooError::new(
                ErrorCode::FileError,
                format!(
                    "Cannot inspect git boundary in '{}': {err}",
                    current.display()
                ),
            )
        })? {
            break;
        }
    }
    if let Some(name) = app_flag {
        return Ok(ResolvedApp {
            app_name: name.to_string(),
            source: AppSource::Flag,
            service_config: None,
            app_config: None,
            config_dir: cwd.to_path_buf(),
        });
    }
    Err(FlooError::with_suggestion(
        ErrorCode::NoConfigFound,
        format!("No {SERVICE_CONFIG_FILE} or {APP_CONFIG_FILE} found."),
        format!("Run 'floo init' to create config files, or write {SERVICE_CONFIG_FILE} manually."),
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
    fn test_resolve_with_app_flag_still_loads_ancestor_configs() {
        let _guard = crate::output::GLOBAL_MODE_LOCK.lock().unwrap();
        crate::output::set_json_mode(false);
        crate::output::set_dry_run_mode(false);
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

        let child = dir.path().join("subdir");
        fs::create_dir(&child).unwrap();
        let result = resolve_app_context(&child, Some("flag-app")).unwrap();
        assert_eq!(result.config_dir, dir.path());
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
    fn test_resolve_conflicting_colocated_app_names_errors() {
        let _guard = crate::output::GLOBAL_MODE_LOCK.lock().unwrap();
        crate::output::set_json_mode(false);
        crate::output::set_dry_run_mode(false);
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

        let err = resolve_app_context(dir.path(), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::AppNameMismatch);
        assert!(err.message.contains("svc-wins"));
        assert!(err.message.contains("app-loses"));
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

[postgres]
"#,
        )
        .unwrap();

        let result = resolve_app_context(dir.path(), None).unwrap();
        assert!(result.service_config.is_some());
        assert!(result.app_config.is_some());
    }
}
