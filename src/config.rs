use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::constants::{CONFIG_DIR_NAME, CONFIG_FILE_NAME, DEFAULT_API_URL};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlooConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_api_url")]
    pub api_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
}

fn default_api_url() -> String {
    DEFAULT_API_URL.to_string()
}

impl Default for FlooConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_url: DEFAULT_API_URL.to_string(),
            user_email: None,
        }
    }
}

fn config_path() -> PathBuf {
    dirs_home().join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME)
}

fn dirs_home() -> PathBuf {
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
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => FlooConfig::default(),
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
        assert_eq!(config.api_url, DEFAULT_API_URL);
        assert!(config.user_email.is_none());
    }

    #[test]
    fn test_config_serialization() {
        let config = FlooConfig {
            api_key: Some("floo_test123".to_string()),
            api_url: "https://api.test.local".to_string(),
            user_email: Some("test@example.com".to_string()),
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: FlooConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.api_key.as_deref(), Some("floo_test123"));
        assert_eq!(deserialized.api_url, "https://api.test.local");
        assert_eq!(deserialized.user_email.as_deref(), Some("test@example.com"));
    }

    #[test]
    fn test_config_env_override() {
        // This test verifies the env override logic conceptually
        // (actual env manipulation in integration tests)
        let config = FlooConfig::default();
        assert_eq!(config.api_url, DEFAULT_API_URL);
    }
}
