use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use fs4::FileExt;

use crate::config::model::Config;
use crate::error::{Result, TermKeyError};

#[cfg(test)]
struct ConfigLockAttemptHook {
    lock_path: PathBuf,
    probe_result: std::sync::mpsc::Sender<std::result::Result<(), fs4::TryLockError>>,
    continue_to_lock: std::sync::mpsc::Receiver<()>,
}

#[cfg(test)]
fn config_lock_attempt_hook() -> &'static std::sync::Mutex<Option<ConfigLockAttemptHook>> {
    static HOOK: std::sync::OnceLock<std::sync::Mutex<Option<ConfigLockAttemptHook>>> =
        std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn install_next_config_lock_attempt_hook(
    config_path: &Path,
) -> (
    std::sync::mpsc::Receiver<std::result::Result<(), fs4::TryLockError>>,
    std::sync::mpsc::Sender<()>,
) {
    let (probe_result_tx, probe_result_rx) = std::sync::mpsc::channel();
    let (continue_to_lock_tx, continue_to_lock_rx) = std::sync::mpsc::channel();
    let mut hook = config_lock_attempt_hook().lock().unwrap();
    assert!(
        hook.is_none(),
        "a config lock-attempt hook is already installed"
    );
    *hook = Some(ConfigLockAttemptHook {
        lock_path: config_path.with_extension("lock"),
        probe_result: probe_result_tx,
        continue_to_lock: continue_to_lock_rx,
    });
    (probe_result_rx, continue_to_lock_tx)
}

#[cfg(test)]
fn run_config_lock_attempt_hook(lock_path: &Path, lock_file: &std::fs::File) {
    let mut installed = config_lock_attempt_hook().lock().unwrap();
    let hook = if installed
        .as_ref()
        .is_some_and(|hook| hook.lock_path == lock_path)
    {
        installed.take()
    } else {
        None
    };
    drop(installed);
    if let Some(hook) = hook {
        let probe_result = FileExt::try_lock(lock_file);
        if probe_result.is_ok() {
            FileExt::unlock(lock_file).unwrap();
        }
        hook.probe_result.send(probe_result).unwrap();
        hook.continue_to_lock.recv().unwrap();
    }
}

#[cfg(not(test))]
fn run_config_lock_attempt_hook(_lock_path: &Path, _lock_file: &std::fs::File) {}

#[cfg(test)]
thread_local! {
    static FAIL_NEXT_CONFIG_REPLACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn fail_next_config_replace_for_test() {
    FAIL_NEXT_CONFIG_REPLACE.with(|fail| {
        assert!(
            !fail.replace(true),
            "a config replacement failure is already pending"
        );
    });
}

#[cfg(test)]
fn fail_config_replace_if_injected() -> Result<()> {
    FAIL_NEXT_CONFIG_REPLACE.with(|fail| {
        if fail.replace(false) {
            Err(std::io::Error::other("injected config replacement failure").into())
        } else {
            Ok(())
        }
    })
}

#[cfg(not(test))]
fn fail_config_replace_if_injected() -> Result<()> {
    Ok(())
}

/// Get the config file path (~/.termkey/config.json).
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
        serde_json::from_str(&data).map_err(|e| TermKeyError::ConfigError(e.to_string()))?;
    Ok(config)
}

/// Load config from disk. Returns default if file doesn't exist.
pub fn load_config() -> Result<Config> {
    load_config_from(&config_path())
}

/// Save config to a specific path atomically with 0600 permissions.
pub fn save_config_to(config: &Config, path: &Path) -> Result<()> {
    with_config_lock(path, || {
        let json = serialize_config(config)?;
        crate::vault::storage::atomic_replace(path, json.as_bytes())
    })
}

/// Save config to disk atomically with 0600 permissions.
pub fn save_config(config: &Config) -> Result<()> {
    save_config_to(config, &config_path())
}

/// Load the latest config while holding its advisory lock, update selected fields,
/// and atomically replace the file only when the update changed it.
pub fn update_config_at<T>(
    path: &Path,
    update: impl FnOnce(&mut Config) -> Result<T>,
) -> Result<T> {
    with_config_lock(path, || {
        let mut config = load_config_from(path)?;
        let original = config.clone();
        let result = update(&mut config)?;
        if config != original {
            let json = serialize_config(&config)?;
            fail_config_replace_if_injected()?;
            crate::vault::storage::atomic_replace(path, json.as_bytes())?;
        }
        Ok(result)
    })
}

/// Update selected fields in the default config using fresh locked state.
pub fn update_config<T>(update: impl FnOnce(&mut Config) -> Result<T>) -> Result<T> {
    update_config_at(&config_path(), update)
}

fn serialize_config(config: &Config) -> Result<String> {
    serde_json::to_string_pretty(config).map_err(|e| TermKeyError::ConfigError(e.to_string()))
}

fn with_config_lock<T>(path: &Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    let lock_path = path.with_extension("lock");
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    set_lock_file_permissions(&lock_path)?;
    run_config_lock_attempt_hook(&lock_path, &lock_file);
    FileExt::lock(&lock_file)?;
    operation()
}

#[cfg(unix)]
fn set_lock_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_lock_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

/// Delete the config file and any leftover .tmp file.
pub fn delete_config() -> Result<()> {
    delete_config_at(&config_path())
}

fn delete_config_at(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    let tmp = path.with_extension("tmp");
    if tmp.exists() {
        let _ = fs::remove_file(&tmp); // best-effort
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{RecoveryConfig, RecoveryConfigV2};
    use crate::vault::format::VaultId;
    use std::sync::mpsc;
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

    #[test]
    fn test_delete_config_removes_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        let config = Config::default();
        save_config_to(&config, &path).unwrap();
        assert!(path.exists());
        delete_config_at(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_delete_config_removes_tmp_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, b"leftover").unwrap();
        delete_config_at(&path).unwrap();
        assert!(!tmp.exists());
    }

    #[test]
    fn test_delete_config_nonexistent_is_ok() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        let result = delete_config_at(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn config_field_updates_are_serialized_and_merge_fresh_state() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        save_config_to(&Config::default(), &path).unwrap();

        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let first_path = path.clone();
        let first = std::thread::spawn(move || {
            update_config_at(&first_path, |config| {
                config.clipboard_timeout_secs = 77;
                first_entered_tx.send(()).unwrap();
                release_first_rx.recv().unwrap();
                Ok(())
            })
            .unwrap();
        });

        first_entered_rx.recv().unwrap();
        let second_path = path.clone();
        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let (second_lock_probe_rx, continue_second_to_lock_tx) =
            install_next_config_lock_attempt_hook(&path);
        let second = std::thread::spawn(move || {
            update_config_at(&second_path, |config| {
                second_entered_tx.send(()).unwrap();
                config.recovery = Some(RecoveryConfig::V2(RecoveryConfigV2 {
                    version: 2,
                    vault_id: VaultId([0x22; 16]),
                    salt: vec![0x33; 32],
                    nonce: vec![0x44; 24],
                    wrapped_dek: vec![0x55; 48],
                }));
                Ok(())
            })
            .unwrap();
        });

        let second_lock_probe = second_lock_probe_rx.recv().unwrap();
        continue_second_to_lock_tx.send(()).unwrap();
        release_first_tx.send(()).unwrap();
        first.join().unwrap();
        second_entered_rx.recv().unwrap();
        second.join().unwrap();
        assert!(
            matches!(&second_lock_probe, Err(fs4::TryLockError::WouldBlock)),
            "the config lock admitted a second updater while the first held it: {second_lock_probe:?}"
        );

        let persisted = load_config_from(&path).unwrap();
        assert_eq!(persisted.clipboard_timeout_secs, 77);
        assert!(matches!(
            persisted.recovery,
            Some(RecoveryConfig::V2(RecoveryConfigV2 { vault_id, .. }))
                if vault_id == VaultId([0x22; 16])
        ));
    }

    #[test]
    fn failed_field_update_does_not_persist_partial_config() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        let original = Config {
            clipboard_timeout_secs: 19,
            ..Config::default()
        };
        save_config_to(&original, &path).unwrap();

        let result: Result<()> = update_config_at(&path, |config| {
            config.clipboard_timeout_secs = 99;
            Err(TermKeyError::ConfigError("injected update failure".into()))
        });

        assert!(matches!(result, Err(TermKeyError::ConfigError(_))));
        assert_eq!(load_config_from(&path).unwrap().clipboard_timeout_secs, 19);
    }
}
