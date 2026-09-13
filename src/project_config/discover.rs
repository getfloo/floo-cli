use std::collections::HashSet;

use crate::errors::{ErrorCode, FlooError};

use super::app_config::{AppServiceEntry, AppServiceType};
use super::resolve::ResolvedApp;
use super::service_config::{
    load_service_config, ResourceConfig, ServiceConfig, ServiceIngress, ServiceType,
};

/// Resolve entries independently; explicit ports win over child files, and inline workers may omit ports.
pub fn discover_services(resolved: &ResolvedApp) -> Result<Vec<ServiceConfig>, FlooError> {
    let inline_entries = inline_service_entries(resolved);
    let delegated_entries = delegated_path_entries(resolved);

    let mut services = if !inline_entries.is_empty() {
        let global_resources = resolved
            .app_config
            .as_ref()
            .and_then(|c| c.resources.as_ref());

        let mut services = Vec::new();

        for (name, normalized_path, entry) in &inline_entries {
            let service_type = match entry.service_type {
                AppServiceType::Api => ServiceType::Api,
                AppServiceType::Web => ServiceType::Web,
                AppServiceType::Worker => ServiceType::Worker,
            };

            let ingress = entry.ingress.unwrap_or(match service_type {
                ServiceType::Worker => ServiceIngress::Internal,
                _ => ServiceIngress::Public,
            });

            if entry.resources.is_some() {
                return Err(FlooError::new(ErrorCode::InvalidProjectConfig,
                    format!("Inline service '{name}' reads resources directly from [services.{name}], not a resources sub-table.")));
            }
            let port = entry.port;

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
            let max_request_body_mb = entry
                .max_request_body_mb
                .or_else(|| global_resources.and_then(|r| r.max_request_body_mb));
            let min_instances = entry.min_instances.or_else(|| {
                (service_type != ServiceType::Worker)
                    .then(|| global_resources.and_then(|r| r.min_instances))
                    .flatten()
            });

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
                max_request_body_mb,
                min_instances,
                instances: entry.instances,
                migrate_command: entry.migrate_command.clone(),
            };

            services.push(svc);
        }

        services
    } else {
        Vec::new()
    };
    services.extend({
        if !delegated_entries.is_empty() {
            // Branch 2: Delegated mode (legacy) — read floo.service.toml from subdirs
            let mut services = Vec::new();

            // Include root floo.service.toml if present
            if let Some(ref svc_file) = resolved.service_config {
                let mut svc = svc_file.service.to_api_service_config(".");
                apply_service_file_resources(&mut svc, &svc_file.resources, None);
                services.push(svc);
            }

            for (name, normalized_path, entry) in &delegated_entries {
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

                // --app changes the target, not the identities declared on disk.
                if let Some(app_config) = resolved
                    .app_config
                    .as_ref()
                    .filter(|app| app.app.name != svc_file.app.name)
                {
                    return Err(FlooError::with_suggestion(
                        ErrorCode::AppNameMismatch,
                        format!(
                            "Service '{name}' at '{normalized_path}/{}' declares app name '{}', but {} declares '{}'.",
                            super::SERVICE_CONFIG_FILE,
                            svc_file.app.name,
                            super::APP_CONFIG_FILE,
                            app_config.app.name,
                        ),
                        format!(
                            "Set [app].name = \"{}\" in {normalized_path}/{}.",
                            app_config.app.name,
                            super::SERVICE_CONFIG_FILE,
                        ),
                    ));
                }

                if svc_file.service.name != *name {
                    return Err(FlooError::new(ErrorCode::InvalidProjectConfig,
                        format!("Service '{name}' at '{normalized_path}' declares [service].name '{}'; set it to '{name}' to match the root key.", svc_file.service.name)));
                }
                let mut svc = svc_file.service.to_api_service_config(normalized_path);
                svc.name = name.clone();

                // Apply resources from floo.service.toml [resources]
                let global_resources = resolved
                    .app_config
                    .as_ref()
                    .and_then(|c| c.resources.as_ref());
                apply_service_file_resources(&mut svc, &svc_file.resources, global_resources);

                if entry.domain.is_some() {
                    svc.domain = entry.domain.clone();
                }
                apply_app_service_overrides(&mut svc, entry)?;

                services.push(svc);
            }

            services
        } else if !services.is_empty() {
            Vec::new()
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
                    "{} has no deployable services (only floo-managed services like postgres/redis).",
                    super::APP_CONFIG_FILE,
                ),
                "Add a [services.<name>] block with type, port, and path. Run 'floo docs config' for the schema.".to_string(),
            ));
        }
    });

    let mut seen_names = HashSet::new();
    for svc in &services {
        if !seen_names.insert(&svc.name) {
            return Err(FlooError::new(
                ErrorCode::DuplicateServiceNames,
                format!("Multiple services named '{}'.", svc.name),
            ));
        }
        if svc.port.is_none() && svc.service_type != ServiceType::Worker {
            return Err(FlooError::new(
                ErrorCode::InvalidProjectConfig,
                format!(
                    "Service '{}' at '{}' is missing 'port'.",
                    svc.name, svc.path
                ),
            ));
        }
    }

    Ok(services)
}

/// Apply resources from floo.service.toml [resources], with optional global fallback.
fn apply_service_file_resources(
    svc: &mut ServiceConfig,
    svc_resources: &Option<ResourceConfig>,
    global_resources: Option<&ResourceConfig>,
) {
    let is_worker = svc.service_type == ServiceType::Worker;
    if let Some(res) = svc_resources {
        svc.cpu = res.cpu.clone();
        svc.memory = res.memory.clone();
        svc.max_instances = res.max_instances;
        svc.max_request_body_mb = res.max_request_body_mb;
        svc.min_instances = (!is_worker).then_some(res.min_instances).flatten();
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
        if svc.max_request_body_mb.is_none() {
            svc.max_request_body_mb = global.max_request_body_mb;
        }
        if !is_worker && svc.min_instances.is_none() {
            svc.min_instances = global.min_instances;
        }
    }
}

/// Apply the app-level declaration last: delegated precedence is
/// floo.app.toml service override > floo.service.toml > global [resources].
fn apply_app_service_overrides(
    svc: &mut ServiceConfig,
    declaration: &AppServiceEntry,
) -> Result<(), FlooError> {
    if declaration.cpu.is_some()
        || declaration.memory.is_some()
        || declaration.max_instances.is_some()
        || declaration.max_request_body_mb.is_some()
        || declaration.min_instances.is_some()
    {
        return Err(FlooError::new(ErrorCode::InvalidProjectConfig,
            format!("Delegated service '{}' requires resource overrides under [services.{}.resources]. Move the flat resource fields there.", svc.name, svc.name)));
    }
    if svc.service_type == ServiceType::Worker && declaration.instances.is_some() {
        svc.instances = declaration.instances;
    }
    if declaration.migrate_command.is_some() {
        svc.migrate_command = declaration.migrate_command.clone();
    }
    let Some(entry) = &declaration.resources else {
        return Ok(());
    };
    if entry.cpu.is_some() {
        svc.cpu = entry.cpu.clone();
    }
    if entry.memory.is_some() {
        svc.memory = entry.memory.clone();
    }
    if entry.max_instances.is_some() {
        svc.max_instances = entry.max_instances;
    }
    if entry.max_request_body_mb.is_some() {
        svc.max_request_body_mb = entry.max_request_body_mb;
    }
    if svc.service_type != ServiceType::Worker && entry.min_instances.is_some() {
        svc.min_instances = entry.min_instances;
    }
    Ok(())
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

/// An enabled managed service declared in floo.app.toml, ready for preflight.
pub use crate::api_types::DeclaredManagedService as ManagedServiceDeclaration;

impl ManagedServiceDeclaration {
    /// Return the credential attachment handle for this instance.
    pub fn env_handle(&self) -> String {
        if self.name == "default" {
            self.service_type.clone()
        } else {
            format!("{}:{}", self.service_type, self.name)
        }
    }
}

/// Extract enabled modern and legacy declarations in stable (type, name) order.
pub fn discover_managed_services(resolved: &ResolvedApp) -> Vec<ManagedServiceDeclaration> {
    let Some(ref app_cfg) = resolved.app_config else {
        return Vec::new();
    };

    let legacy = [
        ("postgres", &app_cfg.postgres),
        ("redis", &app_cfg.redis),
        ("storage", &app_cfg.storage),
    ]
    .into_iter()
    .filter_map(|(service_type, section)| {
        section.as_ref().map(|section| ManagedServiceDeclaration {
            service_type: service_type.to_string(),
            name: "default".to_string(),
            tier: section.tier.clone(),
        })
    });
    let modern = app_cfg
        .managed
        .iter()
        .filter(|(_, block)| block.enabled != Some(false))
        .map(|(name, block)| ManagedServiceDeclaration {
            service_type: block.service_type.clone(),
            name: name.clone(),
            tier: block.tier.clone(),
        });
    let mut result: Vec<_> = legacy.chain(modern).collect();
    result.sort_by(|a, b| (&a.service_type, &a.name).cmp(&(&b.service_type, &b.name)));
    result
}

/// Extract inline entries, giving explicit ports precedence over child files.
/// Returns (service_name, normalized_path, &AppServiceEntry) triples.
fn inline_service_entries(resolved: &ResolvedApp) -> Vec<(String, String, &AppServiceEntry)> {
    let Some(ref app_cfg) = resolved.app_config else {
        return Vec::new();
    };

    app_cfg
        .services
        .iter()
        .filter_map(|(name, entry)| {
            if !is_inline(entry, resolved) {
                return None;
            }
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

/// Extract entries whose specification lives in a child service file.
fn delegated_path_entries(resolved: &ResolvedApp) -> Vec<(String, String, &AppServiceEntry)> {
    let Some(ref app_cfg) = resolved.app_config else {
        return Vec::new();
    };

    app_cfg
        .services
        .iter()
        .filter_map(|(name, entry)| {
            if is_inline(entry, resolved) {
                return None;
            }
            let raw = entry.path.as_deref()?;
            let normalized = normalize_path(raw);
            if normalized.is_empty() || normalized == "." {
                return None;
            }
            Some((name.clone(), normalized, entry))
        })
        .collect()
}

/// Match the server's per-entry choice without reading an overridden child file.
fn is_inline(entry: &AppServiceEntry, resolved: &ResolvedApp) -> bool {
    let path = resolved
        .config_dir
        .join(entry.path.as_deref().unwrap_or("."));
    entry.port.is_some() || !path.join(super::SERVICE_CONFIG_FILE).exists()
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
            domains: Default::default(),
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
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
            },
            edge: None,
            resources: None,
            env: None,
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
        assert_eq!(services[0].port, Some(8000));
    }

    #[test]
    fn test_discover_mixed_inline_and_delegated_services() {
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
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );
        services_map.insert(
            "web".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Web,
                path: Some("./frontend".to_string()),
                dockerfile: None,
                repo: None,
                port: Some(3000),
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
        assert_eq!(api.port, Some(8000));

        let web = services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(web.path, "frontend");
        assert_eq!(web.port, Some(3000));
    }

    #[test]
    fn test_discover_includes_root_service_with_app_paths() {
        let dir = TempDir::new().unwrap();

        // Root floo.service.toml
        let root_svc = ServiceFileConfig {
            domains: Default::default(),
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
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
            },
            edge: None,
            resources: None,
            env: None,
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
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
            domains: Default::default(),
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
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
            },
            edge: None,
            resources: None,
            env: None,
        };

        use crate::project_config::app_config::ManagedServiceSection;
        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: Some(ManagedServiceSection { tier: None }),
            redis: Some(ManagedServiceSection { tier: None }),
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: HashMap::new(),
            environments: HashMap::new(),
            cron: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            Some(root_svc),
            Some(app_config),
            AppSource::ServiceFile,
        );

        // Managed services (postgres, redis) are not deployable — only the root service is
        let services = discover_services(&resolved).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "web");
    }

    #[test]
    fn test_discover_requires_port_when_no_child_file() {
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
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
    fn test_delegated_child_name_must_match_root_key() {
        let dir = TempDir::new().unwrap();

        // Root service named "api"
        let root_svc = ServiceFileConfig {
            domains: Default::default(),
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
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
            },
            edge: None,
            resources: None,
            env: None,
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
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            Some(root_svc),
            Some(app_config),
            AppSource::ServiceFile,
        );

        let err = discover_services(&resolved).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
        assert!(err.message.contains("root key"));
    }

    #[test]
    fn test_discover_errors_no_deployable_services() {
        let dir = TempDir::new().unwrap();

        use crate::project_config::app_config::ManagedServiceSection;
        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: Some(ManagedServiceSection { tier: None }),
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: HashMap::new(),
            environments: HashMap::new(),
            cron: HashMap::new(),
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
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
                port: Some(3000),
                ingress: ServiceIngress::Public,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                migrate_command: None,
            },
            ServiceConfig {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                path: "backend".to_string(),
                port: Some(8000),
                ingress: ServiceIngress::Public,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                migrate_command: None,
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
                port: Some(3000),
                ingress: ServiceIngress::Public,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                migrate_command: None,
            },
            ServiceConfig {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                path: "backend".to_string(),
                port: Some(8000),
                ingress: ServiceIngress::Public,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                migrate_command: None,
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
                port: Some(3000),
                ingress: ServiceIngress::Public,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                migrate_command: None,
            },
            ServiceConfig {
                name: "api".to_string(),
                service_type: ServiceType::Api,
                path: "backend".to_string(),
                port: Some(8000),
                ingress: ServiceIngress::Public,
                domain: None,
                cpu: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                migrate_command: None,
            },
        ];

        let err = filter_services(services, &["nonexistent".to_string()]).unwrap_err();
        assert_eq!(err.code, ErrorCode::UnknownService);
        assert!(err.message.contains("nonexistent"));
        assert!(err.suggestion.as_deref().unwrap().contains("web"));
        assert!(err.suggestion.as_deref().unwrap().contains("api"));
    }

    #[test]
    fn test_delegated_ingress_comes_from_service_toml() {
        let dir = TempDir::new().unwrap();

        // Sub-service declares ingress = "public"
        let backend = dir.path().join("backend");
        fs::create_dir(&backend).unwrap();
        fs::write(
            backend.join("floo.service.toml"),
            make_service_toml("api", "my-app", "api", 8000),
        )
        .unwrap();

        // A root service can coexist with delegated declarations
        let root_svc = ServiceFileConfig {
            domains: Default::default(),
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
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
            },
            edge: None,
            resources: None,
            env: None,
        };

        let mut services_map = HashMap::new();
        services_map.insert(
            "api".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("./backend".to_string()),
                dockerfile: None,
                repo: None,
                port: None,
                ingress: Some(ServiceIngress::Internal),
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
        assert_eq!(api.ingress, ServiceIngress::Public);
    }

    #[test]
    fn test_all_internal_multi_service_parses() {
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
type = "worker"
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
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );
        services_map.insert(
            "bg".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Worker,
                path: Some("./worker".to_string()),
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None,
                env_file: None,
                domain: Some("app.example.com".to_string()),
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
                dockerfile: None,
                repo: None,
                port: Some(8000),
                ingress: Some(ServiceIngress::Public),
                env_file: None,
                domain: None,
                cpu: Some("2".to_string()),
                resources: None,
                memory: Some("4Gi".to_string()),
                max_instances: Some(5),
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );
        services_map.insert(
            "web".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Web,
                path: Some("./frontend".to_string()),
                dockerfile: None,
                repo: None,
                port: Some(3000),
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
        assert_eq!(api.port, Some(8000));
        assert_eq!(api.cpu.as_deref(), Some("2"));
        assert_eq!(api.memory.as_deref(), Some("4Gi"));
        assert_eq!(api.max_instances, Some(5));

        let web = services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(web.path, "frontend");
        assert_eq!(web.port, Some(3000));
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
                dockerfile: None,
                repo: None,
                port: Some(8000),
                ingress: None,
                env_file: None,
                domain: None,
                resources: None,
                cpu: Some("4".to_string()), // per-service override
                memory: None,               // will inherit global
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            edge: None,
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            resources: Some(super::super::service_config::ResourceConfig {
                cpu: Some("1".to_string()),
                memory: Some("2Gi".to_string()),
                max_instances: Some(3),
                max_request_body_mb: None,
                min_instances: None,
            }),
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
    fn worker_never_inherits_global_http_minimum() {
        let mut worker = ServiceConfig {
            name: "jobs".to_string(),
            service_type: ServiceType::Worker,
            path: ".".to_string(),
            port: Some(8080),
            ingress: ServiceIngress::Internal,
            domain: None,
            cpu: None,
            memory: None,
            max_instances: None,
            max_request_body_mb: None,
            min_instances: None,
            instances: None,
            migrate_command: None,
        };
        let global = ResourceConfig {
            cpu: Some("1".to_string()),
            memory: Some("512Mi".to_string()),
            max_instances: Some(3),
            max_request_body_mb: None,
            min_instances: Some(1),
        };

        apply_service_file_resources(&mut worker, &None, Some(&global));

        assert_eq!(worker.min_instances, None);
        assert_eq!(worker.instances, None);
    }

    #[test]
    fn delegated_app_override_wins_service_file_and_global_resources() {
        let mut service = ServiceConfig {
            name: "api".to_string(),
            service_type: ServiceType::Api,
            path: "backend".to_string(),
            port: Some(8080),
            ingress: ServiceIngress::Public,
            domain: None,
            cpu: None,
            memory: None,
            max_instances: None,
            max_request_body_mb: None,
            min_instances: None,
            instances: None,
            migrate_command: None,
        };
        let global = ResourceConfig {
            cpu: Some("1".to_string()),
            memory: Some("512Mi".to_string()),
            max_instances: Some(2),
            max_request_body_mb: None,
            min_instances: Some(0),
        };
        let service_file = Some(ResourceConfig {
            cpu: Some("2".to_string()),
            memory: None,
            max_instances: Some(4),
            max_request_body_mb: None,
            min_instances: Some(1),
        });
        let app_override = AppServiceEntry {
            service_type: AppServiceType::Api,
            path: Some("backend".to_string()),
            dockerfile: None,
            repo: None,
            port: None,
            ingress: None,
            env_file: None,
            domain: None,
            cpu: None,
            resources: Some(ResourceConfig {
                cpu: Some("4".to_string()),
                memory: Some("2Gi".to_string()),
                max_instances: Some(8),
                min_instances: Some(2),
                max_request_body_mb: None,
            }),
            memory: None,
            max_instances: None,
            max_request_body_mb: None,
            min_instances: None,
            instances: None,
            dev_command: None,
            migrate_command: Some("migrate".to_string()),
            command: None,
            env: None,
        };

        apply_service_file_resources(&mut service, &service_file, Some(&global));
        apply_app_service_overrides(&mut service, &app_override).unwrap();

        assert_eq!(service.migrate_command.as_deref(), Some("migrate"));
        assert_eq!(service.cpu.as_deref(), Some("4"));
        assert_eq!(service.memory.as_deref(), Some("2Gi"));
        assert_eq!(service.max_instances, Some(8));
        assert_eq!(service.min_instances, Some(2));
    }

    #[test]
    fn delegated_worker_app_instances_override_wins_service_file() {
        let mut worker = ServiceConfig {
            name: "jobs".to_string(),
            service_type: ServiceType::Worker,
            path: "jobs".to_string(),
            port: Some(8080),
            ingress: ServiceIngress::Internal,
            domain: None,
            cpu: None,
            memory: None,
            max_instances: None,
            max_request_body_mb: None,
            min_instances: None,
            instances: Some(2),
            migrate_command: None,
        };
        let app_override = AppServiceEntry {
            service_type: AppServiceType::Worker,
            path: Some("jobs".to_string()),
            dockerfile: None,
            repo: None,
            port: None,
            ingress: None,
            env_file: None,
            domain: None,
            cpu: None,
            resources: None,
            memory: None,
            max_instances: None,
            max_request_body_mb: None,
            min_instances: None,
            instances: Some(0),
            dev_command: None,
            migrate_command: None,
            command: None,
            env: None,
        };

        apply_app_service_overrides(&mut worker, &app_override).unwrap();

        assert_eq!(worker.instances, Some(0));
    }

    #[test]
    fn test_inline_root_key_wins_over_child_file() {
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
            "renamed".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Api,
                path: Some("./backend".to_string()),
                dockerfile: None,
                repo: None,
                port: Some(8000),
                ingress: None,
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "my-app",
            None,
            Some(app_config),
            AppSource::AppFile,
        );

        let services = discover_services(&resolved).unwrap();
        assert_eq!(services[0].name, "renamed");
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
                dockerfile: None,
                repo: None,
                port: None,
                ingress: None, // should default to internal for workers
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );
        services_map.insert(
            "web".to_string(),
            AppServiceEntry {
                service_type: AppServiceType::Web,
                path: Some("./web".to_string()),
                dockerfile: None,
                repo: None,
                port: Some(3000),
                ingress: None, // should default to public
                env_file: None,
                domain: None,
                cpu: None,
                resources: None,
                memory: None,
                max_instances: None,
                max_request_body_mb: None,
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
                command: None,
                env: None,
            },
        );

        let app_config = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "my-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: services_map,
            environments: HashMap::new(),
            cron: HashMap::new(),
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
        assert_eq!(worker.port, None);
        assert_eq!(worker.ingress, ServiceIngress::Internal);

        let web = services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(web.ingress, ServiceIngress::Public);
    }

    #[test]
    fn test_single_service_with_resources() {
        let dir = TempDir::new().unwrap();

        let svc_file = ServiceFileConfig {
            edge: None,
            domains: Default::default(),
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
                min_instances: None,
                instances: None,
                dev_command: None,
                migrate_command: None,
            },
            resources: Some(super::super::service_config::ResourceConfig {
                cpu: Some("2".to_string()),
                memory: Some("4Gi".to_string()),
                max_instances: Some(5),
                max_request_body_mb: None,
                min_instances: None,
            }),
            env: None,
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
        use crate::project_config::app_config::ManagedServiceSection;
        let app_cfg = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "test-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: Some(ManagedServiceSection {
                tier: Some("hobby".to_string()),
            }),
            redis: Some(ManagedServiceSection { tier: None }),
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: {
                let mut m = HashMap::new();
                m.insert(
                    "web".to_string(),
                    AppServiceEntry {
                        service_type: AppServiceType::Web,
                        path: Some("./frontend".to_string()),
                        dockerfile: None,
                        repo: None,
                        port: Some(3000),
                        ingress: Some(ServiceIngress::Public),
                        env_file: None,
                        domain: None,
                        cpu: None,
                        resources: None,
                        memory: None,
                        max_instances: None,
                        max_request_body_mb: None,
                        min_instances: None,
                        instances: None,
                        dev_command: None,
                        migrate_command: None,
                        command: None,
                        env: None,
                    },
                );
                m
            },
            environments: HashMap::new(),
            cron: HashMap::new(),
        };

        let resolved = make_resolved(
            dir.path(),
            "test-app",
            None,
            Some(app_cfg),
            AppSource::AppFile,
        );

        let managed = discover_managed_services(&resolved);
        assert_eq!(managed.len(), 2);
        assert_eq!(managed[0].service_type, "postgres");
        assert_eq!(managed[0].name, "default");
        assert_eq!(managed[0].tier, Some("hobby".to_string()));
        assert_eq!(managed[1].service_type, "redis");
        assert_eq!(managed[1].name, "default");
        assert!(managed[1].tier.is_none());
    }

    #[test]
    fn test_discover_managed_services_empty_when_no_managed() {
        let dir = TempDir::new().unwrap();
        let app_cfg = AppFileConfig {
            domains: Default::default(),
            app: AppFileAppSection {
                name: "test-app".to_string(),
                access_mode: None,
            },
            auth: None,
            github: None,
            postgres: None,
            redis: None,
            storage: None,
            managed: HashMap::new(),
            edge: None,
            resources: None,
            services: {
                let mut m = HashMap::new();
                m.insert(
                    "web".to_string(),
                    AppServiceEntry {
                        service_type: AppServiceType::Web,
                        path: Some(".".to_string()),
                        dockerfile: None,
                        repo: None,
                        port: Some(3000),
                        ingress: Some(ServiceIngress::Public),
                        env_file: None,
                        domain: None,
                        cpu: None,
                        resources: None,
                        memory: None,
                        max_instances: None,
                        max_request_body_mb: None,
                        min_instances: None,
                        instances: None,
                        dev_command: None,
                        migrate_command: None,
                        command: None,
                        env: None,
                    },
                );
                m
            },
            environments: HashMap::new(),
            cron: HashMap::new(),
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
