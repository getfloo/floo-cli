use std::collections::HashSet;

use crate::errors::{ErrorCode, FlooError};

use super::app_config::AppServiceType;
use super::resolve::ResolvedApp;
use super::service_config::{load_service_config, ServiceConfig, ServiceIngress};

/// Discover all deployable services from resolved config files.
///
/// Three branches:
/// 1. `floo.app.toml` has user-managed services with `path` entries -> read each sub-service's
///    `floo.service.toml` and include root service if present
/// 2. Single `floo.service.toml` only (no app_config path entries) -> single service at "."
/// 3. `floo.app.toml` only with no deployable services -> error
pub fn discover_services(resolved: &ResolvedApp) -> Result<Vec<ServiceConfig>, FlooError> {
    let path_entries = user_managed_path_entries(resolved);

    let services = if !path_entries.is_empty() {
        // Branch 1: app.toml has user-managed services with path fields
        let mut services = Vec::new();
        let mut seen_names = HashSet::new();

        // Include root floo.service.toml if present
        if let Some(ref svc_file) = resolved.service_config {
            let svc = svc_file.service.to_api_service_config(".");
            seen_names.insert(svc.name.clone());
            services.push(svc);
        }

        for (name, normalized_path) in &path_entries {
            let sub_dir = resolved.config_dir.join(normalized_path);
            let svc_file = load_service_config(&sub_dir)?.ok_or_else(|| {
                FlooError::with_suggestion(
                    ErrorCode::ServiceConfigMissing,
                    format!(
                        "No {} found at '{normalized_path}/' (declared as service '{name}' in {}).",
                        super::SERVICE_CONFIG_FILE,
                        super::APP_CONFIG_FILE,
                    ),
                    format!(
                        "Create {normalized_path}/{} with [app] and [service] sections.",
                        super::SERVICE_CONFIG_FILE,
                    ),
                )
            })?;

            if svc_file.app.name != resolved.app_name {
                return Err(FlooError::with_suggestion(
                    ErrorCode::AppNameMismatch,
                    format!(
                        "Service '{name}' at '{normalized_path}/{}' declares app name '{}', but {} declares '{}'.",
                        super::SERVICE_CONFIG_FILE,
                        svc_file.app.name,
                        super::APP_CONFIG_FILE,
                        resolved.app_name,
                    ),
                    format!(
                        "Set [app].name = \"{}\" in {normalized_path}/{}.",
                        resolved.app_name,
                        super::SERVICE_CONFIG_FILE,
                    ),
                ));
            }

            let mut svc = svc_file.service.to_api_service_config(normalized_path);

            // Let floo.app.toml override floo.service.toml values
            if let Some(ref app_cfg) = resolved.app_config {
                if let Some(entry) = app_cfg.services.get(name) {
                    if let Some(override_ingress) = entry.ingress {
                        svc.ingress = override_ingress;
                    }
                    if entry.domain.is_some() {
                        svc.domain = entry.domain.clone();
                    }
                }
            }

            if !seen_names.insert(svc.name.clone()) {
                return Err(FlooError::new(
                    ErrorCode::DuplicateServiceNames,
                    format!(
                        "Multiple services named '{}'. Service names must be unique.",
                        svc.name,
                    ),
                ));
            }

            services.push(svc);
        }

        services
    } else if let Some(ref svc_file) = resolved.service_config {
        // Branch 2: single floo.service.toml only
        vec![svc_file.service.to_api_service_config(".")]
    } else {
        // Branch 3: app.toml only with no deployable services
        return Err(FlooError::with_suggestion(
            ErrorCode::NoDeployableServices,
            format!(
                "{} has no deployable services (only Floo-managed services like postgres/redis).",
                super::APP_CONFIG_FILE,
            ),
            format!(
                "Add a user-managed service with a 'path' field, or create a {} in your project root.",
                super::SERVICE_CONFIG_FILE,
            ),
        ));
    };

    // Multi-service apps must have at least one public service
    if services.len() > 1 && !services.iter().any(|s| s.ingress == ServiceIngress::Public) {
        return Err(FlooError::with_suggestion(
            ErrorCode::NoPublicServices,
            "At least one service must have ingress 'public'. All services are currently set to 'internal'.",
            "Set ingress = \"public\" on at least one service in its floo.service.toml, or override it in floo.app.toml.",
        ));
    }

    Ok(services)
}

/// Filter services by name. Empty filter returns all.
pub fn filter_services(
    services: Vec<ServiceConfig>,
    filter: &[String],
) -> Result<Vec<ServiceConfig>, FlooError> {
    if filter.is_empty() {
        return Ok(services);
    }

    let available: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();

    for name in filter {
        if !available.contains(&name.as_str()) {
            return Err(FlooError::with_suggestion(
                ErrorCode::UnknownService,
                format!("Unknown service '{name}'."),
                format!("Available services: {}", available.join(", ")),
            ));
        }
    }

    let filter_set: HashSet<&str> = filter.iter().map(|s| s.as_str()).collect();
    Ok(services
        .into_iter()
        .filter(|s| filter_set.contains(s.name.as_str()))
        .collect())
}

/// Extract user-managed service entries with `path` fields from app_config, returning
/// (service_name, normalized_path) pairs.
fn user_managed_path_entries(resolved: &ResolvedApp) -> Vec<(String, String)> {
    let Some(ref app_cfg) = resolved.app_config else {
        return Vec::new();
    };

    app_cfg
        .services
        .iter()
        .filter_map(|(name, entry)| {
            let raw = entry.path.as_deref()?;
            if !matches!(
                entry.service_type,
                AppServiceType::Web | AppServiceType::Api | AppServiceType::Worker
            ) {
                return None;
            }
            let normalized = normalize_path(raw);
            if normalized.is_empty() || normalized == "." {
                return None;
            }
            Some((name.clone(), normalized))
        })
        .collect()
}

/// Normalize a relative path: strip leading `./` and trailing `/`.
fn normalize_path(p: &str) -> String {
    let s = p.strip_prefix("./").unwrap_or(p);
    s.strip_suffix('/').unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_config::app_config::{AppFileAppSection, AppFileConfig, AppServiceEntry};
    use crate::project_config::resolve::AppSource;
    use crate::project_config::service_config::{
        ServiceFileAppSection, ServiceFileConfig, ServiceIngress, ServiceSection, ServiceType,
    };
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    fn make_service_toml(name: &str, app_name: &str, svc_type: &str, port: u16) -> String {
        format!(
            r#"[app]
name = "{app_name}"

[service]
name = "{name}"
type = "{svc_type}"
port = {port}
ingress = "public"
"#
        )
    }

    fn make_resolved(
        dir: &std::path::Path,
        app_name: &str,
        service_config: Option<ServiceFileConfig>,
        app_config: Option<AppFileConfig>,
        source: AppSource,
    ) -> ResolvedApp {
        ResolvedApp {
            app_name: app_name.to_string(),
            source,
            service_config,
            app_config,
            config_dir: dir.to_path_buf(),
        }
    }

    #[test]
    fn test_discover_single_service_from_service_file() {
        let dir = TempDir::new().unwrap();
        let svc_file = ServiceFileConfig {
            app: ServiceFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            service: ServiceSection {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                port: 8000,
                ingress: Some(ServiceIngress::Public),
                env_file: None,
                domain: None,
            },
        };
        let resolved = make_resolved(
            dir.path(),
            "my-app",
            Some(svc_file),
            None,
            AppSource::ServiceFile,
        );

        let services = discover_services(&resolved).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "api");
        assert_eq!(services[0].path, ".");
        assert_eq!(services[0].port, 8000);
    }

    #[test]
    fn test_discover_multi_service_from_app_config_paths() {
        let dir = TempDir::new().unwrap();

        // Create subdirs with floo.service.toml
        let backend = dir.path().join("backend");
        let frontend = dir.path().join("frontend");
        fs::create_dir(&backend).unwrap();
        fs::create_dir(&frontend).unwrap();
        fs::write(
            backend.join("floo.service.toml"),
            make_service_toml("api", "my-app", "api", 8000),
        )
        .unwrap();
        fs::write(
            frontend.join("floo.service.toml"),
            make_service_toml("web", "my-app", "web", 3000),
        )
        .unwrap();

        let mut services_map = HashMap::new();
        services_map.insert(
            "api".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("./backend".to_string()),
                repo: None,
                version: None,
                plan: None,
                ingress: None,
                domain: None,
            },
        );
        services_map.insert(
            "web".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Web,
                path: Some("./frontend".to_string()),
                repo: None,
                version: None,
                plan: None,
                ingress: None,
                domain: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            services: services_map,
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            None,
            Some(app_config),
            AppSource::AppFile,
        );

        let services = discover_services(&resolved).unwrap();
        assert_eq!(services.len(), 2);

        let names: HashSet<&str> = services.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains("api"));
        assert!(names.contains("web"));

        let api = services.iter().find(|s| s.name == "api").unwrap();
        assert_eq!(api.path, "backend");
        assert_eq!(api.port, 8000);

        let web = services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(web.path, "frontend");
        assert_eq!(web.port, 3000);
    }

    #[test]
    fn test_discover_includes_root_service_with_app_paths() {
        let dir = TempDir::new().unwrap();

        // Root floo.service.toml
        let root_svc = ServiceFileConfig {
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
                domain: None,
            },
        };

        // Sub-service
        let backend = dir.path().join("backend");
        fs::create_dir(&backend).unwrap();
        fs::write(
            backend.join("floo.service.toml"),
            make_service_toml("api", "my-app", "api", 8000),
        )
        .unwrap();

        let mut services_map = HashMap::new();
        services_map.insert(
            "api".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("backend".to_string()),
                repo: None,
                version: None,
                plan: None,
                ingress: None,
                domain: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            services: services_map,
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            Some(root_svc),
            Some(app_config),
            AppSource::ServiceFile,
        );

        let services = discover_services(&resolved).unwrap();
        assert_eq!(services.len(), 2);

        let names: HashSet<&str> = services.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains("web"));
        assert!(names.contains("api"));

        let root = services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(root.path, ".");
    }

    #[test]
    fn test_discover_skips_floo_managed_services() {
        let dir = TempDir::new().unwrap();

        // floo.service.toml at root for the deployable service
        let root_svc = ServiceFileConfig {
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
                domain: None,
            },
        };

        let mut services_map = HashMap::new();
        services_map.insert(
            "db".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Postgres,
                path: None,
                repo: None,
                version: Some("16".to_string()),
                plan: None,
                ingress: None,
                domain: None,
            },
        );
        services_map.insert(
            "cache".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Redis,
                path: None,
                repo: None,
                version: None,
                plan: None,
                ingress: None,
                domain: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            services: services_map,
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            Some(root_svc),
            Some(app_config),
            AppSource::ServiceFile,
        );

        // No path entries -> falls to branch 2 (single service)
        let services = discover_services(&resolved).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "web");
    }

    #[test]
    fn test_discover_errors_on_missing_service_toml() {
        let dir = TempDir::new().unwrap();

        // Create subdir but don't put floo.service.toml in it
        let backend = dir.path().join("backend");
        fs::create_dir(&backend).unwrap();

        let mut services_map = HashMap::new();
        services_map.insert(
            "api".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("./backend".to_string()),
                repo: None,
                version: None,
                plan: None,
                ingress: None,
                domain: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            services: services_map,
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            None,
            Some(app_config),
            AppSource::AppFile,
        );

        let err = discover_services(&resolved).unwrap_err();
        assert_eq!(err.code, ErrorCode::ServiceConfigMissing);
        assert!(err.message.contains("backend"));
    }

    #[test]
    fn test_discover_errors_on_app_name_mismatch() {
        let dir = TempDir::new().unwrap();

        let backend = dir.path().join("backend");
        fs::create_dir(&backend).unwrap();
        fs::write(
            backend.join("floo.service.toml"),
            make_service_toml("api", "wrong-app", "api", 8000),
        )
        .unwrap();

        let mut services_map = HashMap::new();
        services_map.insert(
            "api".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("backend".to_string()),
                repo: None,
                version: None,
                plan: None,
                ingress: None,
                domain: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            services: services_map,
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            None,
            Some(app_config),
            AppSource::AppFile,
        );

        let err = discover_services(&resolved).unwrap_err();
        assert_eq!(err.code, ErrorCode::AppNameMismatch);
        assert!(err.message.contains("wrong-app"));
        assert!(err.message.contains("my-app"));
    }

    #[test]
    fn test_discover_errors_on_duplicate_names() {
        let dir = TempDir::new().unwrap();

        // Root service named "api"
        let root_svc = ServiceFileConfig {
            app: ServiceFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            service: ServiceSection {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                port: 8000,
                ingress: Some(ServiceIngress::Public),
                env_file: None,
                domain: None,
            },
        };

        // Sub-service also named "api"
        let backend = dir.path().join("backend");
        fs::create_dir(&backend).unwrap();
        fs::write(
            backend.join("floo.service.toml"),
            make_service_toml("api", "my-app", "api", 9000),
        )
        .unwrap();

        let mut services_map = HashMap::new();
        services_map.insert(
            "api-svc".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("backend".to_string()),
                repo: None,
                version: None,
                plan: None,
                ingress: None,
                domain: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            services: services_map,
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            Some(root_svc),
            Some(app_config),
            AppSource::ServiceFile,
        );

        let err = discover_services(&resolved).unwrap_err();
        assert_eq!(err.code, ErrorCode::DuplicateServiceNames);
        assert!(err.message.contains("api"));
    }

    #[test]
    fn test_discover_errors_no_deployable_services() {
        let dir = TempDir::new().unwrap();

        let mut services_map = HashMap::new();
        services_map.insert(
            "db".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Postgres,
                path: None,
                repo: None,
                version: Some("16".to_string()),
                plan: None,
                ingress: None,
                domain: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            services: services_map,
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            None,
            Some(app_config),
            AppSource::AppFile,
        );

        let err = discover_services(&resolved).unwrap_err();
        assert_eq!(err.code, ErrorCode::NoDeployableServices);
    }

    #[test]
    fn test_discover_normalizes_dot_slash_paths() {
        let dir = TempDir::new().unwrap();

        let backend = dir.path().join("backend");
        fs::create_dir(&backend).unwrap();
        fs::write(
            backend.join("floo.service.toml"),
            make_service_toml("api", "my-app", "api", 8000),
        )
        .unwrap();

        let mut services_map = HashMap::new();
        services_map.insert(
            "api".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("./backend/".to_string()),
                repo: None,
                version: None,
                plan: None,
                ingress: None,
                domain: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            services: services_map,
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            None,
            Some(app_config),
            AppSource::AppFile,
        );

        let services = discover_services(&resolved).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].path, "backend");
    }

    #[test]
    fn test_filter_empty_returns_all() {
        let services = vec![
            ServiceConfig {
                name: "web".to_string(),
                service_type: ServiceType::Web,
                path: "frontend".to_string(),
                port: 3000,
                ingress: ServiceIngress::Public,
                domain: None,
            },
            ServiceConfig {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                path: "backend".to_string(),
                port: 8000,
                ingress: ServiceIngress::Public,
                domain: None,
            },
        ];

        let result = filter_services(services, &[]).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_filter_valid_subset() {
        let services = vec![
            ServiceConfig {
                name: "web".to_string(),
                service_type: ServiceType::Web,
                path: "frontend".to_string(),
                port: 3000,
                ingress: ServiceIngress::Public,
                domain: None,
            },
            ServiceConfig {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                path: "backend".to_string(),
                port: 8000,
                ingress: ServiceIngress::Public,
                domain: None,
            },
        ];

        let result = filter_services(services, &["api".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "api");
    }

    #[test]
    fn test_filter_unknown_name_errors() {
        let services = vec![
            ServiceConfig {
                name: "web".to_string(),
                service_type: ServiceType::Web,
                path: "frontend".to_string(),
                port: 3000,
                ingress: ServiceIngress::Public,
                domain: None,
            },
            ServiceConfig {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                path: "backend".to_string(),
                port: 8000,
                ingress: ServiceIngress::Public,
                domain: None,
            },
        ];

        let err = filter_services(services, &["nonexistent".to_string()]).unwrap_err();
        assert_eq!(err.code, ErrorCode::UnknownService);
        assert!(err.message.contains("nonexistent"));
        assert!(err.suggestion.as_deref().unwrap().contains("web"));
        assert!(err.suggestion.as_deref().unwrap().contains("api"));
    }

    #[test]
    fn test_app_toml_ingress_overrides_service_toml() {
        let dir = TempDir::new().unwrap();

        // Sub-service declares ingress = "public"
        let backend = dir.path().join("backend");
        fs::create_dir(&backend).unwrap();
        fs::write(
            backend.join("floo.service.toml"),
            make_service_toml("api", "my-app", "api", 8000),
        )
        .unwrap();

        // Root service to satisfy at-least-one-public
        let root_svc = ServiceFileConfig {
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
                domain: None,
            },
        };

        let mut services_map = HashMap::new();
        services_map.insert(
            "api".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("./backend".to_string()),
                repo: None,
                version: None,
                plan: None,
                ingress: Some(ServiceIngress::Internal),
                domain: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            services: services_map,
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            Some(root_svc),
            Some(app_config),
            AppSource::ServiceFile,
        );

        let services = discover_services(&resolved).unwrap();
        let api = services.iter().find(|s| s.name == "api").unwrap();
        assert_eq!(api.ingress, ServiceIngress::Internal);
    }

    #[test]
    fn test_all_internal_multi_service_errors() {
        let dir = TempDir::new().unwrap();

        let backend = dir.path().join("backend");
        let worker_dir = dir.path().join("worker");
        fs::create_dir(&backend).unwrap();
        fs::create_dir(&worker_dir).unwrap();

        // Both services declare internal ingress via TOML
        fs::write(
            backend.join("floo.service.toml"),
            r#"[app]
name = "my-app"

[service]
name = "api"
type = "api"
port = 8000
ingress = "internal"
"#,
        )
        .unwrap();
        fs::write(
            worker_dir.join("floo.service.toml"),
            r#"[app]
name = "my-app"

[service]
name = "bg"
type = "worker"
port = 9000
ingress = "internal"
"#,
        )
        .unwrap();

        let mut services_map = HashMap::new();
        services_map.insert(
            "api".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("./backend".to_string()),
                repo: None,
                version: None,
                plan: None,
                ingress: None,
                domain: None,
            },
        );
        services_map.insert(
            "bg".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Worker,
                path: Some("./worker".to_string()),
                repo: None,
                version: None,
                plan: None,
                ingress: None,
                domain: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            services: services_map,
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            None,
            Some(app_config),
            AppSource::AppFile,
        );

        let err = discover_services(&resolved).unwrap_err();
        assert_eq!(err.code, ErrorCode::NoPublicServices);
    }
}
