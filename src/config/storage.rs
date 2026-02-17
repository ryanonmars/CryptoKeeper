use std::fs;
use std::path::{Path, PathBuf};

use crate::config::model::Config;
use crate::error::{CryptoKeeperError, Result};

/// Get the config file path (~/.cryptokeeper/config.json).
pub fn config_path() -> PathBuf {
    crate::vault::storage::vault_dir().join("config.json")
}

/// Load config from a specific path. Returns default if file doesn't exist.
pub fn load_config_from(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let data = fs::read_to_string(path)?;
    let config: Config =
        serde_json::from_str(&data).map_err(|e| CryptoKeeperError::ConfigError(e.to_string()))?;
    Ok(config)
}

/// Load config from disk. Returns default if file doesn't exist.
pub fn load_config() -> Result<Config> {
    load_config_from(&config_path())
}

/// Save config to a specific path atomically with 0600 permissions.
pub fn save_config_to(config: &Config, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    let json = serde_json::to_string_pretty(config)
        .map_err(|e| CryptoKeeperError::ConfigError(e.to_string()))?;

    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, json.as_bytes())?;
    set_config_permissions(&temp_path)?;
    fs::rename(&temp_path, path)?;

    Ok(())
}

/// Save config to disk atomically with 0600 permissions.
pub fn save_config(config: &Config) -> Result<()> {
    save_config_to(config, &config_path())
}

#[cfg(unix)]
fn set_config_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_config_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_missing_config_returns_default() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        let config = load_config_from(&path).unwrap();
        assert!(!config.first_run_complete);
        assert_eq!(config.clipboard_timeout_secs, 10);
    }

    #[test]
    fn save_and_load_config_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");

        let config = Config {
            vault_path: "/test/vault.ck".to_string(),
            clipboard_timeout_secs: 20,
            first_run_complete: true,
            recovery: None,
        };
        save_config_to(&config, &path).unwrap();

        let loaded = load_config_from(&path).unwrap();
        assert_eq!(loaded.vault_path, "/test/vault.ck");
        assert_eq!(loaded.clipboard_timeout_secs, 20);
        assert!(loaded.first_run_complete);
    }
}
