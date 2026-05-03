use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, USER_AGENT};
use ring::signature;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::errors::{ErrorCode, FlooError};

const DEFAULT_RELEASES_API_BASE: &str = "https://api.github.com/repos/getfloo/floo-cli/releases";
// ring verifies RSA signatures against PKCS#1 DER, so keep this as
// "RSA PUBLIC KEY" rather than SubjectPublicKeyInfo "PUBLIC KEY" PEM.
const DEFAULT_RELEASE_PUBLIC_KEY_PEM: &str = r#"-----BEGIN RSA PUBLIC KEY-----
MIIBigKCAYEAoxTcA648/UUTEcmZbiZbwGsQJIjI/CwEda/3Zwky26hOdu3ccQKD
U3lXj7c/cAvr0Y+ISnf23YvBr68q0kI0IhihYE74MoOKe0QjRv7aK0cYgIWKj5SZ
xcw0CLvMm36rNG7iZBHJb3Jbew5ebMpaRyCZBnruHQocHQammzUkuDjeJ753ZFmu
Y8Fyr/CLO+F2V7Bou/qh4DA0tJ8Ams4HLTUGAfXHgj3Q5L9DIZC6iDzGqg70DblC
wNrr/n+zx6TCjonKraYxDUXruR6Za6XrKSbTrq6Bh1DFYK5DM3m9OIdiMx2EC+yD
3iY/CZRC/auqq4CQeXLQyxTsExxnvG3O4Ci77MTZH4NSnngkkw5KrcvqCC9KVI9J
IViei4zB3GoTGDm9+FC02cCozhKiTvAqzdb+ieszMNsavQNdOy1qO9bQfObWWvay
Z4rrRM3hE+rKyk5WHrPZcR77YiqZ6cwXVl7g8gJ0JIQi2a8oHzmjQc7n+j1Nmglh
Wk6BmNyJThezAgMBAAE=
-----END RSA PUBLIC KEY-----"#;

#[derive(Debug, Clone)]
pub(crate) struct ReleaseAsset {
    pub(crate) version: String,
    pub(crate) asset_name: String,
    pub(crate) binary_url: String,
    pub(crate) checksum_url: String,
    pub(crate) signature_url: String,
}

#[derive(Debug, Clone)]
pub struct UpdateResult {
    pub version: String,
    pub install_path: PathBuf,
}

pub(crate) fn releases_api_base() -> String {
    env::var("FLOO_UPDATE_API_BASE").unwrap_or_else(|_| DEFAULT_RELEASES_API_BASE.to_string())
}

pub(crate) fn target_asset_name() -> Result<String, FlooError> {
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

pub(crate) fn fetch_release_json(
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

pub(crate) fn release_asset_from_json(
    release: &Value,
    asset_name: &str,
) -> Result<ReleaseAsset, FlooError> {
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

    let signature_name = format!("{asset_name}.sig");
    let signature_url = assets
        .iter()
        .find(|asset| asset.get("name").and_then(Value::as_str) == Some(signature_name.as_str()))
        .and_then(|asset| asset.get("browser_download_url"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| {
            FlooError::with_suggestion(
                ErrorCode::ReleaseSignatureMissing,
                format!("No signature asset found for '{asset_name}'."),
                "Try again once release artifacts are fully published.",
            )
        })?;

    Ok(ReleaseAsset {
        version,
        asset_name: asset_name.to_string(),
        binary_url,
        checksum_url,
        signature_url,
    })
}

pub(crate) fn download_bytes(client: &Client, url: &str) -> Result<Vec<u8>, FlooError> {
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

pub(crate) fn parse_checksum(checksum_contents: &str) -> Result<String, FlooError> {
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

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn decode_public_key_pem(pem: &str) -> Result<Vec<u8>, FlooError> {
    let body = pem
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("-----"))
        .collect::<String>();

    if body.is_empty() {
        return Err(FlooError::with_suggestion(
            ErrorCode::ReleaseSignatureInvalid,
            "Release verification public key is empty.".to_string(),
            "Reinstall via curl -fsSL https://getfloo.com/install.sh | bash. If this persists, contact support.",
        ));
    }

    BASE64_STANDARD.decode(body).map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::ReleaseSignatureInvalid,
            format!("Release verification public key is invalid: {e}"),
            "Reinstall via curl -fsSL https://getfloo.com/install.sh | bash. If this persists, contact support.",
        )
    })
}

pub(crate) fn verify_release_signature(
    asset_name: &str,
    binary_bytes: &[u8],
    signature_bytes: &[u8],
) -> Result<(), FlooError> {
    let public_key_der = decode_public_key_pem(DEFAULT_RELEASE_PUBLIC_KEY_PEM)?;
    let public_key =
        signature::UnparsedPublicKey::new(&signature::RSA_PKCS1_2048_8192_SHA256, public_key_der);

    public_key
        .verify(binary_bytes, signature_bytes)
        .map_err(|_| {
            FlooError::with_suggestion(
                ErrorCode::ReleaseSignatureInvalid,
                format!("Signature verification failed for '{asset_name}'."),
                "Do not run this binary. Retry update later or reinstall via curl -fsSL https://getfloo.com/install.sh | bash",
            )
        })
}

#[cfg(unix)]
pub(crate) fn set_executable(path: &Path) -> Result<(), FlooError> {
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
pub(crate) fn set_executable(_path: &Path) -> Result<(), FlooError> {
    Ok(())
}

fn install_path_override() -> Option<PathBuf> {
    env::var("FLOO_UPDATE_TARGET_PATH").ok().map(PathBuf::from)
}

pub(crate) fn resolve_install_path() -> Result<PathBuf, FlooError> {
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

pub(crate) fn install_binary(binary_bytes: &[u8], destination: &Path) -> Result<(), FlooError> {
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

    // Skip download if already on this version (unless a specific version was requested)
    let current = crate::constants::VERSION;
    let remote = release_asset
        .version
        .strip_prefix('v')
        .unwrap_or(&release_asset.version);
    if version.is_none() && remote == current {
        return Err(FlooError::new(
            ErrorCode::AlreadyUpToDate,
            format!("floo {current} is already the latest version."),
        ));
    }

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

    let signature_bytes = download_bytes(client, &release_asset.signature_url)?;
    verify_release_signature(&release_asset.asset_name, &binary_bytes, &signature_bytes)?;

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

pub struct UpdatePlan {
    pub current_version: String,
    pub target_version: String,
    pub install_path: PathBuf,
    pub already_up_to_date: bool,
}

/// Dry-run equivalent of `run_update`: resolves which release WOULD be installed
/// without downloading the binary, verifying its checksum, or touching disk.
/// Reports the release the updater would have targeted so `--dry-run` callers
/// can see the plan without mutating the binary. See feedback 7b98b798.
pub fn check_update(version: Option<&str>) -> Result<UpdatePlan, FlooError> {
    let client = build_http_client()?;
    let api_base = releases_api_base();
    let install_path = resolve_install_path()?;
    let asset_name = target_asset_name()?;
    let release_json = fetch_release_json(&client, &api_base, version)?;
    let release_asset = release_asset_from_json(&release_json, &asset_name)?;

    let current = crate::constants::VERSION.to_string();
    let remote = release_asset
        .version
        .strip_prefix('v')
        .unwrap_or(&release_asset.version)
        .to_string();
    let already_up_to_date = version.is_none() && remote == current;

    Ok(UpdatePlan {
        current_version: current,
        target_version: release_asset.version,
        install_path,
        already_up_to_date,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};
    use std::sync::{Mutex, OnceLock};

    static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    const FAKE_BINARY_SIGNATURE_B64: &str = "asvfjb0bQYA5IrimKSPkA+BgWNHyuP3ax4H4qDQPM3jbsHL1C1fQvjmeKbgadkR3t1QxdDF+62s4pJ81LlDFzW6Iz/BXY9nUUabDSVRLVDqN9F21RWxIor/m89snTJSnanhvbh1+nJ3SeYDJSmKVBqRlNld1ACykNVBlU6eXOcD+hc2faJD4m3VSdaQvRUZsXCGTL5YzyyHV86PbUk4tYt9LQsGsa/CAA0h5TX2UMNmkk12byCh7IbV9tt58lXr3+e26+54UhjDSPX29jLcHEATDPgpnllXDGUyZLtJO1GsT7ojyWrlj18M1zvNg7el9l794HSaK8uTFq2bhvURRsGKjOe3NH13+fZYvL/azLrnvT8/zOrAbpToHVcJeuNo4DUHRJMc/U6ulykHYpeF4ebafr6JREmzOQ9VVUP8vBSco7Ocw7fCxyc77dfmZnTMGooIoifKKUhIjk9ZFIUykXU9BRRuZWVap8vNy6NHZw+EM3wxk4o+vA4/wAgAvliU5";

    fn fake_binary_signature() -> Vec<u8> {
        BASE64_STANDARD
            .decode(FAKE_BINARY_SIGNATURE_B64)
            .expect("test signature decodes")
    }

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
                },
                {
                    "name": format!("{asset_name}.sig"),
                    "browser_download_url": format!("{base_url}/download/{asset_name}.sig"),
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
        let signature = fake_binary_signature();

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

        let _signature_mock = server
            .mock("GET", format!("/download/{asset_name}.sig").as_str())
            .match_header("user-agent", "floo-cli-updater")
            .with_status(200)
            .with_body(signature.as_slice())
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
    fn test_run_update_with_signature_mismatch() {
        let asset_name = target_asset_name().unwrap();
        let binary_bytes = b"fake-binary-content";
        let checksum = sha256_hex(binary_bytes);

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
            .with_body(format!("{checksum}  {asset_name}"))
            .create();

        let _signature_mock = server
            .mock("GET", format!("/download/{asset_name}.sig").as_str())
            .with_status(200)
            .with_body(b"not-a-valid-signature".as_slice())
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
            crate::errors::ErrorCode::ReleaseSignatureInvalid
        );
        assert!(!install_path.exists());
    }

    #[test]
    fn test_release_asset_requires_signature() {
        let asset_name = target_asset_name().unwrap();
        let release = serde_json::json!({
            "tag_name": "v0.2.0",
            "assets": [
                {
                    "name": asset_name,
                    "browser_download_url": "https://example.test/floo",
                },
                {
                    "name": format!("{asset_name}.sha256"),
                    "browser_download_url": "https://example.test/floo.sha256",
                }
            ]
        });

        let result = release_asset_from_json(&release, &asset_name);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code,
            crate::errors::ErrorCode::ReleaseSignatureMissing
        );
    }

    #[test]
    fn test_release_asset_missing_when_no_assets_uploaded() {
        // Simulates a race: GitHub release object exists but the CI workflow
        // hasn't finished uploading binaries yet (~3-minute window).
        let asset_name = target_asset_name().unwrap();
        let release = serde_json::json!({
            "tag_name": "v0.2.0",
            "assets": []
        });

        let result = release_asset_from_json(&release, &asset_name);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code,
            crate::errors::ErrorCode::ReleaseAssetMissing
        );
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
