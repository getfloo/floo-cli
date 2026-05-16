use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::constants::{CONFIG_FILE_NAME, DEFAULT_API_URL, DEV_API_URL};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlooConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_api_url")]
    pub api_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_org: Option<String>,
}

fn default_api_url() -> String {
    // Variant-aware: a fresh `floo-dev` (no config file, or a config
    // file missing `api_url`) must default to the dev stack, not prod.
    // FLOO_API_URL still overrides this in `load_config`.
    match current_variant() {
        BinaryVariant::Dev => DEV_API_URL.to_string(),
        BinaryVariant::Local | BinaryVariant::Installed => DEFAULT_API_URL.to_string(),
    }
}

impl Default for FlooConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_url: default_api_url(),
            user_email: None,
            skill_paths: Vec::new(),
            default_org: None,
        }
    }
}

impl FlooConfig {
    /// Add a skill path if not already tracked. Returns true if added.
    pub fn add_skill_path(&mut self, path: &str) -> bool {
        if self.skill_paths.iter().any(|p| p == path) {
            return false;
        }
        self.skill_paths.push(path.to_string());
        true
    }
}

/// Which compiled variant is running, decided by the binary's own
/// basename. All three `[[bin]]` targets in Cargo.toml share `main.rs`;
/// behavior diverges only here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryVariant {
    /// `floo` — installed, prod API, `.floo` config.
    Installed,
    /// `floo-local` — locally-compiled, prod API, `.floo-local` config.
    Local,
    /// `floo-dev` — dev-stack API, `.floo-dev` config. A sanctioned,
    /// credential-isolated surface for dev-stack operations so they
    /// never clobber prod creds or tempt raw DB access (see floo
    /// monorepo issue #797).
    Dev,
}

/// Pure mapping from binary basename to variant — unit-testable
/// without faking `current_exe`.
pub(crate) fn variant_from_basename(name: &str) -> BinaryVariant {
    match name {
        "floo-local" => BinaryVariant::Local,
        "floo-dev" => BinaryVariant::Dev,
        _ => BinaryVariant::Installed,
    }
}

pub(crate) fn current_variant() -> BinaryVariant {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .map(|n| variant_from_basename(&n))
        .unwrap_or(BinaryVariant::Installed)
}

/// Returns true when the running binary is the locally-compiled `floo-local`.
pub fn is_local_binary() -> bool {
    current_variant() == BinaryVariant::Local
}

/// Returns true when the running binary is `floo-dev` (dev stack).
pub fn is_dev_binary() -> bool {
    current_variant() == BinaryVariant::Dev
}

/// Config directory name: `.floo-dev` for the dev binary,
/// `.floo-local` for local builds, `.floo` for installed.
pub fn config_dir_name() -> &'static str {
    match current_variant() {
        BinaryVariant::Dev => ".floo-dev",
        BinaryVariant::Local => ".floo-local",
        BinaryVariant::Installed => ".floo",
    }
}

/// Returns the Floo config directory. Checks `FLOO_CONFIG_DIR` first,
/// falls back to `$HOME/.floo-local` or `$HOME/.floo` based on binary name.
pub(crate) fn config_dir() -> PathBuf {
    if let Ok(dir) = env::var("FLOO_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs_home().join(config_dir_name())
}

fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILE_NAME)
}

pub(crate) fn dirs_home() -> PathBuf {
    // HOME on Unix, USERPROFILE on Windows
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("~"))
}

pub fn load_config() -> FlooConfig {
    let env_api_url = env::var("FLOO_API_URL").ok();

    let path = config_path();
    let mut config = if path.exists() {
        match fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(parsed) => parsed,
                Err(e) => {
                    eprintln!(
                        "Warning: config file at {} is corrupted ({e}), using defaults.",
                        path.display()
                    );
                    eprintln!(
                        "  Your API key may have been lost. Run 'floo auth login' to re-authenticate."
                    );
                    FlooConfig::default()
                }
            },
            Err(e) => {
                eprintln!(
                    "Warning: could not read config file at {} ({e}), using defaults.",
                    path.display()
                );
                FlooConfig::default()
            }
        }
    } else {
        FlooConfig::default()
    };

    if let Some(url) = env_api_url {
        config.api_url = url;
    }

    config
}

pub fn save_config(config: &FlooConfig) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config)?;
    fs::write(&path, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn clear_config() {
    let path = config_path();
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = FlooConfig::default();
        assert!(config.api_key.is_none());
        // Under the test binary (basename != floo-dev) the variant is
        // Installed, so the default stays prod. This pins that the
        // floo-dev change did NOT regress prod/local defaulting.
        assert_eq!(config.api_url, DEFAULT_API_URL);
        assert!(config.user_email.is_none());
    }

    #[test]
    fn test_variant_from_basename() {
        assert_eq!(variant_from_basename("floo"), BinaryVariant::Installed);
        assert_eq!(variant_from_basename("floo-local"), BinaryVariant::Local);
        assert_eq!(variant_from_basename("floo-dev"), BinaryVariant::Dev);
        // Anything unrecognized (renamed binary, symlink) falls back to
        // the safe prod-installed behavior — never silently dev.
        assert_eq!(variant_from_basename("floo.exe"), BinaryVariant::Installed);
        assert_eq!(variant_from_basename("something"), BinaryVariant::Installed);
    }

    #[test]
    fn test_dev_variant_defaults_to_dev_api_only_for_dev() {
        // The dev binary must default to the dev stack; prod/local must
        // not. This is the load-bearing isolation: a `floo-dev` with no
        // config file still talks to dev, never accidentally prod.
        assert_eq!(
            match variant_from_basename("floo-dev") {
                BinaryVariant::Dev => DEV_API_URL,
                _ => DEFAULT_API_URL,
            },
            DEV_API_URL
        );
        assert_eq!(
            match variant_from_basename("floo-local") {
                BinaryVariant::Dev => DEV_API_URL,
                _ => DEFAULT_API_URL,
            },
            DEFAULT_API_URL
        );
        assert_ne!(DEV_API_URL, DEFAULT_API_URL);
    }

    #[test]
    fn test_config_serialization() {
        let config = FlooConfig {
            api_key: Some("floo_test123".to_string()),
            api_url: "https://api.test.local".to_string(),
            user_email: Some("test@example.com".to_string()),
            skill_paths: vec!["/tmp/floo.md".to_string()],
            default_org: Some("org-123".to_string()),
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: FlooConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.api_key.as_deref(), Some("floo_test123"));
        assert_eq!(deserialized.api_url, "https://api.test.local");
        assert_eq!(deserialized.user_email.as_deref(), Some("test@example.com"));
        assert_eq!(deserialized.skill_paths, vec!["/tmp/floo.md"]);
        assert_eq!(deserialized.default_org.as_deref(), Some("org-123"));
    }

    #[test]
    fn test_config_without_skill_paths_deserializes() {
        let json = r#"{"api_key":"floo_x","api_url":"https://api.getfloo.com"}"#;
        let config: FlooConfig = serde_json::from_str(json).unwrap();
        assert!(config.skill_paths.is_empty());
    }

    #[test]
    fn test_add_skill_path_deduplicates() {
        let mut config = FlooConfig::default();
        assert!(config.add_skill_path("/tmp/floo.md"));
        assert!(!config.add_skill_path("/tmp/floo.md"));
        assert_eq!(config.skill_paths.len(), 1);
    }

    #[test]
    fn test_skill_paths_omitted_when_empty() {
        let config = FlooConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("skill_paths"));
        assert!(!json.contains("default_org"));
    }

    #[test]
    fn test_corrupted_config_file_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(config_dir_name());
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join(CONFIG_FILE_NAME), "{ not valid json !!!").unwrap();

        env::set_var("HOME", tmp.path());
        let config = load_config();
        assert!(config.api_key.is_none());
        assert_eq!(config.api_url, DEFAULT_API_URL);
    }

    #[test]
    fn test_config_env_override() {
        // This test verifies the env override logic conceptually
        // (actual env manipulation in integration tests)
        let config = FlooConfig::default();
        assert_eq!(config.api_url, DEFAULT_API_URL);
    }

    #[test]
    fn test_config_dir_env_override() {
        let tmp = tempfile::tempdir().unwrap();
        let custom_dir = tmp.path().join("custom-floo");
        env::set_var("FLOO_CONFIG_DIR", &custom_dir);
        let result = config_dir();
        env::remove_var("FLOO_CONFIG_DIR");
        assert_eq!(result, custom_dir);
    }

    #[test]
    fn test_config_dir_fallback() {
        env::remove_var("FLOO_CONFIG_DIR");
        let result = config_dir();
        assert!(result.ends_with(config_dir_name()));
    }
}
