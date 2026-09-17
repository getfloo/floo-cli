//! TLS trust and diagnostics shared by foreground and background updates.
use std::{env, error::Error, fs, time::Duration};

use reqwest::{blocking::Client, Certificate};

use crate::errors::{ErrorCode, FlooError};

pub(super) const CONNECTION_HINT: &str = "Check the reported connection cause. Behind a TLS-inspecting proxy, set SSL_CERT_FILE to its trusted PEM CA bundle.";

/// Keep bundled roots and native trust, validating an explicit CA bundle strictly.
pub(crate) fn build_client(connect_secs: u64, timeout_secs: u64) -> Result<Client, FlooError> {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(connect_secs))
        .timeout(Duration::from_secs(timeout_secs));
    // rustls-native-certs uses SSL_CERT_FILE instead of system discovery when set.
    // Explicit parsing prevents its best-effort loader from hiding a broken override.
    if let Some(path) = env::var_os("SSL_CERT_FILE") {
        let pem =
            fs::read(path).map_err(|e| client_error(format!("Cannot read SSL_CERT_FILE: {e}")))?;
        let certs = Certificate::from_pem_bundle(&pem)
            .map_err(|e| client_error(format!("Invalid SSL_CERT_FILE: {}", error_chain(e))))?;
        if certs.is_empty() {
            return Err(client_error(
                "SSL_CERT_FILE contains no PEM certificates".into(),
            ));
        }
        for cert in certs {
            builder = builder.add_root_certificate(cert);
        }
    }
    builder.build().map_err(|e| client_error(error_chain(e)))
}

fn client_error(cause: String) -> FlooError {
    FlooError::with_suggestion(
        ErrorCode::UpdateHttpClientError,
        format!("Failed to initialize update client: {cause}"),
        CONNECTION_HINT,
    )
}

/// Retain source errors without exposing request URLs or credential-shaped text.
pub(super) fn error_chain(error: reqwest::Error) -> String {
    // Release redirects carry signed query parameters; diagnostics need the cause,
    // not a URL. Suppress any remaining URL-bearing source messages below.
    let error = error.without_url();
    let mut current: Option<&(dyn Error + 'static)> = Some(&error);
    let mut parts = Vec::new();
    while let Some(cause) = current {
        let message = cause.to_string();
        if message.contains("://") || crate::redact::is_secret("message", &message) {
            parts.push(crate::redact::REDACTED_PLACEHOLDER.to_string());
        } else if parts.last() != Some(&message) {
            parts.push(message);
        }
        current = cause.source();
    }
    parts.join(": ")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    /// Run each environment-sensitive case alone, behind the same local proxy fixture.
    pub(crate) fn through_proxy(name: &str, check: impl FnOnce()) {
        if env::var("FLOO_TLS_TEST_CHILD").as_deref() == Ok(name) {
            crate::output::set_json_mode(false);
            crate::output::set_dry_run_mode(false);
            check();
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let mut proxy = Command::new("python3")
            .args([
                "tests/fixtures/update_tls.py",
                directory.path().to_str().unwrap(),
                &crate::updater::target_asset_name().unwrap(),
                crate::updater::tests::FAKE_BINARY_SIGNATURE_B64,
            ])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut port = String::new();
        BufReader::new(proxy.stdout.take().unwrap())
            .read_line(&mut port)
            .unwrap();
        let result = Command::new(env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("FLOO_TLS_TEST_CHILD", name)
            .env("FLOO_CONFIG_DIR", directory.path())
            .env(
                "FLOO_UPDATE_TARGET_PATH",
                directory.path().join("installed-floo"),
            )
            .env("FLOO_UPDATE_API_BASE", "https://releases.test/releases")
            .env("SSL_CERT_FILE", directory.path().join("ca.pem"))
            .env_remove("SSL_CERT_DIR")
            .env(
                "https_proxy",
                format!("http://user:proxy-secret@127.0.0.1:{}", port.trim()),
            )
            .env_remove("HTTPS_PROXY")
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .output();
        proxy.kill().unwrap();
        proxy.wait().unwrap();
        let result = result.unwrap();
        assert!(result.status.success(), "{result:?}");
    }

    #[test]
    fn configured_ca_allows_preflight_and_verified_update() {
        through_proxy(
            "updater::http::tests::configured_ca_allows_preflight_and_verified_update",
            || {
                let plan = crate::updater::check_update(None).unwrap();
                assert_eq!(plan.target_version, "v9999.0.0");
                assert!(!plan.install_path.exists());
                let result = crate::updater::run_update(None).unwrap();
                assert_eq!(
                    fs::read(result.install_path).unwrap(),
                    b"fake-binary-content"
                );
            },
        );
    }

    #[test]
    fn native_loader_accepts_configured_trust() {
        through_proxy(
            "updater::http::tests::native_loader_accepts_configured_trust",
            || {
                let ca = std::path::PathBuf::from(env::var_os("SSL_CERT_FILE").unwrap());
                let directory = ca.parent().unwrap().join("native-roots");
                fs::create_dir(&directory).unwrap();
                fs::copy(&ca, directory.join("ca.pem")).unwrap();
                env::remove_var("SSL_CERT_FILE");
                env::set_var("SSL_CERT_DIR", directory);
                assert_eq!(
                    crate::updater::check_update(None).unwrap().target_version,
                    "v9999.0.0"
                );
            },
        );
    }

    #[test]
    fn unknown_ca_retains_certificate_cause() {
        through_proxy(
            "updater::http::tests::unknown_ca_retains_certificate_cause",
            || {
                env::remove_var("SSL_CERT_FILE");
                let error = crate::updater::check_update(None).err().unwrap();
                assert_eq!(error.code, ErrorCode::ReleaseLookupFailed);
                assert!(error.message.contains("UnknownIssuer"), "{}", error.message);
                assert!(error.suggestion.unwrap().contains("SSL_CERT_FILE"));
                assert!(!error.message.contains("proxy-secret"));
            },
        );
    }

    #[test]
    fn configured_ca_does_not_disable_hostname_verification() {
        through_proxy(
            "updater::http::tests::configured_ca_does_not_disable_hostname_verification",
            || {
                env::set_var("FLOO_UPDATE_API_BASE", "https://wrong-host.test/releases");
                let error = crate::updater::check_update(None).err().unwrap();
                assert!(
                    error.message.contains("not valid for name"),
                    "{}",
                    error.message
                );
            },
        );
    }

    #[test]
    fn broken_explicit_bundles_fail_at_initialization() {
        through_proxy(
            "updater::http::tests::broken_explicit_bundles_fail_at_initialization",
            || {
                let path = std::path::PathBuf::from(env::var_os("SSL_CERT_FILE").unwrap());
                fs::remove_file(&path).unwrap();
                assert!(build_client(1, 1)
                    .err()
                    .unwrap()
                    .message
                    .contains("Cannot read SSL_CERT_FILE"));
                fs::write(&path, b"not a certificate").unwrap();
                assert!(build_client(1, 1)
                    .err()
                    .unwrap()
                    .message
                    .contains("no PEM certificates"));
                fs::write(
                    &path,
                    b"-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n",
                )
                .unwrap();
                assert_eq!(
                    build_client(1, 1).err().unwrap().code,
                    ErrorCode::UpdateHttpClientError
                );
            },
        );
    }
}
