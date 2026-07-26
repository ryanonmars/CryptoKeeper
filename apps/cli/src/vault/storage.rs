use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs4::FileExt;
use rand::RngCore;
use zeroize::Zeroizing;

use crate::crypto::{cipher, kdf};
use crate::error::{Result, TermKeyError};
use crate::vault::format::{validate_kdf_params, MAX_CIPHERTEXT_LEN};
use crate::vault::model::{BackupHeader, EntryMeta, VaultData, VaultHeader};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AtomicReplaceFailurePoint {
    Permissions,
    Write,
    TempSync,
    Rename,
    ParentSync,
}

#[cfg(test)]
thread_local! {
    static ATOMIC_REPLACE_FAILURE: std::cell::Cell<Option<AtomicReplaceFailurePoint>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn fail_next_atomic_replace_at(failure_point: AtomicReplaceFailurePoint) {
    ATOMIC_REPLACE_FAILURE.with(|injected| {
        assert!(
            injected.replace(Some(failure_point)).is_none(),
            "an atomic replacement failure is already pending"
        );
    });
}

#[cfg(test)]
fn fail_if_injected(failure_point: AtomicReplaceFailurePoint) -> Result<()> {
    ATOMIC_REPLACE_FAILURE.with(|injected| {
        if injected.get() == Some(failure_point) {
            injected.set(None);
            Err(std::io::Error::other(format!(
                "injected atomic replacement failure at {failure_point:?}"
            ))
            .into())
        } else {
            Ok(())
        }
    })
}

#[cfg(not(test))]
fn fail_if_injected(_failure_point: AtomicReplaceFailurePoint) -> Result<()> {
    Ok(())
}

/// Get the vault directory path, respecting TERMKEY_VAULT_DIR env var.
pub fn vault_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TERMKEY_VAULT_DIR") {
        PathBuf::from(dir)
    } else {
        dirs_fallback()
    }
}

fn dirs_fallback() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".termkey")
}

pub fn migrate_vault_if_needed() {
    // No-op: legacy vault path migration has been removed.
}

pub fn vault_path() -> PathBuf {
    vault_dir().join("vault.ck")
}

pub fn vault_exists() -> bool {
    vault_path().exists()
}

/// Delete the vault file and any leftover .tmp file.
pub fn delete_vault() -> Result<()> {
    delete_vault_at(&vault_path())
}

fn delete_vault_at(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    let tmp = path.with_extension("tmp");
    if tmp.exists() {
        let _ = fs::remove_file(&tmp); // best-effort
    }
    Ok(())
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

pub(crate) fn with_vault_lock<T>(
    vault_path: &Path,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let parent = parent_dir(vault_path);
    let lock_path = parent.join("vault.lock");
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        // The lock file is only an inode used for advisory locking; preserve its contents.
        .truncate(false)
        .open(&lock_path)?;
    set_file_permissions(&lock_path)?;
    FileExt::lock(&lock_file)?;
    operation()
}

/// Atomically replace `path` with `data`.
///
/// The successful rename is the point of no return: every returned error occurs before
/// rename and therefore leaves the original target unchanged. After rename succeeds,
/// the parent directory is synced on Unix on a best-effort basis, and this function
/// returns success so callers keep their in-memory state aligned with the committed
/// target.
pub(crate) fn atomic_replace(path: &Path, data: &[u8]) -> Result<()> {
    let parent = parent_dir(path);
    let parent_directory = ParentDirectory::open(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(TermKeyError::InvalidVaultFormat)?;
    let (temp_path, mut temp_file) = create_unique_temp_file(parent, file_name)?;

    let before_rename = (|| {
        fail_if_injected(AtomicReplaceFailurePoint::Write)?;
        temp_file.write_all(data)?;
        fail_if_injected(AtomicReplaceFailurePoint::TempSync)?;
        temp_file.sync_all()?;
        drop(temp_file);
        fail_if_injected(AtomicReplaceFailurePoint::Rename)?;
        fs::rename(&temp_path, path)?;
        Ok(())
    })();

    if let Err(error) = before_rename {
        let _ = fs::remove_file(temp_path);
        return Err(error);
    }

    let _parent_sync = fail_if_injected(AtomicReplaceFailurePoint::ParentSync)
        .and_then(|_| parent_directory.sync());
    Ok(())
}

fn create_unique_temp_file(parent: &Path, file_name: &str) -> Result<(PathBuf, File)> {
    for _ in 0..128 {
        let suffix = rand::thread_rng().next_u64();
        let temp_path = parent.join(format!(".{file_name}.tmp-{suffix:016x}"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => {
                if let Err(error) = fail_if_injected(AtomicReplaceFailurePoint::Permissions)
                    .and_then(|_| set_file_permissions(&temp_path))
                {
                    drop(file);
                    fs::remove_file(&temp_path)?;
                    return Err(error);
                }
                return Ok((temp_path, file));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not create a unique vault temporary file",
    )
    .into())
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

struct ParentDirectory {
    #[cfg(unix)]
    handle: File,
}

impl ParentDirectory {
    fn open(parent: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                handle: File::open(parent)?,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = parent;
            Ok(Self {})
        }
    }

    fn sync(&self) -> Result<()> {
        #[cfg(unix)]
        {
            self.handle.sync_all()?;
        }
        Ok(())
    }
}

/// Read entry metadata (names, network, type, notes) without password. Returns empty for v1 vaults.
pub fn read_metadata(path: &Path) -> Result<Vec<EntryMeta>> {
    let data = fs::read(path)?;
    if data.len() < 12 {
        return Ok(Vec::new());
    }
    if &data[0..4] != VaultHeader::MAGIC {
        return Ok(Vec::new());
    }
    let version = u32::from_le_bytes(data[4..8].try_into().unwrap());
    if version != VaultHeader::FORMAT_VERSION_V2 {
        return Ok(Vec::new());
    }
    let meta_len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    if data.len() < 12 + meta_len {
        return Ok(Vec::new());
    }
    let meta_json = std::str::from_utf8(&data[12..12 + meta_len])
        .map_err(|_| TermKeyError::InvalidVaultFormat)?;
    let meta: Vec<EntryMeta> =
        serde_json::from_str(meta_json).map_err(|_| TermKeyError::InvalidVaultFormat)?;
    Ok(meta)
}

/// Read vault metadata without password. Returns empty list if vault doesn't exist or is v1.
pub fn read_vault_metadata() -> Result<Vec<EntryMeta>> {
    let path = vault_path();
    if !path.exists() {
        return Err(TermKeyError::VaultNotFound);
    }
    read_metadata(&path)
}

/// Encrypt and write vault data to disk atomically.
pub fn write_vault(vault: &VaultData, password: &[u8], path: &Path) -> Result<()> {
    vault.validate()?;
    with_vault_lock(path, || {
        if path.exists() {
            let current = fs::read(path)?;
            if current.get(4..8)
                == Some(
                    crate::vault::format::FORMAT_VERSION_V3
                        .to_le_bytes()
                        .as_slice(),
                )
            {
                let opened =
                    crate::vault::format::decode_v3(password, &current, VaultHeader::MAGIC)?;
                if vault.revision != opened.vault.revision {
                    return Err(TermKeyError::VaultConflict);
                }
                let mut next = vault.clone();
                next.revision = opened
                    .vault
                    .revision
                    .checked_add(1)
                    .ok_or(TermKeyError::VaultConflict)?;
                let encoded = crate::vault::format::encode_v3(
                    &next,
                    password,
                    VaultHeader::MAGIC,
                    Some(&opened.dek),
                    Some(opened.vault_id),
                )?;
                return atomic_replace(path, &encoded);
            }
        }
        write_encrypted_file(vault, password, path, VaultHeader::MAGIC)
    })
}

/// Encrypt and write backup file.
pub fn write_backup(vault: &VaultData, password: &[u8], path: &Path) -> Result<()> {
    vault.validate()?;
    let encoded =
        crate::vault::format::encode_v3(vault, password, BackupHeader::MAGIC, None, None)?;
    atomic_replace(path, &encoded)
}

fn write_encrypted_file(
    vault: &VaultData,
    password: &[u8],
    path: &Path,
    magic: &[u8; 4],
) -> Result<()> {
    vault.validate()?;
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

    let ciphertext = cipher::encrypt(&key, &nonce, &plaintext)?;
    let ct_len = ciphertext.len() as u32;

    let mut data = Vec::new();
    data.extend_from_slice(magic);

    if magic == VaultHeader::MAGIC {
        let meta = vault.metadata();
        let meta_json = serde_json::to_vec(&meta)?;
        let meta_len = meta_json.len() as u32;
        data.extend_from_slice(&VaultHeader::FORMAT_VERSION_V2.to_le_bytes());
        data.extend_from_slice(&meta_len.to_le_bytes());
        data.extend_from_slice(&meta_json);
    } else {
        data.extend_from_slice(&VaultHeader::FORMAT_VERSION_V1.to_le_bytes());
    }

    data.extend_from_slice(&salt);
    data.extend_from_slice(&kdf::DEFAULT_M_COST.to_le_bytes());
    data.extend_from_slice(&kdf::DEFAULT_T_COST.to_le_bytes());
    data.extend_from_slice(&kdf::DEFAULT_P_COST.to_le_bytes());
    data.extend_from_slice(&nonce);
    data.extend_from_slice(&ct_len.to_le_bytes());
    data.extend_from_slice(&ciphertext);

    atomic_replace(path, &data)
}

/// Read and decrypt vault from disk.
pub fn read_vault(password: &[u8], path: &Path) -> Result<VaultData> {
    let bytes = fs::read(path)?;
    if bytes.get(4..8)
        == Some(
            crate::vault::format::FORMAT_VERSION_V3
                .to_le_bytes()
                .as_slice(),
        )
    {
        return Ok(crate::vault::format::decode_v3(password, &bytes, VaultHeader::MAGIC)?.vault);
    }
    read_encrypted_file(password, path, VaultHeader::MAGIC)
}

/// Read and decrypt backup from disk.
pub fn read_backup(password: &[u8], path: &Path) -> Result<VaultData> {
    let bytes = fs::read(path)?;
    if bytes.get(4..8)
        == Some(
            crate::vault::format::FORMAT_VERSION_V3
                .to_le_bytes()
                .as_slice(),
        )
    {
        return Ok(crate::vault::format::decode_v3(password, &bytes, BackupHeader::MAGIC)?.vault);
    }
    read_legacy_vault(password, &bytes, BackupHeader::MAGIC)
}

fn read_encrypted_file(
    password: &[u8],
    path: &Path,
    expected_magic: &[u8; 4],
) -> Result<VaultData> {
    let data = fs::read(path)?;
    read_legacy_vault(password, &data, expected_magic)
}

pub(crate) fn read_legacy_vault(
    password: &[u8],
    data: &[u8],
    expected_magic: &[u8; 4],
) -> Result<VaultData> {
    if data.len() < 8 {
        return Err(TermKeyError::InvalidVaultFormat);
    }

    if data.get(..4) != Some(expected_magic.as_slice()) {
        return Err(TermKeyError::InvalidVaultFormat);
    }

    let version = read_u32(data, 4)?;
    let salt_offset = match version {
        VaultHeader::FORMAT_VERSION_V1 => 8,
        VaultHeader::FORMAT_VERSION_V2 => {
            let meta_len = usize::try_from(read_u32(data, 8)?)
                .map_err(|_| TermKeyError::InvalidVaultFormat)?;
            12usize
                .checked_add(meta_len)
                .ok_or(TermKeyError::InvalidVaultFormat)?
        }
        unsupported => return Err(TermKeyError::UnsupportedVaultVersion(unsupported)),
    };
    let ct_offset = salt_offset
        .checked_add(72)
        .ok_or(TermKeyError::InvalidVaultFormat)?;

    if data.len() < ct_offset {
        return Err(TermKeyError::InvalidVaultFormat);
    }

    let salt = read_array::<32>(data, salt_offset)?;
    let m_cost = read_u32(data, salt_offset + 32)?;
    let t_cost = read_u32(data, salt_offset + 36)?;
    let p_cost = read_u32(data, salt_offset + 40)?;
    validate_kdf_params(m_cost, t_cost, p_cost)?;
    let nonce = read_array::<24>(data, salt_offset + 44)?;
    let ct_len = usize::try_from(read_u32(data, salt_offset + 68)?)
        .map_err(|_| TermKeyError::VaultTooLarge)?;
    if ct_len > MAX_CIPHERTEXT_LEN {
        return Err(TermKeyError::VaultTooLarge);
    }
    if ct_len < 16 {
        return Err(TermKeyError::InvalidVaultFormat);
    }
    let ciphertext_end = ct_offset
        .checked_add(ct_len)
        .ok_or(TermKeyError::VaultTooLarge)?;
    if data.len() != ciphertext_end {
        return Err(TermKeyError::InvalidVaultFormat);
    }

    let ciphertext = data
        .get(ct_offset..ciphertext_end)
        .ok_or(TermKeyError::InvalidVaultFormat)?;

    let key = kdf::derive_key(password, &salt, m_cost, t_cost, p_cost)?;
    let plaintext = cipher::decrypt(&key, &nonce, ciphertext)?;
    let mut vault: VaultData =
        serde_json::from_slice(&plaintext).map_err(|_| TermKeyError::InvalidVaultFormat)?;
    vault
        .normalize_legacy_entry_names()
        .map_err(|_| TermKeyError::InvalidVaultFormat)?;

    Ok(vault)
}

fn read_array<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N]> {
    let end = offset
        .checked_add(N)
        .ok_or(TermKeyError::InvalidVaultFormat)?;
    data.get(offset..end)
        .ok_or(TermKeyError::InvalidVaultFormat)?
        .try_into()
        .map_err(|_| TermKeyError::InvalidVaultFormat)
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(read_array::<4>(data, offset)?))
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
            public_address: None,
            username: None,
            url: None,
            site_rules: Vec::new(),
            notes: "Test note".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        });
        vault
    }

    fn encode_legacy_unchecked(
        vault: &VaultData,
        password: &[u8],
        magic: &[u8; 4],
        version: u32,
    ) -> Vec<u8> {
        let plaintext = Zeroizing::new(serde_json::to_vec(vault).unwrap());
        let salt = kdf::generate_salt();
        let nonce = cipher::generate_nonce();
        let key = kdf::derive_key(
            password,
            &salt,
            kdf::DEFAULT_M_COST,
            kdf::DEFAULT_T_COST,
            kdf::DEFAULT_P_COST,
        )
        .unwrap();
        let ciphertext = cipher::encrypt(&key, &nonce, &plaintext).unwrap();

        let mut data = Vec::new();
        data.extend_from_slice(magic);
        data.extend_from_slice(&version.to_le_bytes());
        if version == VaultHeader::FORMAT_VERSION_V2 {
            let metadata = serde_json::to_vec(&vault.metadata()).unwrap();
            data.extend_from_slice(&u32::try_from(metadata.len()).unwrap().to_le_bytes());
            data.extend_from_slice(&metadata);
        }
        data.extend_from_slice(&salt);
        data.extend_from_slice(&kdf::DEFAULT_M_COST.to_le_bytes());
        data.extend_from_slice(&kdf::DEFAULT_T_COST.to_le_bytes());
        data.extend_from_slice(&kdf::DEFAULT_P_COST.to_le_bytes());
        data.extend_from_slice(&nonce);
        data.extend_from_slice(&u32::try_from(ciphertext.len()).unwrap().to_le_bytes());
        data.extend_from_slice(&ciphertext);
        data
    }

    fn legacy_vault_with_conflicting_names() -> VaultData {
        let template = test_vault().entries.remove(0);
        let names = [" Alpha ", "", "alpha", " untitled ", "ALPHA (2)", "   "];
        let entries = names
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                let mut entry = template.clone();
                entry.name = name.to_string();
                entry.secret = format!("secret-{index}");
                entry
            })
            .collect();
        VaultData {
            entries,
            version: 1,
            revision: 0,
        }
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
        let encoded = fs::read(&path).unwrap();
        assert_eq!(encoded.get(..4), Some(BackupHeader::MAGIC.as_slice()));
        assert_eq!(
            u32::from_le_bytes(encoded[4..8].try_into().unwrap()),
            crate::vault::format::FORMAT_VERSION_V3
        );
        let loaded = read_backup(password, &path).unwrap();

        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].name, "Test Key");
    }

    #[test]
    fn v3_backup_header_is_authenticated() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("backup.ck");
        let password = b"backup-pass";
        let vault = test_vault();
        let mut encoded =
            crate::vault::format::encode_v3(&vault, password, BackupHeader::MAGIC, None, None)
                .unwrap();
        fs::write(&path, encoded.as_slice()).unwrap();

        assert_eq!(
            read_backup(password, &path).unwrap().entries[0].name,
            "Test Key"
        );

        encoded[8] ^= 1;
        fs::write(&path, encoded.as_slice()).unwrap();
        assert!(matches!(
            read_backup(password, &path),
            Err(TermKeyError::DecryptionFailed)
        ));
    }

    #[test]
    fn v3_backup_rejects_out_of_policy_kdf_before_derivation() {
        const V3_M_COST_OFFSET: usize = 4 + 4 + 16 + 32;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("backup.ck");
        let vault = test_vault();
        let mut encoded = crate::vault::format::encode_v3(
            &vault,
            b"backup-pass",
            BackupHeader::MAGIC,
            None,
            None,
        )
        .unwrap();
        encoded[V3_M_COST_OFFSET..V3_M_COST_OFFSET + 4]
            .copy_from_slice(&(crate::vault::format::MAX_M_COST + 1).to_le_bytes());
        fs::write(&path, encoded.as_slice()).unwrap();

        assert!(matches!(
            read_backup(b"backup-pass", &path),
            Err(TermKeyError::KdfParametersOutOfPolicy)
        ));
    }

    #[test]
    fn v3_backup_rejects_oversized_ciphertext_before_allocation() {
        const V3_CIPHERTEXT_LEN_OFFSET: usize = 4 + 4 + 16 + 32 + 4 + 4 + 4 + 24 + 48 + 24;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("backup.ck");
        let vault = test_vault();
        let mut encoded = crate::vault::format::encode_v3(
            &vault,
            b"backup-pass",
            BackupHeader::MAGIC,
            None,
            None,
        )
        .unwrap();
        encoded[V3_CIPHERTEXT_LEN_OFFSET..V3_CIPHERTEXT_LEN_OFFSET + 4]
            .copy_from_slice(&((MAX_CIPHERTEXT_LEN as u32) + 1).to_le_bytes());
        fs::write(&path, encoded.as_slice()).unwrap();

        assert!(matches!(
            read_backup(b"backup-pass", &path),
            Err(TermKeyError::VaultTooLarge)
        ));
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

    #[test]
    fn test_delete_vault_removes_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let vault = test_vault();
        write_vault(&vault, b"password", &path).unwrap();
        assert!(path.exists());
        delete_vault_at(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_delete_vault_removes_tmp_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, b"leftover").unwrap();
        delete_vault_at(&path).unwrap(); // vault.ck doesn't exist, tmp does
        assert!(!tmp.exists());
    }

    #[test]
    fn test_delete_vault_nonexistent_is_ok() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        // Neither file exists — should not error
        let result = delete_vault_at(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn unknown_legacy_version_is_rejected() {
        let mut bytes = Vec::from(VaultHeader::MAGIC.as_slice());
        bytes.extend_from_slice(&99u32.to_le_bytes());

        assert!(matches!(
            read_legacy_vault(b"password", &bytes, VaultHeader::MAGIC),
            Err(TermKeyError::UnsupportedVaultVersion(99))
        ));
    }

    #[test]
    fn legacy_v1_and_v2_decode_normalizes_names_without_losing_entries() {
        let vault = legacy_vault_with_conflicting_names();

        for version in [
            VaultHeader::FORMAT_VERSION_V1,
            VaultHeader::FORMAT_VERSION_V2,
        ] {
            let encoded = encode_legacy_unchecked(&vault, b"password", VaultHeader::MAGIC, version);
            let decoded = read_legacy_vault(b"password", &encoded, VaultHeader::MAGIC).unwrap();

            assert_eq!(decoded.entries.len(), 6);
            assert_eq!(
                decoded
                    .entries
                    .iter()
                    .map(|entry| entry.name.as_str())
                    .collect::<Vec<_>>(),
                [
                    "Alpha",
                    "Untitled",
                    "alpha (2)",
                    "untitled (2)",
                    "ALPHA (2) (2)",
                    "Untitled (3)",
                ]
            );
            assert_eq!(
                decoded
                    .entries
                    .iter()
                    .map(|entry| entry.secret.as_str())
                    .collect::<Vec<_>>(),
                ["secret-0", "secret-1", "secret-2", "secret-3", "secret-4", "secret-5",]
            );
        }
    }

    #[test]
    fn legacy_backup_read_uses_the_same_name_normalization() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("legacy-backup.ck");
        let vault = legacy_vault_with_conflicting_names();
        let encoded = encode_legacy_unchecked(
            &vault,
            b"password",
            BackupHeader::MAGIC,
            VaultHeader::FORMAT_VERSION_V1,
        );
        fs::write(&path, encoded).unwrap();

        let decoded = read_backup(b"password", &path).unwrap();

        assert_eq!(decoded.entries.len(), 6);
        assert_eq!(decoded.entries[0].name, "Alpha");
        assert_eq!(decoded.entries[1].name, "Untitled");
        assert_eq!(decoded.entries[5].name, "Untitled (3)");
    }

    #[test]
    fn legacy_decode_rejects_irreparable_protected_entry_as_corruption() {
        let mut vault = test_vault();
        vault.entries[0].has_secondary_password = true;
        let encoded = encode_legacy_unchecked(
            &vault,
            b"password",
            VaultHeader::MAGIC,
            VaultHeader::FORMAT_VERSION_V2,
        );

        assert!(matches!(
            read_legacy_vault(b"password", &encoded, VaultHeader::MAGIC),
            Err(TermKeyError::InvalidVaultFormat)
        ));
    }

    #[test]
    fn legacy_write_boundaries_reject_invalid_vault_data() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault.ck");
        let backup_path = dir.path().join("backup.ck");
        let mut vault = test_vault();
        vault.entries[0].name = " untrimmed ".to_string();

        assert!(matches!(
            write_vault(&vault, b"password", &vault_path),
            Err(TermKeyError::InvalidEntry(_))
        ));
        assert!(matches!(
            write_backup(&vault, b"password", &backup_path),
            Err(TermKeyError::InvalidEntry(_))
        ));
        assert!(!vault_path.exists());
        assert!(!backup_path.exists());
    }

    #[test]
    fn permissions_failure_removes_unique_temp_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        fs::write(&path, b"original").unwrap();
        fail_next_atomic_replace_at(AtomicReplaceFailurePoint::Permissions);

        assert!(atomic_replace(&path, b"replacement").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(fs::read_dir(dir.path()).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".vault.ck.tmp-")));
    }
}
