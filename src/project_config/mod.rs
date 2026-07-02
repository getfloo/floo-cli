mod app_config;
mod discover;
mod resolve;
mod service_config;

pub const SERVICE_CONFIG_FILE: &str = "floo.service.toml";
pub const APP_CONFIG_FILE: &str = "floo.app.toml";
pub const LEGACY_CONFIG_FILE: &str = "floo.toml";
const SCHEMA_URL: &str = "https://getfloo.com/docs/reference/config-spec";
const MAX_WALK_UP_LEVELS: usize = 20;

/// Convert a `toml::de::Error` from project-config parsing into a FlooError
/// with a helpful suggestion. When serde reports an `unknown field` (the
/// `deny_unknown_fields` failure mode), surface a CLI-version-skew hint:
/// the most common cause is a TOML key that exists in newer docs but the
/// locally-installed CLI doesn't recognize yet, and the previous "see schema"
/// suggestion sent users to the docs that already document the rejected key.
pub(super) fn toml_parse_error(file_label: &str, err: toml::de::Error) -> crate::errors::FlooError {
    let raw = err.to_string();
    let suggestion = match extract_unknown_field(&raw) {
        Some(field) => format!(
            "Unknown key `{field}` may require a newer CLI \
             (you're on {version}). Try `floo update` and re-run preflight. \
             If the key is correct for your installed version, see {SCHEMA_URL}.",
            version = crate::constants::VERSION,
        ),
        None => format!("See {SCHEMA_URL} for the schema reference."),
    };

    crate::errors::FlooError::with_suggestion(
        crate::errors::ErrorCode::InvalidProjectConfig,
        format!("Invalid {file_label}: {raw}"),
        suggestion,
    )
}

/// Extract the field name from the `toml` crate's "unknown field" message.
///
/// The crate emits errors like:
///   `unknown field \`access_policy\`, expected one of \`name\`, ...`
fn extract_unknown_field(msg: &str) -> Option<&str> {
    const PREFIX: &str = "unknown field `";
    let start = msg.find(PREFIX)? + PREFIX.len();
    let rest = &msg[start..];
    let end = rest.find('`')?;
    Some(&rest[..end])
}

#[cfg(test)]
pub use app_config::AppServiceType;
pub use app_config::{
    load_app_config, write_app_config_with_header, AppAccessMode, AppAgentMode, AppFileAppSection,
    AppFileConfig, AppServiceEntry, GitHubConfig, ReparoConfig,
};
pub use discover::{
    discover_managed_services, discover_services, filter_services, ManagedServiceDeclaration,
};
pub use resolve::{resolve_app_context, AppSource, ResolvedApp};
pub use service_config::{
    load_service_config, managed_env_attachment_keys, ServiceConfig, ServiceEnvContract,
    ServiceIngress, ServiceType,
};

pub fn load_service_env_contract(
    dir: &std::path::Path,
) -> Result<Option<ServiceEnvContract>, crate::errors::FlooError> {
    Ok(load_service_config(dir)?.and_then(|cfg| cfg.env))
}

/// Wire-format representation of a cron job sent to the API.
///
/// Flattens `AppFileConfig.cron` HashMap<name, CronJobConfig> into a list
/// where the name is included as a field (matching the API's `CronJobDefinition` schema).
#[derive(Debug, serde::Serialize, Clone)]
pub struct CronJobEntry {
    pub name: String,
    pub schedule: String,
    pub command: String,
    pub service: String,
    pub timeout: u32,
}

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

    #[test]
    fn test_extract_unknown_field_typical_message() {
        let msg = "TOML parse error at line 4, column 1\n  |\n4 | access_policy = \"domain\"\n  | ^^^^^^^^^^^^^\nunknown field `access_policy`, expected `name`";
        assert_eq!(extract_unknown_field(msg), Some("access_policy"));
    }

    #[test]
    fn test_extract_unknown_field_returns_none_when_no_match() {
        let msg = "TOML parse error: missing field `name`";
        assert_eq!(extract_unknown_field(msg), None);
    }

    #[test]
    fn test_toml_parse_error_unknown_field_suggests_update() {
        // Synthesize the same path real callers hit.
        #[derive(serde::Deserialize, Debug)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct Probe {
            name: String,
        }
        let err = toml::from_str::<Probe>("name = \"x\"\nfoo = 1\n").unwrap_err();
        let floo_err = toml_parse_error("floo.app.toml", err);
        let suggestion = floo_err
            .suggestion
            .expect("unknown-field path always sets a suggestion");
        assert!(suggestion.contains("`foo`"), "got: {suggestion}");
        assert!(suggestion.contains("floo update"), "got: {suggestion}");
        // Falls back to schema link when the user is on a current CLI.
        assert!(suggestion.contains(SCHEMA_URL), "got: {suggestion}");
    }

    #[test]
    fn test_toml_parse_error_non_unknown_field_uses_schema_url() {
        // A non-`unknown field` parse error (here: type mismatch) should fall
        // back to the schema-reference suggestion. Sending users to
        // `floo update` for a typo would be misleading.
        #[derive(serde::Deserialize, Debug)]
        #[allow(dead_code)]
        struct Probe {
            count: u32,
        }
        let err = toml::from_str::<Probe>("count = \"not a number\"\n").unwrap_err();
        let floo_err = toml_parse_error("floo.app.toml", err);
        let suggestion = floo_err
            .suggestion
            .expect("non-unknown-field path still sets a suggestion");
        assert!(suggestion.contains(SCHEMA_URL), "got: {suggestion}");
        assert!(
            !suggestion.contains("floo update"),
            "should not suggest update for non-schema-skew errors, got: {suggestion}"
        );
    }
}
