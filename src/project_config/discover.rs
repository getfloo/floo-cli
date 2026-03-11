use std::collections::HashSet;

use serde::Serialize;

use crate::errors::{ErrorCode, FlooError};

use super::app_config::{AppServiceEntry, AppServiceType};
use super::resolve::ResolvedApp;
use super::service_config::{
    load_service_config, ResourceConfig, ServiceConfig, ServiceIngress, ServiceType,
};

/// Discover all deployable services from resolved config files.
///
/// Three branches:
/// 1. Inline mode: `floo.app.toml` has user-managed services with `port` fields —
///    build ServiceConfig directly from app.toml entries (no floo.service.toml in subdirs)
/// 2. Delegated mode (legacy): `floo.app.toml` has user-managed services with `path` but no `port` —
///    read each sub-service's `floo.service.toml`
/// 3. Single `floo.service.toml` only (no app_config user-managed entries) -> single service at "."
/// 4. `floo.app.toml` only with no deployable services -> error
pub fn discover_services(resolved: &ResolvedApp) -> Result<Vec<ServiceConfig>, FlooError> {
    let inline_entries = inline_service_entries(resolved);

    let services = if !inline_entries.is_empty() {
        // Branch 1: Inline mode — build ServiceConfig directly from app.toml
        // Error if floo.service.toml files exist in subdirs (enforce either/or)
        for (name, normalized_path, _) in &inline_entries {
            let sub_dir = resolved.config_dir.join(normalized_path);
            if sub_dir.join(super::SERVICE_CONFIG_FILE).exists() {
                return Err(FlooError::with_suggestion(
                    ErrorCode::InvalidProjectConfig,
                    format!(
                        "Service '{name}' is defined inline in {} but '{normalized_path}/{}' also exists.",
                        super::APP_CONFIG_FILE,
                        super::SERVICE_CONFIG_FILE,
                    ),
                    format!(
                        "Remove {normalized_path}/{} — inline services in {} don't use separate service files.",
                        super::SERVICE_CONFIG_FILE,
                        super::APP_CONFIG_FILE,
                    ),
                ));
            }
        }

        let global_resources = resolved
            .app_config
            .as_ref()
            .and_then(|c| c.resources.as_ref());

        let mut services = Vec::new();
        let mut seen_names = HashSet::new();

        for (name, normalized_path, entry) in &inline_entries {
            let service_type = match entry.service_type {
                AppServiceType::Api => ServiceType::Api,
                AppServiceType::Web => ServiceType::Web,
                AppServiceType::Worker => ServiceType::Worker,
                _ => continue,
            };

            let ingress = entry.ingress.unwrap_or(match service_type {
                ServiceType::Worker => ServiceIngress::Internal,
                _ => ServiceIngress::Public,
            });

            // port is guaranteed by validation
            let port = entry.port.unwrap();

            // Merge resources: per-service > global
            let cpu = entry
                .cpu
                .clone()
                .or_else(|| global_resources.and_then(|r| r.cpu.clone()));
            let memory = entry
                .memory
                .clone()
                .or_else(|| global_resources.and_then(|r| r.memory.clone()));
            let max_instances = entry
                .max_instances
                .or_else(|| global_resources.and_then(|r| r.max_instances));

            let svc = ServiceConfig {
                name: name.clone(),
                service_type,
                path: normalized_path.clone(),
                port,
                ingress,
                domain: entry.domain.clone(),
                cpu,
                memory,
                max_instances,
            };

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
    } else {
        let delegated_entries = delegated_path_entries(resolved);
        if !delegated_entries.is_empty() {
            // Branch 2: Delegated mode (legacy) — read floo.service.toml from subdirs
            let mut services = Vec::new();
            let mut seen_names = HashSet::new();

            // Include root floo.service.toml if present
            if let Some(ref svc_file) = resolved.service_config {
                let mut svc = svc_file.service.to_api_service_config(".");
                apply_service_file_resources(&mut svc, &svc_file.resources, None);
                seen_names.insert(svc.name.clone());
                services.push(svc);
            }

            for (name, normalized_path) in &delegated_entries {
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
                            "Create {normalized_path}/{} with [app] and [service] sections, or add port/type fields inline in {}.",
                            super::SERVICE_CONFIG_FILE,
                            super::APP_CONFIG_FILE,
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

                // Apply resources from floo.service.toml [resources]
                let global_resources = resolved
                    .app_config
                    .as_ref()
                    .and_then(|c| c.resources.as_ref());
                apply_service_file_resources(&mut svc, &svc_file.resources, global_resources);

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
            // Branch 3: single floo.service.toml only
            let mut svc = svc_file.service.to_api_service_config(".");
            apply_service_file_resources(&mut svc, &svc_file.resources, None);
            vec![svc]
        } else {
            // Branch 4: app.toml only with no deployable services
            return Err(FlooError::with_suggestion(
                ErrorCode::NoDeployableServices,
                format!(
                    "{} has no deployable services (only Floo-managed services like postgres/redis).",
                    super::APP_CONFIG_FILE,
                ),
                format!(
                    "Add a user-managed service with port/type/path fields, or create a {} in your project root.",
                    super::SERVICE_CONFIG_FILE,
                ),
            ));
        }
    };

    // Multi-service apps must have at least one public service
    if services.len() > 1 && !services.iter().any(|s| s.ingress == ServiceIngress::Public) {
        return Err(FlooError::with_suggestion(
            ErrorCode::NoPublicServices,
            "At least one service must have ingress 'public'. All services are currently set to 'internal'.",
            "Set ingress = \"public\" on at least one service in floo.app.toml or floo.service.toml.",
        ));
    }

    Ok(services)
}

/// Apply resources from floo.service.toml [resources], with optional global fallback.
fn apply_service_file_resources(
    svc: &mut ServiceConfig,
    svc_resources: &Option<ResourceConfig>,
    global_resources: Option<&ResourceConfig>,
) {
    if let Some(res) = svc_resources {
        svc.cpu = res.cpu.clone();
        svc.memory = res.memory.clone();
        svc.max_instances = res.max_instances;
    }
    // Fall back to global for any fields still None
    if let Some(global) = global_resources {
        if svc.cpu.is_none() {
            svc.cpu = global.cpu.clone();
        }
        if svc.memory.is_none() {
            svc.memory = global.memory.clone();
        }
        if svc.max_instances.is_none() {
            svc.max_instances = global.max_instances;
        }
    }
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

/// A managed service declaration from floo.app.toml (e.g. postgres, redis).
#[derive(Debug, Clone, Serialize)]
pub struct ManagedServiceDeclaration {
    pub name: String,
    #[serde(rename = "type")]
    pub service_type: AppServiceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

/// Extract managed service declarations from floo.app.toml.
/// These are services where `!is_user_managed()` (Postgres, Redis).
pub fn discover_managed_services(resolved: &ResolvedApp) -> Vec<ManagedServiceDeclaration> {
    let Some(ref app_cfg) = resolved.app_config else {
        return Vec::new();
    };

    app_cfg
        .services
        .iter()
        .filter_map(|(name, entry)| {
            if entry.service_type.is_user_managed() {
                return None;
            }
            Some(ManagedServiceDeclaration {
                name: name.clone(),
                service_type: entry.service_type.clone(),
                version: entry.version.clone(),
                tier: entry.plan.clone(),
            })
        })
        .collect()
}

/// Extract user-managed inline service entries (have `port` set) from app_config.
/// Returns (service_name, normalized_path, &AppServiceEntry) triples.
fn inline_service_entries(resolved: &ResolvedApp) -> Vec<(String, String, &AppServiceEntry)> {
    let Some(ref app_cfg) = resolved.app_config else {
        return Vec::new();
    };

    app_cfg
        .services
        .iter()
        .filter_map(|(name, entry)| {
            // Inline mode: user-managed + has port
            if !entry.service_type.is_user_managed() {
                return None;
            }
            entry.port?; // must have port for inline mode
            let raw = entry.path.as_deref().unwrap_or(".");
            let normalized = normalize_path(raw);
            let path = if normalized.is_empty() {
                ".".to_string()
            } else {
                normalized
            };
            Some((name.clone(), path, entry))
        })
        .collect()
}

/// Extract user-managed service entries with `path` but no `port` (delegated/legacy mode).
fn delegated_path_entries(resolved: &ResolvedApp) -> Vec<(String, String)> {
    let Some(ref app_cfg) = resolved.app_config else {
        return Vec::new();
    };

    // If any entry has port (inline mode), don't use delegated mode
    let has_inline = app_cfg
        .services
        .values()
        .any(|e| e.port.is_some() && e.service_type.is_user_managed());
    if has_inline {
        return Vec::new();
    }

    app_cfg
        .services
        .iter()
        .filter_map(|(name, entry)| {
            let raw = entry.path.as_deref()?;
            if !entry.service_type.is_user_managed() {
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
            resources: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
            resources: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
            resources: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
            resources: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
                cpu: None,
                memory: None,
                max_instances: None,
            },
            ServiceConfig {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                path: "backend".to_string(),
                port: 8000,
                ingress: ServiceIngress::Public,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
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
                cpu: None,
                memory: None,
                max_instances: None,
            },
            ServiceConfig {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                path: "backend".to_string(),
                port: 8000,
                ingress: ServiceIngress::Public,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
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
                cpu: None,
                memory: None,
                max_instances: None,
            },
            ServiceConfig {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                path: "backend".to_string(),
                port: 8000,
                ingress: ServiceIngress::Public,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
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
            resources: None,
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
                port: None,
                ingress: Some(ServiceIngress::Internal),
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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

    #[test]
    fn test_app_toml_domain_overrides_service_toml() {
        let dir = TempDir::new().unwrap();

        // Sub-service declares domain = "svc.example.com"
        let backend = dir.path().join("backend");
        fs::create_dir(&backend).unwrap();
        fs::write(
            backend.join("floo.service.toml"),
            r#"[app]
name = "my-app"

[service]
name = "api"
type = "api"
port = 8000
ingress = "public"
domain = "svc.example.com"
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
                port: None,
                ingress: None,
                env_file: None,
                domain: Some("app.example.com".to_string()),
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
        let api = services.iter().find(|s| s.name == "api").unwrap();
        // floo.app.toml domain should override floo.service.toml domain
        assert_eq!(api.domain.as_deref(), Some("app.example.com"));
    }

    #[test]
    fn test_service_toml_domain_preserved_when_no_app_override() {
        let dir = TempDir::new().unwrap();

        let backend = dir.path().join("backend");
        fs::create_dir(&backend).unwrap();
        fs::write(
            backend.join("floo.service.toml"),
            r#"[app]
name = "my-app"

[service]
name = "api"
type = "api"
port = 8000
ingress = "public"
domain = "svc.example.com"
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
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
        let api = services.iter().find(|s| s.name == "api").unwrap();
        // Domain from floo.service.toml should be preserved
        assert_eq!(api.domain.as_deref(), Some("svc.example.com"));
    }

    // --- Inline mode tests ---

    #[test]
    fn test_discover_inline_services_from_app_config() {
        let dir = TempDir::new().unwrap();

        let backend = dir.path().join("backend");
        let frontend = dir.path().join("frontend");
        fs::create_dir(&backend).unwrap();
        fs::create_dir(&frontend).unwrap();

        let mut services_map = HashMap::new();
        services_map.insert(
            "api".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("./backend".to_string()),
                repo: None,
                version: None,
                plan: None,
                port: Some(8000),
                ingress: Some(ServiceIngress::Public),
                env_file: None,
                domain: None,
                cpu: Some("2".to_string()),
                memory: Some("4Gi".to_string()),
                max_instances: Some(5),
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
                port: Some(3000),
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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

        let api = services.iter().find(|s| s.name == "api").unwrap();
        assert_eq!(api.path, "backend");
        assert_eq!(api.port, 8000);
        assert_eq!(api.cpu.as_deref(), Some("2"));
        assert_eq!(api.memory.as_deref(), Some("4Gi"));
        assert_eq!(api.max_instances, Some(5));

        let web = services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(web.path, "frontend");
        assert_eq!(web.port, 3000);
        assert!(web.cpu.is_none());
    }

    #[test]
    fn test_discover_inline_with_global_resources() {
        let dir = TempDir::new().unwrap();

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
                port: Some(8000),
                ingress: None,
                env_file: None,
                domain: None,
                cpu: Some("4".to_string()), // per-service override
                memory: None,               // will inherit global
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: Some(super::super::service_config::ResourceConfig {
                cpu: Some("1".to_string()),
                memory: Some("2Gi".to_string()),
                max_instances: Some(3),
            }),
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
        let api = &services[0];
        assert_eq!(api.cpu.as_deref(), Some("4")); // per-service wins
        assert_eq!(api.memory.as_deref(), Some("2Gi")); // global fallback
        assert_eq!(api.max_instances, Some(3)); // global fallback
    }

    #[test]
    fn test_discover_inline_errors_when_service_toml_exists() {
        let dir = TempDir::new().unwrap();

        let backend = dir.path().join("backend");
        fs::create_dir(&backend).unwrap();
        // Create conflicting floo.service.toml in subdir
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
                path: Some("./backend".to_string()),
                repo: None,
                version: None,
                plan: None,
                port: Some(8000),
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
        assert!(err.message.contains("inline"));
    }

    #[test]
    fn test_discover_inline_worker_defaults_internal() {
        let dir = TempDir::new().unwrap();

        let worker_dir = dir.path().join("worker");
        let web_dir = dir.path().join("web");
        fs::create_dir(&worker_dir).unwrap();
        fs::create_dir(&web_dir).unwrap();

        let mut services_map = HashMap::new();
        services_map.insert(
            "bg".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Worker,
                path: Some("./worker".to_string()),
                repo: None,
                version: None,
                plan: None,
                port: Some(9000),
                ingress: None, // should default to internal for workers
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );
        services_map.insert(
            "web".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Web,
                path: Some("./web".to_string()),
                repo: None,
                version: None,
                plan: None,
                port: Some(3000),
                ingress: None, // should default to public
                env_file: None,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
            },
        );

        let app_config = AppFileConfig {
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            resources: None,
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
        let worker = services.iter().find(|s| s.name == "bg").unwrap();
        assert_eq!(worker.ingress, ServiceIngress::Internal);

        let web = services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(web.ingress, ServiceIngress::Public);
    }

    #[test]
    fn test_single_service_with_resources() {
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
            resources: Some(super::super::service_config::ResourceConfig {
                cpu: Some("2".to_string()),
                memory: Some("4Gi".to_string()),
                max_instances: Some(5),
            }),
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
        assert_eq!(services[0].cpu.as_deref(), Some("2"));
        assert_eq!(services[0].memory.as_deref(), Some("4Gi"));
        assert_eq!(services[0].max_instances, Some(5));
    }

    // --- Managed service discovery tests ---

    #[test]
    fn test_discover_managed_services_postgres_redis() {
        let dir = TempDir::new().unwrap();
        let app_cfg = AppFileConfig {
            app: AppFileAppSection {
                name: "test-app".to_string(),
                access_mode: None,
            },
            resources: None,
            services: {
                let mut m = HashMap::new();
                m.insert(
                    "db".to_string(),
                    AppServiceEntry {
                        service_type: AppServiceType::Postgres,
                        path: None,
                        repo: None,
                        version: Some("16".to_string()),
                        plan: Some("hobby".to_string()),
                        port: None,
                        ingress: None,
                        env_file: None,
                        domain: None,
                        cpu: None,
                        memory: None,
                        max_instances: None,
                    },
                );
                m.insert(
                    "web".to_string(),
                    AppServiceEntry {
                        service_type: AppServiceType::Web,
                        path: Some("./frontend".to_string()),
                        repo: None,
                        version: None,
                        plan: None,
                        port: Some(3000),
                        ingress: Some(ServiceIngress::Public),
                        env_file: None,
                        domain: None,
                        cpu: None,
                        memory: None,
                        max_instances: None,
                    },
                );
                m
            },
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "test-app",
            None,
            Some(app_cfg),
            AppSource::AppFile,
        );

        let managed = discover_managed_services(&resolved);
        assert_eq!(managed.len(), 1);
        assert_eq!(managed[0].name, "db");
        assert_eq!(managed[0].service_type, AppServiceType::Postgres);
        assert_eq!(managed[0].version, Some("16".to_string()));
        assert_eq!(managed[0].tier, Some("hobby".to_string()));
    }

    #[test]
    fn test_discover_managed_services_empty_when_no_managed() {
        let dir = TempDir::new().unwrap();
        let app_cfg = AppFileConfig {
            app: AppFileAppSection {
                name: "test-app".to_string(),
                access_mode: None,
            },
            resources: None,
            services: {
                let mut m = HashMap::new();
                m.insert(
                    "web".to_string(),
                    AppServiceEntry {
                        service_type: AppServiceType::Web,
                        path: Some(".".to_string()),
                        repo: None,
                        version: None,
                        plan: None,
                        port: Some(3000),
                        ingress: Some(ServiceIngress::Public),
                        env_file: None,
                        domain: None,
                        cpu: None,
                        memory: None,
                        max_instances: None,
                    },
                );
                m
            },
            environments: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "test-app",
            None,
            Some(app_cfg),
            AppSource::AppFile,
        );

        let managed = discover_managed_services(&resolved);
        assert!(managed.is_empty());
    }

    #[test]
    fn test_discover_managed_services_empty_when_no_app_config() {
        let dir = TempDir::new().unwrap();
        let resolved = make_resolved(dir.path(), "test-app", None, None, AppSource::Flag);

        let managed = discover_managed_services(&resolved);
        assert!(managed.is_empty());
    }
}
