use std::fs;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

use crate::crypto::{cipher, kdf};
use crate::error::{CryptoKeeperError, Result};
use crate::vault::model::{BackupHeader, VaultData, VaultHeader};

/// Get the vault directory path, respecting CRYPTOKEEPER_VAULT_DIR env var.
pub fn vault_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CRYPTOKEEPER_VAULT_DIR") {
        PathBuf::from(dir)
    } else {
        dirs_fallback()
    }
}

fn dirs_fallback() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cryptokeeper")
}

pub fn vault_path() -> PathBuf {
    vault_dir().join("vault.ck")
}

pub fn vault_exists() -> bool {
    vault_path().exists()
}

/// Ensure the vault directory exists with proper permissions.
pub fn ensure_vault_dir() -> Result<()> {
    let dir = vault_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
        set_dir_permissions(&dir)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

/// Encrypt and write vault data to disk atomically.
pub fn write_vault(vault: &VaultData, password: &[u8], path: &Path) -> Result<()> {
    write_encrypted_file(vault, password, path, VaultHeader::MAGIC)
}

/// Encrypt and write backup file.
pub fn write_backup(vault: &VaultData, password: &[u8], path: &Path) -> Result<()> {
    write_encrypted_file(vault, password, path, BackupHeader::MAGIC)
}

fn write_encrypted_file(
    vault: &VaultData,
    password: &[u8],
    path: &Path,
    magic: &[u8; 4],
) -> Result<()> {
    let plaintext = Zeroizing::new(serde_json::to_vec(vault)?);

    let salt = kdf::generate_salt();
    let nonce = cipher::generate_nonce();
    let key = kdf::derive_key(
        password,
        &salt,
        kdf::DEFAULT_M_COST,
        kdf::DEFAULT_T_COST,
        kdf::DEFAULT_P_COST,
    )?;

    let ciphertext = cipher::encrypt(&*key, &nonce, &plaintext)?;
    let ct_len = ciphertext.len() as u32;

    let mut data = Vec::with_capacity(VaultHeader::HEADER_SIZE + ciphertext.len());
    data.extend_from_slice(magic);
    data.extend_from_slice(&1u32.to_le_bytes()); // format version
    data.extend_from_slice(&salt);
    data.extend_from_slice(&kdf::DEFAULT_M_COST.to_le_bytes());
    data.extend_from_slice(&kdf::DEFAULT_T_COST.to_le_bytes());
    data.extend_from_slice(&kdf::DEFAULT_P_COST.to_le_bytes());
    data.extend_from_slice(&nonce);
    data.extend_from_slice(&ct_len.to_le_bytes());
    data.extend_from_slice(&ciphertext);

    // Atomic write: write to temp file then rename
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, &data)?;
    set_file_permissions(&temp_path)?;
    fs::rename(&temp_path, path)?;

    Ok(())
}

/// Read and decrypt vault from disk.
pub fn read_vault(password: &[u8], path: &Path) -> Result<VaultData> {
    read_encrypted_file(password, path, VaultHeader::MAGIC)
}

/// Read and decrypt backup from disk.
pub fn read_backup(password: &[u8], path: &Path) -> Result<VaultData> {
    read_encrypted_file(password, path, BackupHeader::MAGIC)
}

fn read_encrypted_file(password: &[u8], path: &Path, expected_magic: &[u8; 4]) -> Result<VaultData> {
    let data = fs::read(path)?;

    if data.len() < VaultHeader::HEADER_SIZE {
        return Err(CryptoKeeperError::InvalidVaultFormat);
    }

    // Parse header
    let magic = &data[0..4];
    if magic != expected_magic {
        return Err(CryptoKeeperError::InvalidVaultFormat);
    }

    let _version = u32::from_le_bytes(data[4..8].try_into().unwrap());

    let mut salt = [0u8; 32];
    salt.copy_from_slice(&data[8..40]);

    let m_cost = u32::from_le_bytes(data[40..44].try_into().unwrap());
    let t_cost = u32::from_le_bytes(data[44..48].try_into().unwrap());
    let p_cost = u32::from_le_bytes(data[48..52].try_into().unwrap());

    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&data[52..76]);

    let ct_len = u32::from_le_bytes(data[76..80].try_into().unwrap()) as usize;

    if data.len() < VaultHeader::HEADER_SIZE + ct_len {
        return Err(CryptoKeeperError::InvalidVaultFormat);
    }

    let ciphertext = &data[80..80 + ct_len];

    // Derive key
    let key = kdf::derive_key(password, &salt, m_cost, t_cost, p_cost)?;

    // Decrypt
    let plaintext = cipher::decrypt(&*key, &nonce, ciphertext)?;

    // Deserialize
    let vault: VaultData = serde_json::from_slice(&plaintext)?;

    Ok(vault)
}

/// Prompt for master password and unlock the vault.
pub fn prompt_and_unlock() -> Result<(VaultData, Zeroizing<String>)> {
    if !vault_exists() {
        return Err(CryptoKeeperError::VaultNotFound);
    }

    let password = Zeroizing::new(
        rpassword::prompt_password("Master password: ")
            .map_err(|e| CryptoKeeperError::Io(e))?,
    );

    if password.is_empty() {
        return Err(CryptoKeeperError::EmptyPassword);
    }

    eprintln!("Unlocking vault...");
    let vault = read_vault(password.as_bytes(), &vault_path())?;

    Ok((vault, password))
}

/// Save vault with the given password.
pub fn save_vault(vault: &VaultData, password: &[u8]) -> Result<()> {
    write_vault(vault, password, &vault_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::model::{Entry, SecretType};
    use chrono::Utc;
    use tempfile::TempDir;

    fn test_vault() -> VaultData {
        let mut vault = VaultData::new();
        vault.entries.push(Entry {
            name: "Test Key".to_string(),
            secret: "0xdeadbeef".to_string(),
            secret_type: SecretType::PrivateKey,
            network: "Ethereum".to_string(),
            notes: "Test note".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        vault
    }

    #[test]
    fn test_vault_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let password = b"test-password";
        let vault = test_vault();

        write_vault(&vault, password, &path).unwrap();
        let loaded = read_vault(password, &path).unwrap();

        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].name, "Test Key");
        assert_eq!(loaded.entries[0].secret, "0xdeadbeef");
    }

    #[test]
    fn test_vault_wrong_password() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let vault = test_vault();

        write_vault(&vault, b"correct", &path).unwrap();
        let result = read_vault(b"wrong", &path);
        assert!(result.is_err());
    }

    #[test]
    fn test_backup_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("backup.ck");
        let password = b"backup-pass";
        let vault = test_vault();

        write_backup(&vault, password, &path).unwrap();
        let loaded = read_backup(password, &path).unwrap();

        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].name, "Test Key");
    }

    #[test]
    fn test_backup_wrong_magic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("backup.ck");
        let vault = test_vault();

        // Write as vault, try to read as backup
        write_vault(&vault, b"pass", &path).unwrap();
        let result = read_backup(b"pass", &path);
        assert!(result.is_err());
    }

    #[test]
    fn test_corrupted_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        fs::write(&path, b"too short").unwrap();
        let result = read_vault(b"pass", &path);
        assert!(result.is_err());
    }
}
