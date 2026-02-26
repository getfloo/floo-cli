mod app_config;
mod resolve;
mod service_config;

pub const SERVICE_CONFIG_FILE: &str = "floo.service.toml";
pub const APP_CONFIG_FILE: &str = "floo.app.toml";
pub const LEGACY_CONFIG_FILE: &str = "floo.toml";
const SCHEMA_URL: &str = "https://getfloo.com/docs/floo-toml";
const MAX_WALK_UP_LEVELS: usize = 20;

pub use app_config::{write_app_config, AppFileAppSection, AppFileConfig};
pub use resolve::{resolve_app_context, AppSource, ResolvedApp};
pub use service_config::{
    write_service_config, ServiceConfig, ServiceFileAppSection, ServiceFileConfig, ServiceIngress,
    ServiceSection, ServiceType,
};
