use thiserror::Error;

#[derive(Clone, Debug, PartialEq)]
pub enum ErrorCode {
    AppNameMismatch,
    AppNotFound,
    ChecksumMismatch,
    ChecksumMissing,
    ChecksumParseError,
    ConfigError,
    ConfigExists,
    ConfigInvalid,
    ConfigWriteError,
    CwdError,
    DatabaseNotFound,
    DeployFailed,
    DeployNotFound,
    DeployTimeout,
    DeviceAuthDenied,
    DeviceCodeExpired,
    DownloadFailed,
    DuplicateService,
    DuplicateServiceNames,
    EmailTaken,
    EnvFileNotFound,
    EnvParseError,
    FileError,
    InternalError,
    InvalidAmount,
    InvalidFormat,
    InvalidIngress,
    InvalidPath,
    InvalidProjectConfig,
    InvalidResponse,
    InvalidRole,
    InvalidServiceName,
    InvalidType,
    LegacyConfig,
    MissingAppName,
    MissingArgument,
    MissingPort,
    MissingType,
    MultipleServices,
    MultipleServicesNoTarget,
    NoConfigFound,
    NoDeployableServices,
    NoEnvFiles,
    NoPublicServices,
    NoRuntimeDetected,
    NotAuthenticated,
    ParseError,
    ReleaseAssetMissing,
    ReleaseLookupFailed,
    ReleaseNotFound,
    ReleaseParseError,
    RestartFailed,
    ServiceConfigMissing,
    ServiceNotFound,
    SignupDisabled,
    StreamError,
    UnknownService,
    UnsupportedPlatform,
    UpdateHttpClientError,
    UpdateInstallFailed,
    UpdateInstallPathUnresolved,
    UpdatePermissionDenied,
    Other(String),
}

impl ErrorCode {
    pub fn as_str(&self) -> &str {
        match self {
            ErrorCode::AppNameMismatch => "APP_NAME_MISMATCH",
            ErrorCode::AppNotFound => "APP_NOT_FOUND",
            ErrorCode::ChecksumMismatch => "CHECKSUM_MISMATCH",
            ErrorCode::ChecksumMissing => "CHECKSUM_MISSING",
            ErrorCode::ChecksumParseError => "CHECKSUM_PARSE_ERROR",
            ErrorCode::ConfigError => "CONFIG_ERROR",
            ErrorCode::ConfigExists => "CONFIG_EXISTS",
            ErrorCode::ConfigInvalid => "CONFIG_INVALID",
            ErrorCode::ConfigWriteError => "CONFIG_WRITE_ERROR",
            ErrorCode::CwdError => "CWD_ERROR",
            ErrorCode::DatabaseNotFound => "DATABASE_NOT_FOUND",
            ErrorCode::DeployFailed => "DEPLOY_FAILED",
            ErrorCode::DeployNotFound => "DEPLOY_NOT_FOUND",
            ErrorCode::DeployTimeout => "DEPLOY_TIMEOUT",
            ErrorCode::DeviceAuthDenied => "DEVICE_AUTH_DENIED",
            ErrorCode::DeviceCodeExpired => "DEVICE_CODE_EXPIRED",
            ErrorCode::DownloadFailed => "DOWNLOAD_FAILED",
            ErrorCode::DuplicateService => "DUPLICATE_SERVICE",
            ErrorCode::DuplicateServiceNames => "DUPLICATE_SERVICE_NAMES",
            ErrorCode::EmailTaken => "EMAIL_TAKEN",
            ErrorCode::EnvFileNotFound => "ENV_FILE_NOT_FOUND",
            ErrorCode::EnvParseError => "ENV_PARSE_ERROR",
            ErrorCode::FileError => "FILE_ERROR",
            ErrorCode::InternalError => "INTERNAL_ERROR",
            ErrorCode::InvalidAmount => "INVALID_AMOUNT",
            ErrorCode::InvalidFormat => "INVALID_FORMAT",
            ErrorCode::InvalidIngress => "INVALID_INGRESS",
            ErrorCode::InvalidPath => "INVALID_PATH",
            ErrorCode::InvalidProjectConfig => "INVALID_PROJECT_CONFIG",
            ErrorCode::InvalidResponse => "INVALID_RESPONSE",
            ErrorCode::InvalidRole => "INVALID_ROLE",
            ErrorCode::InvalidServiceName => "INVALID_SERVICE_NAME",
            ErrorCode::InvalidType => "INVALID_TYPE",
            ErrorCode::LegacyConfig => "LEGACY_CONFIG",
            ErrorCode::MissingAppName => "MISSING_APP_NAME",
            ErrorCode::MissingArgument => "MISSING_ARGUMENT",
            ErrorCode::MissingPort => "MISSING_PORT",
            ErrorCode::MissingType => "MISSING_TYPE",
            ErrorCode::MultipleServices => "MULTIPLE_SERVICES",
            ErrorCode::MultipleServicesNoTarget => "MULTIPLE_SERVICES_NO_TARGET",
            ErrorCode::NoConfigFound => "NO_CONFIG_FOUND",
            ErrorCode::NoDeployableServices => "NO_DEPLOYABLE_SERVICES",
            ErrorCode::NoEnvFiles => "NO_ENV_FILES",
            ErrorCode::NoPublicServices => "NO_PUBLIC_SERVICES",
            ErrorCode::NoRuntimeDetected => "NO_RUNTIME_DETECTED",
            ErrorCode::NotAuthenticated => "NOT_AUTHENTICATED",
            ErrorCode::ParseError => "PARSE_ERROR",
            ErrorCode::ReleaseAssetMissing => "RELEASE_ASSET_MISSING",
            ErrorCode::ReleaseLookupFailed => "RELEASE_LOOKUP_FAILED",
            ErrorCode::ReleaseNotFound => "RELEASE_NOT_FOUND",
            ErrorCode::ReleaseParseError => "RELEASE_PARSE_ERROR",
            ErrorCode::RestartFailed => "RESTART_FAILED",
            ErrorCode::ServiceConfigMissing => "SERVICE_CONFIG_MISSING",
            ErrorCode::ServiceNotFound => "SERVICE_NOT_FOUND",
            ErrorCode::SignupDisabled => "SIGNUP_DISABLED",
            ErrorCode::StreamError => "STREAM_ERROR",
            ErrorCode::UnknownService => "UNKNOWN_SERVICE",
            ErrorCode::UnsupportedPlatform => "UNSUPPORTED_PLATFORM",
            ErrorCode::UpdateHttpClientError => "UPDATE_HTTP_CLIENT_ERROR",
            ErrorCode::UpdateInstallFailed => "UPDATE_INSTALL_FAILED",
            ErrorCode::UpdateInstallPathUnresolved => "UPDATE_INSTALL_PATH_UNRESOLVED",
            ErrorCode::UpdatePermissionDenied => "UPDATE_PERMISSION_DENIED",
            ErrorCode::Other(s) => s.as_str(),
        }
    }

    /// Convert an API-sourced string code to ErrorCode.
    /// Unknown codes become Other(s.to_string()).
    pub fn from_api(s: &str) -> Self {
        match s {
            "APP_NAME_MISMATCH" => ErrorCode::AppNameMismatch,
            "APP_NOT_FOUND" => ErrorCode::AppNotFound,
            "CHECKSUM_MISMATCH" => ErrorCode::ChecksumMismatch,
            "CHECKSUM_MISSING" => ErrorCode::ChecksumMissing,
            "CHECKSUM_PARSE_ERROR" => ErrorCode::ChecksumParseError,
            "CONFIG_ERROR" => ErrorCode::ConfigError,
            "CONFIG_EXISTS" => ErrorCode::ConfigExists,
            "CONFIG_INVALID" => ErrorCode::ConfigInvalid,
            "CONFIG_WRITE_ERROR" => ErrorCode::ConfigWriteError,
            "CWD_ERROR" => ErrorCode::CwdError,
            "DATABASE_NOT_FOUND" => ErrorCode::DatabaseNotFound,
            "DEPLOY_FAILED" => ErrorCode::DeployFailed,
            "DEPLOY_NOT_FOUND" => ErrorCode::DeployNotFound,
            "DEPLOY_TIMEOUT" => ErrorCode::DeployTimeout,
            "DEVICE_AUTH_DENIED" => ErrorCode::DeviceAuthDenied,
            "DEVICE_CODE_EXPIRED" => ErrorCode::DeviceCodeExpired,
            "DOWNLOAD_FAILED" => ErrorCode::DownloadFailed,
            "DUPLICATE_SERVICE" => ErrorCode::DuplicateService,
            "DUPLICATE_SERVICE_NAMES" => ErrorCode::DuplicateServiceNames,
            "EMAIL_TAKEN" => ErrorCode::EmailTaken,
            "ENV_FILE_NOT_FOUND" => ErrorCode::EnvFileNotFound,
            "ENV_PARSE_ERROR" => ErrorCode::EnvParseError,
            "FILE_ERROR" => ErrorCode::FileError,
            "INTERNAL_ERROR" => ErrorCode::InternalError,
            "INVALID_AMOUNT" => ErrorCode::InvalidAmount,
            "INVALID_FORMAT" => ErrorCode::InvalidFormat,
            "INVALID_INGRESS" => ErrorCode::InvalidIngress,
            "INVALID_PATH" => ErrorCode::InvalidPath,
            "INVALID_PROJECT_CONFIG" => ErrorCode::InvalidProjectConfig,
            "INVALID_RESPONSE" => ErrorCode::InvalidResponse,
            "INVALID_ROLE" => ErrorCode::InvalidRole,
            "INVALID_SERVICE_NAME" => ErrorCode::InvalidServiceName,
            "INVALID_TYPE" => ErrorCode::InvalidType,
            "LEGACY_CONFIG" => ErrorCode::LegacyConfig,
            "MISSING_APP_NAME" => ErrorCode::MissingAppName,
            "MISSING_ARGUMENT" => ErrorCode::MissingArgument,
            "MISSING_PORT" => ErrorCode::MissingPort,
            "MISSING_TYPE" => ErrorCode::MissingType,
            "MULTIPLE_SERVICES" => ErrorCode::MultipleServices,
            "MULTIPLE_SERVICES_NO_TARGET" => ErrorCode::MultipleServicesNoTarget,
            "NO_CONFIG_FOUND" => ErrorCode::NoConfigFound,
            "NO_DEPLOYABLE_SERVICES" => ErrorCode::NoDeployableServices,
            "NO_ENV_FILES" => ErrorCode::NoEnvFiles,
            "NO_PUBLIC_SERVICES" => ErrorCode::NoPublicServices,
            "NO_RUNTIME_DETECTED" => ErrorCode::NoRuntimeDetected,
            "NOT_AUTHENTICATED" => ErrorCode::NotAuthenticated,
            "PARSE_ERROR" => ErrorCode::ParseError,
            "RELEASE_ASSET_MISSING" => ErrorCode::ReleaseAssetMissing,
            "RELEASE_LOOKUP_FAILED" => ErrorCode::ReleaseLookupFailed,
            "RELEASE_NOT_FOUND" => ErrorCode::ReleaseNotFound,
            "RELEASE_PARSE_ERROR" => ErrorCode::ReleaseParseError,
            "RESTART_FAILED" => ErrorCode::RestartFailed,
            "SERVICE_CONFIG_MISSING" => ErrorCode::ServiceConfigMissing,
            "SERVICE_NOT_FOUND" => ErrorCode::ServiceNotFound,
            "SIGNUP_DISABLED" => ErrorCode::SignupDisabled,
            "STREAM_ERROR" => ErrorCode::StreamError,
            "UNKNOWN_SERVICE" => ErrorCode::UnknownService,
            "UNSUPPORTED_PLATFORM" => ErrorCode::UnsupportedPlatform,
            "UPDATE_HTTP_CLIENT_ERROR" => ErrorCode::UpdateHttpClientError,
            "UPDATE_INSTALL_FAILED" => ErrorCode::UpdateInstallFailed,
            "UPDATE_INSTALL_PATH_UNRESOLVED" => ErrorCode::UpdateInstallPathUnresolved,
            "UPDATE_PERMISSION_DENIED" => ErrorCode::UpdatePermissionDenied,
            _ => ErrorCode::Other(s.to_string()),
        }
    }
}

#[derive(Error, Debug)]
#[error("{message}")]
pub struct FlooError {
    pub code: ErrorCode,
    pub message: String,
    pub suggestion: Option<String>,
}

impl FlooError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            suggestion: None,
        }
    }

    pub fn with_suggestion(
        code: ErrorCode,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            suggestion: Some(suggestion.into()),
        }
    }
}

#[derive(Error, Debug)]
#[error("{message}")]
pub struct FlooApiError {
    pub status_code: u16,
    pub code: String,
    pub message: String,
    pub extra: Option<serde_json::Value>,
}

impl FlooApiError {
    pub fn new(status_code: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status_code,
            code: code.into(),
            message: message.into(),
            extra: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_floo_error_display() {
        let err = FlooError::new(ErrorCode::Other("TEST".into()), "something failed");
        assert_eq!(err.to_string(), "something failed");
        assert_eq!(err.code, ErrorCode::Other("TEST".into()));
        assert!(err.suggestion.is_none());
    }

    #[test]
    fn test_floo_error_with_suggestion() {
        let err = FlooError::with_suggestion(ErrorCode::Other("CODE".into()), "msg", "try this");
        assert_eq!(err.suggestion.as_deref(), Some("try this"));
    }

    #[test]
    fn test_floo_api_error() {
        let err = FlooApiError::new(404, "NOT_FOUND", "App not found.");
        assert_eq!(err.status_code, 404);
        assert_eq!(err.code, "NOT_FOUND");
        assert_eq!(err.to_string(), "App not found.");
    }

    #[test]
    fn test_error_code_as_str() {
        assert_eq!(ErrorCode::NotAuthenticated.as_str(), "NOT_AUTHENTICATED");
        assert_eq!(ErrorCode::AppNotFound.as_str(), "APP_NOT_FOUND");
        assert_eq!(ErrorCode::Other("CUSTOM".into()).as_str(), "CUSTOM");
    }

    #[test]
    fn test_error_code_from_api() {
        assert_eq!(
            ErrorCode::from_api("NOT_AUTHENTICATED"),
            ErrorCode::NotAuthenticated
        );
        assert_eq!(ErrorCode::from_api("APP_NOT_FOUND"), ErrorCode::AppNotFound);
        assert_eq!(
            ErrorCode::from_api("UNKNOWN_CODE"),
            ErrorCode::Other("UNKNOWN_CODE".into())
        );
    }

    #[test]
    fn test_from_api_roundtrip_all_variants() {
        let variants = [
            ErrorCode::AppNameMismatch,
            ErrorCode::AppNotFound,
            ErrorCode::ChecksumMismatch,
            ErrorCode::ChecksumMissing,
            ErrorCode::ChecksumParseError,
            ErrorCode::ConfigError,
            ErrorCode::ConfigExists,
            ErrorCode::ConfigInvalid,
            ErrorCode::ConfigWriteError,
            ErrorCode::CwdError,
            ErrorCode::DatabaseNotFound,
            ErrorCode::DeployFailed,
            ErrorCode::DeployNotFound,
            ErrorCode::DeployTimeout,
            ErrorCode::DeviceAuthDenied,
            ErrorCode::DeviceCodeExpired,
            ErrorCode::DownloadFailed,
            ErrorCode::DuplicateService,
            ErrorCode::DuplicateServiceNames,
            ErrorCode::EmailTaken,
            ErrorCode::EnvFileNotFound,
            ErrorCode::EnvParseError,
            ErrorCode::FileError,
            ErrorCode::InternalError,
            ErrorCode::InvalidAmount,
            ErrorCode::InvalidFormat,
            ErrorCode::InvalidIngress,
            ErrorCode::InvalidPath,
            ErrorCode::InvalidProjectConfig,
            ErrorCode::InvalidResponse,
            ErrorCode::InvalidRole,
            ErrorCode::InvalidServiceName,
            ErrorCode::InvalidType,
            ErrorCode::LegacyConfig,
            ErrorCode::MissingAppName,
            ErrorCode::MissingArgument,
            ErrorCode::MissingPort,
            ErrorCode::MissingType,
            ErrorCode::MultipleServices,
            ErrorCode::MultipleServicesNoTarget,
            ErrorCode::NoConfigFound,
            ErrorCode::NoDeployableServices,
            ErrorCode::NoEnvFiles,
            ErrorCode::NoPublicServices,
            ErrorCode::NoRuntimeDetected,
            ErrorCode::NotAuthenticated,
            ErrorCode::ParseError,
            ErrorCode::ReleaseAssetMissing,
            ErrorCode::ReleaseLookupFailed,
            ErrorCode::ReleaseNotFound,
            ErrorCode::ReleaseParseError,
            ErrorCode::RestartFailed,
            ErrorCode::ServiceConfigMissing,
            ErrorCode::ServiceNotFound,
            ErrorCode::SignupDisabled,
            ErrorCode::StreamError,
            ErrorCode::UnknownService,
            ErrorCode::UnsupportedPlatform,
            ErrorCode::UpdateHttpClientError,
            ErrorCode::UpdateInstallFailed,
            ErrorCode::UpdateInstallPathUnresolved,
            ErrorCode::UpdatePermissionDenied,
        ];
        for variant in &variants {
            assert_eq!(
                ErrorCode::from_api(variant.as_str()),
                *variant,
                "from_api(as_str()) roundtrip failed for {:?}",
                variant
            );
        }
    }
}
