use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, USER_AGENT};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::errors::{ErrorCode, FlooError};

const DEFAULT_RELEASES_API_BASE: &str = "https://api.github.com/repos/getfloo/floo-cli/releases";

#[derive(Debug, Clone)]
struct ReleaseAsset {
    version: String,
    asset_name: String,
    binary_url: String,
    checksum_url: String,
}

#[derive(Debug, Clone)]
pub struct UpdateResult {
    pub version: String,
    pub install_path: PathBuf,
}

fn releases_api_base() -> String {
    env::var("FLOO_UPDATE_API_BASE").unwrap_or_else(|_| DEFAULT_RELEASES_API_BASE.to_string())
}

fn target_asset_name() -> Result<String, FlooError> {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    let target = match (os, arch) {
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc.exe",
        _ => {
            return Err(FlooError::with_suggestion(
                ErrorCode::UnsupportedPlatform,
                format!("Platform '{os}/{arch}' is not supported for automatic updates."),
                "Download a release binary manually from https://github.com/getfloo/floo-cli/releases",
            ));
        }
    };

    Ok(format!("floo-{target}"))
}

fn build_release_url(api_base: &str, version: Option<&str>) -> String {
    match version {
        Some(v) if !v.trim().is_empty() => format!("{api_base}/tags/{v}"),
        _ => format!("{api_base}/latest"),
    }
}

fn build_http_client() -> Result<Client, FlooError> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::UpdateHttpClientError,
            format!("Failed to initialize update client: {e}"),
            "Try again. If it persists, reinstall via curl -fsSL https://getfloo.com/install.sh | bash",
        )
    })
}

fn fetch_release_json(
    client: &Client,
    api_base: &str,
    version: Option<&str>,
) -> Result<Value, FlooError> {
    let url = build_release_url(api_base, version);
    let response = client
        .get(url)
        .header(USER_AGENT, "floo-cli-updater")
        .header(ACCEPT, "application/vnd.github+json")
        .send()
        .map_err(|e| {
            FlooError::with_suggestion(
                ErrorCode::ReleaseLookupFailed,
                format!("Failed to fetch release metadata: {e}"),
                "Check your network and try again.",
            )
        })?;

    if !response.status().is_success() {
        return Err(FlooError::with_suggestion(
            ErrorCode::ReleaseLookupFailed,
            format!("Release lookup failed with status {}.", response.status()),
            "Verify the version tag exists, or run 'floo update' without --version.",
        ));
    }

    response.json::<Value>().map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::ReleaseParseError,
            format!("Failed to parse release metadata: {e}"),
            "Try again. If it persists, reinstall via curl -fsSL https://getfloo.com/install.sh | bash",
        )
    })
}

fn release_asset_from_json(release: &Value, asset_name: &str) -> Result<ReleaseAsset, FlooError> {
    let version = release
        .get("tag_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let assets = release
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            FlooError::with_suggestion(
                ErrorCode::ReleaseAssetMissing,
                "Release metadata did not include assets.".to_string(),
                "Try again shortly after the release publishes.",
            )
        })?;

    let binary_url = assets
        .iter()
        .find(|asset| asset.get("name").and_then(Value::as_str) == Some(asset_name))
        .and_then(|asset| asset.get("browser_download_url"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| {
            FlooError::with_suggestion(
                ErrorCode::ReleaseAssetMissing,
                format!("No binary asset found for '{asset_name}'."),
                "Check https://github.com/getfloo/floo-cli/releases for available artifacts.",
            )
        })?;

    let checksum_name = format!("{asset_name}.sha256");
    let checksum_url = assets
        .iter()
        .find(|asset| asset.get("name").and_then(Value::as_str) == Some(checksum_name.as_str()))
        .and_then(|asset| asset.get("browser_download_url"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| {
            FlooError::with_suggestion(
                ErrorCode::ChecksumMissing,
                format!("No checksum asset found for '{asset_name}'."),
                "Try again once release artifacts are fully published.",
            )
        })?;

    Ok(ReleaseAsset {
        version,
        asset_name: asset_name.to_string(),
        binary_url,
        checksum_url,
    })
}

fn download_bytes(client: &Client, url: &str) -> Result<Vec<u8>, FlooError> {
    let response = client
        .get(url)
        .header(USER_AGENT, "floo-cli-updater")
        .send()
        .map_err(|e| {
            FlooError::with_suggestion(
                ErrorCode::DownloadFailed,
                format!("Failed to download update asset: {e}"),
                "Check your network and try again.",
            )
        })?;

    if !response.status().is_success() {
        return Err(FlooError::with_suggestion(
            ErrorCode::DownloadFailed,
            format!("Asset download failed with status {}.", response.status()),
            "Check the release artifacts and try again.",
        ));
    }

    response.bytes().map(|bytes| bytes.to_vec()).map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::DownloadFailed,
            format!("Failed to read downloaded bytes: {e}"),
            "Try again.",
        )
    })
}

fn parse_checksum(checksum_contents: &str) -> Result<String, FlooError> {
    for line in checksum_contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(token) = trimmed.split_whitespace().next() {
            let normalized = token.to_ascii_lowercase();
            if normalized.len() == 64 && normalized.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(normalized);
            }
        }
    }

    Err(FlooError::with_suggestion(
        ErrorCode::ChecksumParseError,
        "Checksum file did not contain a valid SHA256 hash.".to_string(),
        "Try again. If this persists, reinstall via curl -fsSL https://getfloo.com/install.sh | bash",
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), FlooError> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|e| {
            FlooError::new(
                ErrorCode::UpdateInstallFailed,
                format!("Failed to stat file: {e}"),
            )
        })?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|e| {
        FlooError::new(
            ErrorCode::UpdateInstallFailed,
            format!("Failed to set executable permissions: {e}"),
        )
    })
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), FlooError> {
    Ok(())
}

fn install_path_override() -> Option<PathBuf> {
    env::var("FLOO_UPDATE_TARGET_PATH").ok().map(PathBuf::from)
}

fn resolve_install_path() -> Result<PathBuf, FlooError> {
    if let Some(path) = install_path_override() {
        return Ok(path);
    }

    env::current_exe().map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::UpdateInstallPathUnresolved,
            format!("Failed to determine installed CLI path: {e}"),
            "Set FLOO_UPDATE_TARGET_PATH to the floo binary path or reinstall via curl -fsSL https://getfloo.com/install.sh | bash",
        )
    })
}

fn install_binary(binary_bytes: &[u8], destination: &Path) -> Result<(), FlooError> {
    let destination_dir = destination.parent().ok_or_else(|| {
        FlooError::new(
            ErrorCode::UpdateInstallFailed,
            "Failed to determine destination directory.".to_string(),
        )
    })?;

    fs::create_dir_all(destination_dir).map_err(|e| {
        FlooError::new(
            ErrorCode::UpdateInstallFailed,
            format!("Failed to prepare destination directory: {e}"),
        )
    })?;

    let temp_name = format!(
        ".{}.tmp-update-{}",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("floo"),
        std::process::id()
    );
    let temp_path = destination_dir.join(temp_name);

    fs::write(&temp_path, binary_bytes).map_err(|e| {
        FlooError::new(
            ErrorCode::UpdateInstallFailed,
            format!("Failed to write update file: {e}"),
        )
    })?;
    set_executable(&temp_path)?;

    fs::rename(&temp_path, destination).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            FlooError::with_suggestion(
                ErrorCode::UpdatePermissionDenied,
                format!("Permission denied while writing to '{}'.", destination.display()),
                "Re-run with appropriate permissions, or reinstall via curl -fsSL https://getfloo.com/install.sh | bash",
            )
        } else {
            FlooError::new(
                ErrorCode::UpdateInstallFailed,
                format!("Failed to replace existing binary: {e}"),
            )
        }
    })
}

fn run_update_with(
    client: &Client,
    version: Option<&str>,
    api_base: &str,
    install_path: &Path,
) -> Result<UpdateResult, FlooError> {
    let asset_name = target_asset_name()?;
    let release_json = fetch_release_json(client, api_base, version)?;
    let release_asset = release_asset_from_json(&release_json, &asset_name)?;

    let binary_bytes = download_bytes(client, &release_asset.binary_url)?;
    let checksum_bytes = download_bytes(client, &release_asset.checksum_url)?;
    let expected_checksum = parse_checksum(&String::from_utf8_lossy(&checksum_bytes))?;
    let actual_checksum = sha256_hex(&binary_bytes);

    if actual_checksum != expected_checksum {
        return Err(FlooError::with_suggestion(
            ErrorCode::ChecksumMismatch,
            format!(
                "Checksum mismatch for '{}': expected {}, got {}.",
                release_asset.asset_name, expected_checksum, actual_checksum
            ),
            "Do not run this binary. Retry update later or reinstall via curl -fsSL https://getfloo.com/install.sh | bash",
        ));
    }

    install_binary(&binary_bytes, install_path)?;

    Ok(UpdateResult {
        version: release_asset.version,
        install_path: install_path.to_path_buf(),
    })
}

pub fn run_update(version: Option<&str>) -> Result<UpdateResult, FlooError> {
    let client = build_http_client()?;
    let api_base = releases_api_base();
    let install_path = resolve_install_path()?;

    run_update_with(&client, version, &api_base, &install_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};
    use std::sync::{Mutex, OnceLock};

    static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    fn sample_release_json(asset_name: &str, base_url: &str) -> Value {
        serde_json::json!({
            "tag_name": "v0.2.0",
            "assets": [
                {
                    "name": asset_name,
                    "browser_download_url": format!("{base_url}/download/{asset_name}"),
                },
                {
                    "name": format!("{asset_name}.sha256"),
                    "browser_download_url": format!("{base_url}/download/{asset_name}.sha256"),
                }
            ]
        })
    }

    #[test]
    fn test_parse_checksum_ok() {
        let value = parse_checksum(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  floo-x86_64-unknown-linux-musl",
        )
        .unwrap();
        assert_eq!(value.len(), 64);
    }

    #[test]
    fn test_parse_checksum_invalid() {
        let result = parse_checksum("not-a-checksum");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, ErrorCode::ChecksumParseError);
    }

    #[test]
    fn test_run_update_with_mock_server_success() {
        let asset_name = target_asset_name().unwrap();
        let binary_bytes = b"fake-binary-content";
        let checksum = sha256_hex(binary_bytes);

        let mut server = Server::new();
        let release = sample_release_json(&asset_name, &server.url());

        let _release_mock = server
            .mock("GET", "/releases/latest")
            .match_header("user-agent", "floo-cli-updater")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(release.to_string())
            .create();

        let _binary_mock = server
            .mock("GET", format!("/download/{asset_name}").as_str())
            .match_header("user-agent", "floo-cli-updater")
            .with_status(200)
            .with_body(binary_bytes.as_slice())
            .create();

        let _checksum_mock = server
            .mock("GET", format!("/download/{asset_name}.sha256").as_str())
            .match_header("user-agent", "floo-cli-updater")
            .with_status(200)
            .with_body(format!("{checksum}  {asset_name}"))
            .create();

        let client = build_http_client().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let install_path = temp_dir.path().join("floo");

        let result = run_update_with(
            &client,
            None,
            &format!("{}/releases", server.url()),
            &install_path,
        )
        .unwrap();

        assert_eq!(result.version, "v0.2.0");
        assert_eq!(result.install_path, install_path);
        assert!(install_path.exists());
        assert_eq!(fs::read(install_path).unwrap(), binary_bytes.as_slice());
    }

    #[test]
    fn test_run_update_with_checksum_mismatch() {
        let asset_name = target_asset_name().unwrap();
        let binary_bytes = b"fake-binary-content";
        let wrong_checksum = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

        let mut server = Server::new();
        let release = sample_release_json(&asset_name, &server.url());

        let _release_mock = server
            .mock("GET", "/releases/latest")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(release.to_string())
            .create();

        let _binary_mock = server
            .mock("GET", format!("/download/{asset_name}").as_str())
            .with_status(200)
            .with_body(binary_bytes.as_slice())
            .create();

        let _checksum_mock = server
            .mock("GET", format!("/download/{asset_name}.sha256").as_str())
            .with_status(200)
            .with_body(format!("{wrong_checksum}  {asset_name}"))
            .create();

        let client = build_http_client().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let install_path = temp_dir.path().join("floo");

        let result = run_update_with(
            &client,
            None,
            &format!("{}/releases", server.url()),
            &install_path,
        );

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code,
            crate::errors::ErrorCode::ChecksumMismatch
        );
        assert!(!install_path.exists());
    }

    #[test]
    fn test_release_lookup_404() {
        let mut server = Server::new();
        let _release_mock = server
            .mock("GET", "/releases/tags/v9.9.9")
            .match_query(Matcher::Any)
            .with_status(404)
            .with_body("not found")
            .create();

        let client = build_http_client().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let install_path = temp_dir.path().join("floo");

        let result = run_update_with(
            &client,
            Some("v9.9.9"),
            &format!("{}/releases", server.url()),
            &install_path,
        );

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code,
            crate::errors::ErrorCode::ReleaseLookupFailed
        );
    }

    #[test]
    fn test_resolve_install_path_override() {
        let _guard = ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let old_value = env::var("FLOO_UPDATE_TARGET_PATH").ok();
        env::set_var("FLOO_UPDATE_TARGET_PATH", "/tmp/floo-test-path");

        let resolved = resolve_install_path().unwrap();
        assert_eq!(resolved, PathBuf::from("/tmp/floo-test-path"));

        if let Some(value) = old_value {
            env::set_var("FLOO_UPDATE_TARGET_PATH", value);
        } else {
            env::remove_var("FLOO_UPDATE_TARGET_PATH");
        }
    }

    #[test]
    fn test_resolve_install_path_uses_current_exe_path() {
        let _guard = ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let old_value = env::var("FLOO_UPDATE_TARGET_PATH").ok();
        env::remove_var("FLOO_UPDATE_TARGET_PATH");

        let resolved = resolve_install_path().unwrap();
        assert!(resolved.is_absolute());

        if let Some(value) = old_value {
            env::set_var("FLOO_UPDATE_TARGET_PATH", value);
        }
    }
}
