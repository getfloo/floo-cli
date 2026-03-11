mod app_config;
mod discover;
mod resolve;
mod service_config;

pub const SERVICE_CONFIG_FILE: &str = "floo.service.toml";
pub const APP_CONFIG_FILE: &str = "floo.app.toml";
pub const LEGACY_CONFIG_FILE: &str = "floo.toml";
const SCHEMA_URL: &str = "https://getfloo.com/docs/floo-toml";
const MAX_WALK_UP_LEVELS: usize = 20;

pub use app_config::{
    load_app_config, write_app_config, AppAccessMode, AppFileAppSection, AppFileConfig,
    AppServiceEntry, AppServiceType,
};
pub use discover::{discover_managed_services, discover_services, filter_services};
pub use resolve::{resolve_app_context, AppSource, ResolvedApp};
pub use service_config::{
    load_service_config, write_service_config, ServiceConfig, ServiceFileAppSection,
    ServiceFileConfig, ServiceIngress, ServiceSection, ServiceType,
};

/// Validate a service name for DNS-label compatibility.
///
/// Rules: lowercase ASCII, digits, hyphens; must start with a letter,
/// end with a letter or digit; 2–21 chars. No regex crate needed.
pub fn validate_service_name(name: &str) -> Result<(), String> {
    if name.len() < 2 || name.len() > 21 {
        return Err(format!(
            "Service name '{name}' must be between 2 and 21 characters."
        ));
    }

    let bytes = name.as_bytes();

    if !bytes[0].is_ascii_lowercase() {
        return Err(format!(
            "Service name '{name}' must start with a lowercase letter."
        ));
    }

    let last = bytes[bytes.len() - 1];
    if !last.is_ascii_lowercase() && !last.is_ascii_digit() {
        return Err(format!(
            "Service name '{name}' must end with a lowercase letter or digit."
        ));
    }

    for &b in bytes {
        if !b.is_ascii_lowercase() && !b.is_ascii_digit() && b != b'-' {
            return Err(format!(
                "Service name '{name}' contains invalid character '{}'. Use lowercase letters, digits, and hyphens only.",
                b as char
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_service_name_valid() {
        assert!(validate_service_name("web").is_ok());
        assert!(validate_service_name("api").is_ok());
        assert!(validate_service_name("my-service-1").is_ok());
        assert!(validate_service_name("ab").is_ok());
        assert!(validate_service_name("a-long-but-valid-name").is_ok());
    }

    #[test]
    fn test_validate_service_name_uppercase_rejected() {
        assert!(validate_service_name("Web").is_err());
        assert!(validate_service_name("API").is_err());
    }

    #[test]
    fn test_validate_service_name_start_with_letter() {
        assert!(validate_service_name("1abc").is_err());
        assert!(validate_service_name("-abc").is_err());
    }

    #[test]
    fn test_validate_service_name_end_with_letter_or_digit() {
        assert!(validate_service_name("abc-").is_err());
    }

    #[test]
    fn test_validate_service_name_too_short() {
        assert!(validate_service_name("a").is_err());
    }

    #[test]
    fn test_validate_service_name_too_long() {
        assert!(validate_service_name("abcdefghijklmnopqrstuv").is_err()); // 22 chars
    }

    #[test]
    fn test_validate_service_name_special_chars() {
        assert!(validate_service_name("my_service").is_err());
        assert!(validate_service_name("my.service").is_err());
        assert!(validate_service_name("my service").is_err());
    }
}
