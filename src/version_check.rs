use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::config;
use crate::updater;

const CACHE_TTL_SECS: u64 = 900; // 15 minutes
const CHECK_TIMEOUT_SECS: u64 = 3;
const DOWNLOAD_TIMEOUT_SECS: u64 = 30;
const EXIT_WAIT_MS: u64 = 500;

const STAGED_DIR_NAME: &str = "staged-update";
const CACHE_FILE_NAME: &str = "version-check.json";
const STAGED_BINARY_NAME: &str = "binary";
const STAGED_META_NAME: &str = "metadata.json";
const DOWNLOADING_NAME: &str = ".downloading";

const MANUAL_UPDATE_HINT: &str =
    "  Run `floo update` or reinstall via: curl -fsSL https://getfloo.com/install.sh | bash";

#[derive(Serialize, Deserialize)]
struct VersionCache {
    latest_version: String,
    checked_at: u64,
}

#[derive(Serialize, Deserialize)]
struct StagedUpdateMeta {
    version: String,
    staged_at: u64,
}

pub struct VersionCheckHandle {
    rx: mpsc::Receiver<CheckResult>,
}

enum CheckResult {
    Downloaded(String),
    UpToDate,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn floo_dir() -> Option<PathBuf> {
    let dir = config::config_dir();
    // config_dir() falls back to "~/.floo" when HOME is unset; "~" is not a usable path
    if dir.starts_with("~") {
        return None;
    }
    Some(dir)
}

fn cache_path() -> Option<PathBuf> {
    floo_dir().map(|d| d.join(CACHE_FILE_NAME))
}

fn staged_dir() -> Option<PathBuf> {
    floo_dir().map(|d| d.join(STAGED_DIR_NAME))
}

fn read_cache() -> Option<VersionCache> {
    let path = cache_path()?;
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache(cache: &VersionCache) {
    let Some(path) = cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let Ok(json) = serde_json::to_string(cache) else {
        return;
    };
    let _ = fs::write(path, json);
}

fn read_staged_meta() -> Option<StagedUpdateMeta> {
    let dir = staged_dir()?;
    let meta_path = dir.join(STAGED_META_NAME);
    let content = fs::read_to_string(meta_path).ok()?;
    serde_json::from_str(&content).ok()
}

fn staged_binary_path() -> Option<PathBuf> {
    staged_dir().map(|d| d.join(STAGED_BINARY_NAME))
}

fn staged_binary_exists() -> bool {
    staged_binary_path().is_some_and(|p| p.exists())
}

fn clean_staged_dir() {
    if let Some(dir) = staged_dir() {
        let _ = fs::remove_dir_all(dir);
    }
}

fn clean_orphaned_downloading() {
    let Some(dir) = staged_dir() else { return };
    let dl = dir.join(DOWNLOADING_NAME);
    // Skip the exists() pre-check — just try to remove and let the OS tell us.
    // Only remove if metadata.json is absent (orphaned temp file).
    if dir.join(STAGED_META_NAME).exists() {
        return;
    }
    let _ = fs::remove_file(dl);
}

fn cache_is_fresh(cache: &VersionCache) -> bool {
    let elapsed = now_secs().saturating_sub(cache.checked_at);
    elapsed < CACHE_TTL_SECS
}

fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |v: &str| -> Option<(u64, u64, u64)> {
        let v = v.strip_prefix('v').unwrap_or(v);
        let parts: Vec<&str> = v.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ))
    };

    match (parse(remote), parse(local)) {
        (Some(r), Some(l)) => r > l,
        _ => false,
    }
}

fn build_client(timeout_secs: u64) -> Option<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(CHECK_TIMEOUT_SECS))
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .ok()
}

/// Fetch latest release version from GitHub. Returns (tag_name, full release JSON)
/// so the caller can reuse the JSON for asset download without a second request.
fn fetch_latest_release(client: &Client, api_base: &str) -> Option<(String, serde_json::Value)> {
    let json = updater::fetch_release_json(client, api_base, None).ok()?;
    let version = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)?;
    Some((version, json))
}

fn download_and_stage(current_version: &str, release_json: &serde_json::Value) -> Option<String> {
    let client = build_client(DOWNLOAD_TIMEOUT_SECS)?;
    let asset_name = updater::target_asset_name().ok()?;
    let release_asset = updater::release_asset_from_json(release_json, &asset_name).ok()?;

    // Double-check version from full release metadata
    if !is_newer(&release_asset.version, current_version) {
        return None;
    }

    let dir = staged_dir()?;
    fs::create_dir_all(&dir).ok()?;
    let dl_path = dir.join(DOWNLOADING_NAME);

    let binary_bytes = updater::download_bytes(&client, &release_asset.binary_url).ok()?;
    let checksum_bytes = updater::download_bytes(&client, &release_asset.checksum_url).ok()?;
    let expected = updater::parse_checksum(&String::from_utf8_lossy(&checksum_bytes)).ok()?;
    let actual = updater::sha256_hex(&binary_bytes);

    if actual != expected {
        let _ = fs::remove_file(&dl_path);
        // Update cache so we don't re-download this version until TTL expires
        write_cache(&VersionCache {
            latest_version: release_asset.version.clone(),
            checked_at: now_secs(),
        });
        return None;
    }

    // Write to temp file, then rename atomically
    fs::write(&dl_path, &binary_bytes).ok()?;

    #[cfg(unix)]
    if updater::set_executable(&dl_path).is_err() {
        let _ = fs::remove_file(&dl_path);
        return None;
    }

    let binary_path = dir.join(STAGED_BINARY_NAME);
    fs::rename(&dl_path, &binary_path).ok()?;

    // Write metadata last — its presence signals a complete download
    let meta = StagedUpdateMeta {
        version: release_asset.version.clone(),
        staged_at: now_secs(),
    };
    let meta_json = serde_json::to_string(&meta).ok()?;
    if fs::write(dir.join(STAGED_META_NAME), &meta_json).is_err() {
        // Clean up binary to avoid stuck state (binary without metadata)
        let _ = fs::remove_file(&binary_path);
        return None;
    }

    Some(release_asset.version)
}

/// Phase 2: Apply a previously staged update. Called at startup before command dispatch.
pub fn apply_staged_update(current_version: &str) {
    // Clean up orphaned .downloading files from interrupted previous runs
    clean_orphaned_downloading();

    let meta = match read_staged_meta() {
        Some(m) => m,
        None => return,
    };

    // Only apply if staged version is actually newer
    if !is_newer(&meta.version, current_version) {
        clean_staged_dir();
        return;
    }

    let binary_path = match staged_binary_path() {
        Some(p) if p.exists() => p,
        _ => {
            clean_staged_dir();
            return;
        }
    };

    let binary_bytes = match fs::read(&binary_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "  Update to floo {} staged but could not be read: {e}",
                meta.version
            );
            eprintln!("{MANUAL_UPDATE_HINT}");
            clean_staged_dir();
            return;
        }
    };

    let install_path = match updater::resolve_install_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "  Update to floo {} could not be applied: {}",
                meta.version, e.message
            );
            if let Some(suggestion) = &e.suggestion {
                eprintln!("  {suggestion}");
            }
            clean_staged_dir();
            return;
        }
    };

    match updater::install_binary(&binary_bytes, &install_path) {
        Ok(()) => {
            let refreshed = crate::commands::skills::refresh_skill_files();
            eprintln!("  Updated floo to {}.", meta.version);
            for path in &refreshed {
                eprintln!("  Refreshed agent skill at {path}");
            }
            clean_staged_dir();
        }
        Err(e) => {
            eprintln!(
                "  Update to floo {} could not be applied: {}",
                meta.version, e.message
            );
            if let Some(suggestion) = &e.suggestion {
                eprintln!("  {suggestion}");
            }
            clean_staged_dir();
        }
    }
}

/// Phase 1: Spawn a background version check + download. Returns a handle for post-command notice.
pub fn spawn_check(current_version: &str) -> Option<VersionCheckHandle> {
    let cache = read_cache();

    if let Some(ref c) = cache {
        if cache_is_fresh(c) {
            if !is_newer(&c.latest_version, current_version) {
                return None;
            }
            if staged_binary_exists() {
                return None;
            }
            // Cache says newer version exists but no staged binary — spawn download-only
            let version = current_version.to_string();
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                // Re-fetch release JSON for asset URLs (cache only stores version string)
                let Some(client) = build_client(CHECK_TIMEOUT_SECS) else {
                    let _ = tx.send(CheckResult::UpToDate);
                    return;
                };
                let api_base = updater::releases_api_base();
                let Some((_, release_json)) = fetch_latest_release(&client, &api_base) else {
                    let _ = tx.send(CheckResult::UpToDate);
                    return;
                };
                let result = match download_and_stage(&version, &release_json) {
                    Some(v) => CheckResult::Downloaded(v),
                    None => CheckResult::UpToDate,
                };
                let _ = tx.send(result);
            });
            return Some(VersionCheckHandle { rx });
        }
    }

    // Cache stale or missing — check + possibly download
    let version = current_version.to_string();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let Some(client) = build_client(CHECK_TIMEOUT_SECS) else {
            let _ = tx.send(CheckResult::UpToDate);
            return;
        };
        let api_base = updater::releases_api_base();
        let Some((latest, release_json)) = fetch_latest_release(&client, &api_base) else {
            let _ = tx.send(CheckResult::UpToDate);
            return;
        };

        // Update cache regardless of whether version is newer
        write_cache(&VersionCache {
            latest_version: latest.clone(),
            checked_at: now_secs(),
        });

        if !is_newer(&latest, &version) {
            let _ = tx.send(CheckResult::UpToDate);
            return;
        }

        if staged_binary_exists() {
            let _ = tx.send(CheckResult::UpToDate);
            return;
        }

        let result = match download_and_stage(&version, &release_json) {
            Some(v) => CheckResult::Downloaded(v),
            None => CheckResult::UpToDate,
        };
        let _ = tx.send(result);
    });

    Some(VersionCheckHandle { rx })
}

impl VersionCheckHandle {
    pub fn print_notice(self) {
        if let Ok(CheckResult::Downloaded(version)) =
            self.rx.recv_timeout(Duration::from_millis(EXIT_WAIT_MS))
        {
            eprintln!("  floo {version} downloaded. Update will be applied on next run.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_basic() {
        assert!(is_newer("v0.3.0", "v0.2.0"));
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("v1.0.0", "v0.9.9"));
        assert!(is_newer("v0.2.1", "v0.2.0"));
    }

    #[test]
    fn test_is_newer_equal() {
        assert!(!is_newer("v0.2.0", "v0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
    }

    #[test]
    fn test_is_newer_older() {
        assert!(!is_newer("v0.1.0", "v0.2.0"));
        assert!(!is_newer("v0.2.0", "v0.3.0"));
    }

    #[test]
    fn test_is_newer_invalid() {
        assert!(!is_newer("invalid", "v0.2.0"));
        assert!(!is_newer("v0.2.0", "invalid"));
        assert!(!is_newer("", ""));
        assert!(!is_newer("v1.0", "v0.9.0"));
    }

    #[test]
    fn test_is_newer_v_prefix_mismatch() {
        assert!(is_newer("v0.3.0", "0.2.0"));
        assert!(is_newer("0.3.0", "v0.2.0"));
    }

    #[test]
    fn test_cache_is_fresh_recent() {
        let cache = VersionCache {
            latest_version: "v0.2.0".to_string(),
            checked_at: now_secs() - 100,
        };
        assert!(cache_is_fresh(&cache));
    }

    #[test]
    fn test_cache_is_stale() {
        let cache = VersionCache {
            latest_version: "v0.2.0".to_string(),
            checked_at: now_secs() - CACHE_TTL_SECS - 1,
        };
        assert!(!cache_is_fresh(&cache));
    }

    #[test]
    fn test_cache_serde_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_file = dir.path().join("version-check.json");

        let cache = VersionCache {
            latest_version: "v0.3.0".to_string(),
            checked_at: 1234567890,
        };
        let json = serde_json::to_string(&cache).unwrap();
        fs::write(&cache_file, &json).unwrap();

        let content = fs::read_to_string(&cache_file).unwrap();
        let loaded: VersionCache = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.latest_version, "v0.3.0");
        assert_eq!(loaded.checked_at, 1234567890);
    }

    #[test]
    fn test_staged_meta_serde_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let meta_file = dir.path().join("metadata.json");

        let meta = StagedUpdateMeta {
            version: "v0.4.0".to_string(),
            staged_at: 9999999,
        };
        let json = serde_json::to_string(&meta).unwrap();
        fs::write(&meta_file, &json).unwrap();

        let content = fs::read_to_string(&meta_file).unwrap();
        let loaded: StagedUpdateMeta = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.version, "v0.4.0");
        assert_eq!(loaded.staged_at, 9999999);
    }

    #[test]
    fn test_orphaned_downloading_cleaned_when_no_meta() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join(STAGED_DIR_NAME);
        fs::create_dir_all(&staged).unwrap();

        let dl_path = staged.join(DOWNLOADING_NAME);
        fs::write(&dl_path, b"partial").unwrap();
        assert!(dl_path.exists());

        // No metadata.json exists — .downloading is orphaned and should be cleaned
        let meta_path = staged.join(STAGED_META_NAME);
        assert!(!meta_path.exists());
        if dl_path.exists() && !meta_path.exists() {
            let _ = fs::remove_file(&dl_path);
        }
        assert!(!dl_path.exists());
    }

    #[test]
    fn test_downloading_kept_when_meta_exists() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join(STAGED_DIR_NAME);
        fs::create_dir_all(&staged).unwrap();

        let dl_path = staged.join(DOWNLOADING_NAME);
        fs::write(&dl_path, b"partial").unwrap();
        fs::write(
            staged.join(STAGED_META_NAME),
            r#"{"version":"v1.0.0","staged_at":0}"#,
        )
        .unwrap();

        // metadata.json exists — .downloading should NOT be removed
        let meta_path = staged.join(STAGED_META_NAME);
        if dl_path.exists() && !meta_path.exists() {
            let _ = fs::remove_file(&dl_path);
        }
        assert!(dl_path.exists());
    }
}
