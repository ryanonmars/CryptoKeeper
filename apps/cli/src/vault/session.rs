use std::fs;
use std::path::PathBuf;

use rand::RngCore;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::error::{Result, TermKeyError};
use crate::vault::format::{decode_v3, decode_v3_with_dek, encode_v3, VaultId, FORMAT_VERSION_V3};
use crate::vault::model::{VaultData, VaultHeader};
use crate::vault::storage::{atomic_replace, read_legacy_vault, with_vault_lock};

pub struct VaultSession {
    pub vault: VaultData,
    vault_id: VaultId,
    dek: Zeroizing<[u8; 32]>,
    master_password: Zeroizing<String>,
    expected_revision: u64,
    source_digest: [u8; 32],
    write_credentials_fresh: bool,
    path: PathBuf,
}

pub struct OpenOutcome {
    pub session: VaultSession,
    pub migrated_legacy: bool,
    pub recovery_notice: Option<String>,
}

impl VaultSession {
    pub fn prompt_and_open() -> Result<OpenOutcome> {
        if !crate::vault::storage::vault_exists() {
            return Err(TermKeyError::VaultNotFound);
        }
        let password = Zeroizing::new(
            rpassword::prompt_password("Master password: ").map_err(TermKeyError::Io)?,
        );
        if password.is_empty() {
            return Err(TermKeyError::EmptyPassword);
        }
        eprintln!("Unlocking vault...");
        let outcome = Self::open(password, crate::vault::storage::vault_path())?;
        if let Some(notice) = outcome.recovery_notice.as_deref() {
            eprintln!();
            eprintln!("Recovery notice: {notice}");
        }
        Ok(outcome)
    }

    pub fn create(vault: VaultData, password: Zeroizing<String>, path: PathBuf) -> Result<Self> {
        let lock_path = path.clone();
        with_vault_lock(&lock_path, || {
            if path.exists() {
                return Err(TermKeyError::VaultAlreadyExists(path.display().to_string()));
            }
            let encoded = encode_v3(&vault, password.as_bytes(), VaultHeader::MAGIC, None, None)?;
            let opened = decode_v3(password.as_bytes(), &encoded, VaultHeader::MAGIC)?;
            atomic_replace(&path, &encoded)?;
            Ok(Self {
                expected_revision: opened.vault.revision,
                source_digest: digest(&encoded),
                path,
                vault: opened.vault,
                vault_id: opened.vault_id,
                dek: opened.dek,
                master_password: password,
                write_credentials_fresh: true,
            })
        })
    }

    pub fn open(password: Zeroizing<String>, path: PathBuf) -> Result<OpenOutcome> {
        let lock_path = path.clone();
        let outcome = with_vault_lock(&lock_path, || {
            let bytes = fs::read(&path)?;
            let version = vault_version(&bytes)?;

            if version == FORMAT_VERSION_V3 {
                let opened = decode_v3(password.as_bytes(), &bytes, VaultHeader::MAGIC)?;
                let expected_revision = opened.vault.revision;
                return Ok(OpenOutcome {
                    session: Self {
                        vault: opened.vault,
                        vault_id: opened.vault_id,
                        dek: opened.dek,
                        master_password: password,
                        write_credentials_fresh: true,
                        expected_revision,
                        source_digest: digest(&bytes),
                        path: path.clone(),
                    },
                    migrated_legacy: false,
                    recovery_notice: None,
                });
            }

            let vault = read_legacy_vault(password.as_bytes(), &bytes, VaultHeader::MAGIC)?;
            let mut dek = Zeroizing::new([0u8; 32]);
            rand::thread_rng().fill_bytes(dek.as_mut());
            let mut vault_id_bytes = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut vault_id_bytes);
            let vault_id = VaultId(vault_id_bytes);
            let encoded = encode_v3(
                &vault,
                password.as_bytes(),
                VaultHeader::MAGIC,
                Some(&dek),
                Some(vault_id),
            )?;
            atomic_replace(&path, &encoded)?;
            let expected_revision = vault.revision;

            Ok(OpenOutcome {
                session: Self {
                    vault,
                    vault_id,
                    dek,
                    master_password: password,
                    write_credentials_fresh: true,
                    expected_revision,
                    source_digest: digest(&encoded),
                    path: path.clone(),
                },
                migrated_legacy: true,
                recovery_notice: None,
            })
        })?;

        if !outcome.migrated_legacy {
            return Ok(outcome);
        }

        let mut outcome = outcome;
        outcome.recovery_notice =
            match clear_legacy_recovery_after_migration(&path, outcome.session.vault_id()) {
                Ok(true) => None,
                Ok(false) => Some(
                    "Vault upgraded to the v3 format. Configure a new recovery phrase in Settings; legacy security-question recovery is no longer supported."
                        .to_string(),
                ),
                Err(error) => Some(format!(
                    "Vault unlocked and upgraded to the v3 format, but recovery settings could not be updated: {error}. Configure a new recovery phrase in Settings."
                )),
            };
        Ok(outcome)
    }

    pub fn recover(
        dek: Zeroizing<[u8; 32]>,
        expected_vault_id: VaultId,
        password: Zeroizing<String>,
        path: PathBuf,
    ) -> Result<Self> {
        let lock_path = path.clone();
        with_vault_lock(&lock_path, || {
            let bytes = fs::read(&path)?;
            if vault_version(&bytes)? != FORMAT_VERSION_V3 {
                return Err(TermKeyError::RecoveryFailed(
                    "Recovery requires a migrated vault".into(),
                ));
            }
            let (mut vault, vault_id) = decode_v3_with_dek(&dek, &bytes, VaultHeader::MAGIC)?;
            if vault_id != expected_vault_id {
                return Err(TermKeyError::RecoveryFailed(
                    "Recovery configuration does not match this vault".into(),
                ));
            }
            vault.revision = vault
                .revision
                .checked_add(1)
                .ok_or(TermKeyError::VaultConflict)?;
            let encoded = encode_v3(
                &vault,
                password.as_bytes(),
                VaultHeader::MAGIC,
                Some(&dek),
                Some(vault_id),
            )?;
            atomic_replace(&path, &encoded)?;
            Ok(Self {
                expected_revision: vault.revision,
                source_digest: digest(&encoded),
                path,
                vault,
                vault_id,
                dek,
                master_password: password,
                write_credentials_fresh: true,
            })
        })
    }

    pub fn save(&mut self) -> Result<()> {
        if !self.write_credentials_fresh {
            return Err(TermKeyError::VaultConflict);
        }
        let next_revision = self
            .expected_revision
            .checked_add(1)
            .ok_or(TermKeyError::VaultConflict)?;
        let mut next_vault = self.vault.clone();
        next_vault.revision = next_revision;

        let encoded = with_vault_lock(&self.path, || {
            self.verify_current_state()?;
            let encoded = encode_v3(
                &next_vault,
                self.master_password.as_bytes(),
                VaultHeader::MAGIC,
                Some(&self.dek),
                Some(self.vault_id),
            )?;
            atomic_replace(&self.path, &encoded)?;
            Ok(encoded)
        })?;

        self.vault.revision = next_revision;
        self.expected_revision = next_revision;
        self.source_digest = digest(&encoded);
        Ok(())
    }

    pub fn change_master_password(&mut self, password: Zeroizing<String>) -> Result<()> {
        if !self.write_credentials_fresh {
            return Err(TermKeyError::VaultConflict);
        }
        let next_revision = self
            .expected_revision
            .checked_add(1)
            .ok_or(TermKeyError::VaultConflict)?;
        let mut next_vault = self.vault.clone();
        next_vault.revision = next_revision;

        let encoded = with_vault_lock(&self.path, || {
            self.verify_current_state()?;
            let encoded = encode_v3(
                &next_vault,
                password.as_bytes(),
                VaultHeader::MAGIC,
                Some(&self.dek),
                Some(self.vault_id),
            )?;
            atomic_replace(&self.path, &encoded)?;
            Ok(encoded)
        })?;

        self.vault.revision = next_revision;
        self.expected_revision = next_revision;
        self.source_digest = digest(&encoded);
        self.master_password = password;
        Ok(())
    }

    pub fn reload(&mut self) -> Result<()> {
        let (vault, source_digest) = with_vault_lock(&self.path, || {
            let bytes = fs::read(&self.path)?;
            let source_digest = digest(&bytes);
            let (vault, vault_id) = decode_v3_with_dek(&self.dek, &bytes, VaultHeader::MAGIC)
                .map_err(|_| TermKeyError::VaultConflict)?;
            if vault_id != self.vault_id
                || vault.revision < self.expected_revision
                || (vault.revision == self.expected_revision && source_digest != self.source_digest)
            {
                return Err(TermKeyError::VaultConflict);
            }
            Ok((vault, source_digest))
        })?;

        if vault.revision > self.expected_revision {
            self.write_credentials_fresh = false;
        }
        self.expected_revision = vault.revision;
        self.source_digest = source_digest;
        self.vault = vault;
        Ok(())
    }

    pub fn vault_id(&self) -> VaultId {
        self.vault_id
    }

    pub fn dek(&self) -> &[u8; 32] {
        &self.dek
    }

    fn verify_current_state(&self) -> Result<()> {
        let bytes = fs::read(&self.path)?;
        let current_digest = digest(&bytes);
        let decoded = decode_v3_with_dek(&self.dek, &bytes, VaultHeader::MAGIC);
        let (current_vault, current_vault_id) = match decoded {
            Ok(current) => current,
            Err(_) if current_digest != self.source_digest => {
                return Err(TermKeyError::VaultConflict);
            }
            Err(error) => return Err(error),
        };

        if current_digest != self.source_digest
            || current_vault_id != self.vault_id
            || current_vault.revision != self.expected_revision
        {
            return Err(TermKeyError::VaultConflict);
        }
        Ok(())
    }
}

fn clear_legacy_recovery_after_migration(
    vault_path: &std::path::Path,
    vault_id: VaultId,
) -> Result<bool> {
    let config_path = vault_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("config.json");
    crate::config::storage::update_config_at(&config_path, |config| {
        let has_active_recovery = config.has_active_recovery_for(vault_id);
        if matches!(
            config.recovery,
            Some(crate::config::model::RecoveryConfig::Legacy(_))
        ) {
            config.recovery = None;
        }
        Ok(has_active_recovery)
    })
}

fn vault_version(bytes: &[u8]) -> Result<u32> {
    if bytes.get(..4) != Some(VaultHeader::MAGIC.as_slice()) {
        return Err(TermKeyError::InvalidVaultFormat);
    }
    let version = bytes
        .get(4..8)
        .ok_or(TermKeyError::InvalidVaultFormat)?
        .try_into()
        .map(u32::from_le_bytes)
        .map_err(|_| TermKeyError::InvalidVaultFormat)?;
    match version {
        VaultHeader::FORMAT_VERSION_V1 | VaultHeader::FORMAT_VERSION_V2 | FORMAT_VERSION_V3 => {
            Ok(version)
        }
        unsupported => Err(TermKeyError::UnsupportedVaultVersion(unsupported)),
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;
    use zeroize::Zeroizing;

    use super::{clear_legacy_recovery_after_migration, VaultSession};
    use crate::config::model::{Config, RecoveryConfig, RecoveryConfigV2};
    use crate::crypto::recovery::{create_recovery_config, generate_recovery_phrase, recover_dek};
    use crate::error::TermKeyError;
    use crate::vault::format::{decode_v3, encode_v3_unchecked_for_test, FORMAT_VERSION_V3};
    use crate::vault::model::VaultHeader;
    use crate::vault::storage::write_vault;

    fn password(value: &str) -> Zeroizing<String> {
        Zeroizing::new(value.to_string())
    }

    fn legacy_vault(path: &std::path::Path, password: &[u8]) {
        let mut vault = crate::vault::model::VaultData::new();
        vault.entries.push(crate::vault::model::Entry {
            name: "Migrated entry".to_string(),
            secret: "migration-secret".to_string(),
            secret_type: crate::vault::model::SecretType::Password,
            network: String::new(),
            public_address: None,
            username: Some("migrated-user".to_string()),
            url: Some("https://migration.example".to_string()),
            site_rules: Vec::new(),
            notes: "legacy notes".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        });
        write_vault(&vault, password, path).unwrap();
    }

    #[test]
    fn legacy_literal_protected_marker_migrates_as_unprotected_secret() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let mut vault = crate::vault::model::VaultData::new();
        let mut entry = crate::vault::model::Entry {
            name: "Literal marker".to_string(),
            secret: "placeholder".to_string(),
            secret_type: crate::vault::model::SecretType::Password,
            network: String::new(),
            public_address: None,
            username: None,
            url: None,
            site_rules: Vec::new(),
            notes: String::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        };
        entry.secret = "[encrypted]".to_string();
        vault.entries.push(entry);
        write_vault(&vault, b"correct-password", &path).unwrap();

        let outcome = VaultSession::open(password("correct-password"), path.clone()).unwrap();

        assert!(outcome.migrated_legacy);
        let migrated = &outcome.session.vault.entries[0];
        assert!(!migrated.has_secondary_password);
        assert_eq!(&*migrated.reveal_secret(None).unwrap(), "[encrypted]");

        let reopened = decode_v3(
            b"correct-password",
            &fs::read(path).unwrap(),
            VaultHeader::MAGIC,
        )
        .unwrap();
        let reopened_entry = &reopened.vault.entries[0];
        assert!(!reopened_entry.has_secondary_password);
        assert_eq!(&*reopened_entry.reveal_secret(None).unwrap(), "[encrypted]");
    }

    #[test]
    fn legacy_v2_open_migrates_to_v3_without_losing_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");

        let outcome = VaultSession::open(password("correct-password"), path.clone()).unwrap();

        assert!(outcome.migrated_legacy);
        assert_eq!(outcome.session.vault.entries.len(), 1);
        assert_eq!(outcome.session.vault.entries[0].name, "Migrated entry");
        assert_eq!(outcome.session.vault.entries[0].secret, "migration-secret");
        assert_eq!(
            u32::from_le_bytes(fs::read(&path).unwrap()[4..8].try_into().unwrap()),
            FORMAT_VERSION_V3
        );

        let reopened = decode_v3(
            b"correct-password",
            &fs::read(path).unwrap(),
            VaultHeader::MAGIC,
        )
        .unwrap();
        assert_eq!(reopened.vault.entries[0].name, "Migrated entry");
    }

    #[test]
    fn failed_legacy_authentication_does_not_modify_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let original = fs::read(&path).unwrap();

        let result = VaultSession::open(password("wrong-password"), path.clone());

        assert!(matches!(result, Err(TermKeyError::DecryptionFailed)));
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn failed_v3_persist_preserves_original_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        session.vault.entries[0].notes = "unsaved change".to_string();
        let original = fs::read(&path).unwrap();
        let original_revision = session.vault.revision;

        for failure_point in [
            crate::vault::storage::AtomicReplaceFailurePoint::Write,
            crate::vault::storage::AtomicReplaceFailurePoint::TempSync,
            crate::vault::storage::AtomicReplaceFailurePoint::Rename,
        ] {
            crate::vault::storage::fail_next_atomic_replace_at(failure_point);
            assert!(session.save().is_err());
            assert_eq!(session.vault.revision, original_revision);
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn parent_sync_failure_after_rename_advances_disk_and_session_together() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        session.vault.entries[0].notes = "committed change".to_string();
        let original_revision = session.vault.revision;

        crate::vault::storage::fail_next_atomic_replace_at(
            crate::vault::storage::AtomicReplaceFailurePoint::ParentSync,
        );
        let result = session.save();

        assert!(result.is_ok());
        assert_eq!(session.vault.revision, original_revision + 1);
        let persisted = decode_v3(
            b"correct-password",
            &fs::read(path).unwrap(),
            VaultHeader::MAGIC,
        )
        .unwrap()
        .vault;
        assert_eq!(persisted.revision, original_revision + 1);
        assert_eq!(persisted.entries[0].notes, "committed change");
    }

    #[test]
    fn stale_session_save_returns_vault_conflict() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut first = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let mut stale = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;

        first.vault.entries[0].notes = "first update".to_string();
        first.save().unwrap();
        stale.vault.entries[0].notes = "stale update".to_string();

        assert!(matches!(stale.save(), Err(TermKeyError::VaultConflict)));
        let persisted = VaultSession::open(password("correct-password"), path)
            .unwrap()
            .session;
        assert_eq!(persisted.vault.entries[0].notes, "first update");
    }

    #[test]
    fn reload_adopts_authenticated_disk_changes_with_stable_identity() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let mut independent = VaultSession::open(password("correct-password"), path)
            .unwrap()
            .session;
        independent.vault.entries[0].secret = "changed-secret".into();
        independent.save().unwrap();

        session.reload().unwrap();

        assert_eq!(session.vault.entries[0].secret, "changed-secret");
        assert_eq!(session.vault.revision, independent.vault.revision);
    }

    #[test]
    fn reload_rejects_replacement_vault_id_transactionally() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let original_vault = serde_json::to_vec(&session.vault).unwrap();
        let replacement = crate::vault::format::encode_v3(
            &session.vault,
            b"correct-password",
            VaultHeader::MAGIC,
            Some(session.dek()),
            None,
        )
        .unwrap();
        fs::write(path, replacement.as_slice()).unwrap();

        assert!(matches!(session.reload(), Err(TermKeyError::VaultConflict)));
        assert_eq!(serde_json::to_vec(&session.vault).unwrap(), original_vault);
    }

    #[test]
    fn reload_rejects_corruption_transactionally() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let original_vault = serde_json::to_vec(&session.vault).unwrap();
        fs::write(path, b"corrupted replacement").unwrap();

        assert!(matches!(session.reload(), Err(TermKeyError::VaultConflict)));
        assert_eq!(serde_json::to_vec(&session.vault).unwrap(), original_vault);
    }

    #[test]
    fn reload_rejects_authenticated_invalid_data_transactionally() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let original_vault = serde_json::to_vec(&session.vault).unwrap();
        let mut invalid = session.vault.clone();
        let mut duplicate = invalid.entries[0].clone();
        duplicate.name = invalid.entries[0].name.to_lowercase();
        invalid.entries.push(duplicate);
        invalid.revision += 1;
        let encoded = encode_v3_unchecked_for_test(
            &invalid,
            b"correct-password",
            VaultHeader::MAGIC,
            Some(session.dek()),
            Some(session.vault_id()),
        );
        fs::write(path, encoded.as_slice()).unwrap();

        assert!(matches!(session.reload(), Err(TermKeyError::VaultConflict)));
        assert_eq!(serde_json::to_vec(&session.vault).unwrap(), original_vault);
    }

    #[test]
    fn reload_rejects_authenticated_rollback_transactionally() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let old_snapshot = fs::read(&path).unwrap();
        let mut independent = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        independent.vault.entries[0].secret = "newer-secret".into();
        independent.save().unwrap();
        session.reload().unwrap();
        let current_vault = serde_json::to_vec(&session.vault).unwrap();
        fs::write(path, old_snapshot).unwrap();

        assert!(matches!(session.reload(), Err(TermKeyError::VaultConflict)));
        assert_eq!(serde_json::to_vec(&session.vault).unwrap(), current_vault);
    }

    #[test]
    fn reload_rejects_equal_revision_authenticated_fork_transactionally() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let original_vault = serde_json::to_vec(&session.vault).unwrap();
        let fork = crate::vault::format::encode_v3(
            &session.vault,
            b"correct-password",
            VaultHeader::MAGIC,
            Some(session.dek()),
            Some(session.vault_id()),
        )
        .unwrap();
        fs::write(path, fork.as_slice()).unwrap();

        assert!(matches!(session.reload(), Err(TermKeyError::VaultConflict)));
        assert_eq!(serde_json::to_vec(&session.vault).unwrap(), original_vault);
    }

    #[test]
    fn reload_accepts_unchanged_authenticated_snapshot() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path)
            .unwrap()
            .session;
        let original_vault = serde_json::to_vec(&session.vault).unwrap();

        session.reload().unwrap();

        assert_eq!(serde_json::to_vec(&session.vault).unwrap(), original_vault);
    }

    #[test]
    fn reload_after_external_password_change_makes_session_read_only() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"old-password");
        let mut session = VaultSession::open(password("old-password"), path.clone())
            .unwrap()
            .session;
        let mut independent = VaultSession::open(password("old-password"), path.clone())
            .unwrap()
            .session;
        independent
            .change_master_password(password("new-password"))
            .unwrap();
        session.reload().unwrap();
        session.vault.entries[0].notes = "must not persist".into();
        let rotated_file = fs::read(&path).unwrap();

        assert!(matches!(session.save(), Err(TermKeyError::VaultConflict)));
        assert!(matches!(
            session.change_master_password(password("another-password")),
            Err(TermKeyError::VaultConflict)
        ));
        assert_eq!(fs::read(&path).unwrap(), rotated_file);
        assert!(matches!(
            VaultSession::open(password("old-password"), path.clone()),
            Err(TermKeyError::DecryptionFailed)
        ));
        let reopened = VaultSession::open(password("new-password"), path)
            .unwrap()
            .session;
        assert_ne!(reopened.vault.entries[0].notes, "must not persist");
    }

    #[test]
    fn same_revision_different_authenticated_file_returns_vault_conflict() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let replacement = crate::vault::format::encode_v3(
            &session.vault,
            b"correct-password",
            VaultHeader::MAGIC,
            Some(session.dek()),
            Some(session.vault_id()),
        )
        .unwrap();
        crate::vault::storage::atomic_replace(&path, &replacement).unwrap();
        let authenticated_replacement = fs::read(&path).unwrap();
        let original_revision = session.vault.revision;

        assert!(matches!(session.save(), Err(TermKeyError::VaultConflict)));
        assert_eq!(session.vault.revision, original_revision);
        assert_eq!(fs::read(path).unwrap(), authenticated_replacement);
    }

    #[test]
    fn successful_save_increments_revision_once() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut session = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let original_revision = session.vault.revision;

        session.save().unwrap();

        assert_eq!(session.vault.revision, original_revision + 1);
        let reopened = VaultSession::open(password("correct-password"), path)
            .unwrap()
            .session;
        assert_eq!(reopened.vault.revision, original_revision + 1);
    }

    #[test]
    fn change_master_password_rewraps_existing_vault() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"old-password");
        let mut session = VaultSession::open(password("old-password"), path.clone())
            .unwrap()
            .session;
        let vault_id = session.vault_id();
        let dek = *session.dek();

        session
            .change_master_password(password("new-password"))
            .unwrap();

        assert!(matches!(
            VaultSession::open(password("old-password"), path.clone()),
            Err(TermKeyError::DecryptionFailed)
        ));
        let reopened = VaultSession::open(password("new-password"), path)
            .unwrap()
            .session;
        assert_eq!(reopened.vault_id(), vault_id);
        assert_eq!(reopened.dek(), &dek);
    }

    #[test]
    fn password_change_preserves_recovery_dek() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"old-password");
        let mut session = VaultSession::open(password("old-password"), path.clone())
            .unwrap()
            .session;
        let phrase = generate_recovery_phrase().unwrap();
        let recovery = create_recovery_config(session.vault_id(), session.dek(), &phrase).unwrap();

        session
            .change_master_password(password("new-password"))
            .unwrap();

        let recovered = recover_dek(&recovery, session.vault_id(), &phrase).unwrap();
        let reopened = VaultSession::open(password("new-password"), path)
            .unwrap()
            .session;
        assert_eq!(&*recovered, reopened.dek());
    }

    #[test]
    fn recover_then_change_password_can_recover_again() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"original-password");
        let original = VaultSession::open(password("original-password"), path.clone())
            .unwrap()
            .session;
        let original_revision = original.vault.revision;
        let phrase = generate_recovery_phrase().unwrap();
        let recovery =
            create_recovery_config(original.vault_id(), original.dek(), &phrase).unwrap();
        let recovered_dek = recover_dek(&recovery, original.vault_id(), &phrase).unwrap();

        let mut recovered = VaultSession::recover(
            recovered_dek,
            original.vault_id(),
            password("recovered-password"),
            path.clone(),
        )
        .unwrap();
        assert_eq!(recovered.vault.revision, original_revision + 1);
        recovered
            .change_master_password(password("rotated-password"))
            .unwrap();
        assert_eq!(recovered.vault.revision, original_revision + 2);

        let recovered_again = recover_dek(&recovery, recovered.vault_id(), &phrase).unwrap();
        let reopened = VaultSession::recover(
            recovered_again,
            recovered.vault_id(),
            password("second-recovery-password"),
            path,
        )
        .unwrap();
        assert_eq!(reopened.vault.revision, original_revision + 3);
        assert_eq!(reopened.vault_id(), original.vault_id());
        assert_eq!(reopened.dek(), original.dek());
    }

    #[test]
    fn recovery_rejects_authenticated_invalid_data_without_rewriting_it() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"original-password");
        let original = VaultSession::open(password("original-password"), path.clone())
            .unwrap()
            .session;
        let mut invalid = original.vault.clone();
        invalid.entries[0].has_secondary_password = true;
        let encoded = encode_v3_unchecked_for_test(
            &invalid,
            b"original-password",
            VaultHeader::MAGIC,
            Some(original.dek()),
            Some(original.vault_id()),
        );
        fs::write(&path, encoded.as_slice()).unwrap();
        let before_recovery = fs::read(&path).unwrap();

        let result = VaultSession::recover(
            Zeroizing::new(*original.dek()),
            original.vault_id(),
            password("replacement-password"),
            path.clone(),
        );

        assert!(matches!(result, Err(TermKeyError::InvalidVaultFormat)));
        assert_eq!(fs::read(path).unwrap(), before_recovery);
    }

    #[test]
    fn compatibility_write_preserves_v3_identity() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let opened = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let vault_id = opened.vault_id();
        let dek = *opened.dek();
        let mut edited = opened.vault.clone();
        edited.entries[0].notes = "compatibility edit".into();

        crate::vault::storage::write_vault(&edited, b"correct-password", &path).unwrap();

        let reopened = VaultSession::open(password("correct-password"), path)
            .unwrap()
            .session;
        assert_eq!(reopened.vault_id(), vault_id);
        assert_eq!(reopened.dek(), &dek);
        assert_eq!(reopened.vault.entries[0].notes, "compatibility edit");
    }

    #[test]
    fn stale_compatibility_write_returns_vault_conflict() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        legacy_vault(&path, b"correct-password");
        let mut current = VaultSession::open(password("correct-password"), path.clone())
            .unwrap()
            .session;
        let mut stale = current.vault.clone();
        stale.entries[0].notes = "stale compatibility edit".into();
        current.vault.entries[0].notes = "committed session edit".into();
        current.save().unwrap();

        let result = crate::vault::storage::write_vault(&stale, b"correct-password", &path);

        assert!(matches!(result, Err(TermKeyError::VaultConflict)));
        let reopened = VaultSession::open(password("correct-password"), path)
            .unwrap()
            .session;
        assert_eq!(reopened.vault.entries[0].notes, "committed session edit");
    }

    #[test]
    fn normal_legacy_unlock_clears_legacy_recovery() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let config_path = dir.path().join("config.json");
        legacy_vault(&path, b"correct-password");
        let mut config: Config = serde_json::from_str(
            r#"{
                "recovery": {
                    "question_index": 1,
                    "answer_hash": [1],
                    "answer_salt": [2],
                    "master_key_blob": [3],
                    "master_key_blob_nonce": [4],
                    "master_key_blob_salt": [5]
                }
            }"#,
        )
        .unwrap();
        config.vault_path = path.display().to_string();
        crate::config::storage::save_config_to(&config, &config_path).unwrap();

        let outcome = VaultSession::open(password("correct-password"), path).unwrap();

        assert!(outcome.migrated_legacy);
        let persisted = crate::config::storage::load_config_from(&config_path).unwrap();
        assert!(!matches!(
            persisted.recovery,
            Some(RecoveryConfig::Legacy(_))
        ));
    }

    #[test]
    fn committed_migration_with_corrupt_config_returns_usable_session_and_notice() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let config_path = dir.path().join("config.json");
        legacy_vault(&path, b"correct-password");
        fs::write(&config_path, b"{not valid json").unwrap();

        let outcome = VaultSession::open(password("correct-password"), path.clone()).unwrap();

        assert!(outcome.migrated_legacy);
        assert!(outcome.recovery_notice.is_some());
        assert_eq!(outcome.session.vault.entries[0].name, "Migrated entry");
        assert_eq!(
            u32::from_le_bytes(fs::read(path).unwrap()[4..8].try_into().unwrap()),
            FORMAT_VERSION_V3
        );
    }

    #[test]
    fn committed_migration_with_unwritable_config_returns_usable_session_and_notice() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let config_path = dir.path().join("config.json");
        legacy_vault(&path, b"correct-password");
        let mut config: Config = serde_json::from_str(
            r#"{
                "recovery": {
                    "question_index": 1,
                    "answer_hash": [1],
                    "answer_salt": [2],
                    "master_key_blob": [3],
                    "master_key_blob_nonce": [4],
                    "master_key_blob_salt": [5]
                }
            }"#,
        )
        .unwrap();
        config.vault_path = path.display().to_string();
        crate::config::storage::save_config_to(&config, &config_path).unwrap();
        crate::config::storage::fail_next_config_replace_for_test();

        let outcome = VaultSession::open(password("correct-password"), path.clone()).unwrap();

        assert!(outcome.migrated_legacy);
        assert!(outcome.recovery_notice.is_some());
        assert_eq!(outcome.session.vault.entries[0].name, "Migrated entry");
        assert_eq!(
            u32::from_le_bytes(fs::read(path).unwrap()[4..8].try_into().unwrap()),
            FORMAT_VERSION_V3
        );
        assert!(matches!(
            crate::config::storage::load_config_from(&config_path)
                .unwrap()
                .recovery,
            Some(RecoveryConfig::Legacy(_))
        ));
    }

    #[test]
    fn existing_v3_unlock_does_not_touch_corrupt_config() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let config_path = dir.path().join("config.json");
        VaultSession::create(
            crate::vault::model::VaultData::new(),
            password("correct-password"),
            path.clone(),
        )
        .unwrap();
        fs::write(config_path, b"{not valid json").unwrap();

        let outcome = VaultSession::open(password("correct-password"), path).unwrap();

        assert!(!outcome.migrated_legacy);
        assert!(outcome.recovery_notice.is_none());
    }

    #[test]
    fn concurrent_recovery_replacement_is_not_erased_by_legacy_cleanup() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault.ck");
        let config_path = dir.path().join("config.json");
        let vault_id = crate::vault::format::VaultId([0x42; 16]);
        let mut legacy: Config = serde_json::from_str(
            r#"{
                "recovery": {
                    "question_index": 1,
                    "answer_hash": [1],
                    "answer_salt": [2],
                    "master_key_blob": [3],
                    "master_key_blob_nonce": [4],
                    "master_key_blob_salt": [5]
                }
            }"#,
        )
        .unwrap();
        legacy.vault_path = vault_path.display().to_string();
        crate::config::storage::save_config_to(&legacy, &config_path).unwrap();

        let replacement = RecoveryConfigV2 {
            version: 2,
            vault_id,
            salt: vec![0x11; 32],
            nonce: vec![0x22; 24],
            wrapped_dek: vec![0x33; 48],
        };
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let cleanup_barrier = std::sync::Arc::clone(&barrier);
        let cleanup_path = vault_path.clone();
        let cleanup = std::thread::spawn(move || {
            cleanup_barrier.wait();
            clear_legacy_recovery_after_migration(&cleanup_path, vault_id).unwrap();
        });
        let replacement_barrier = std::sync::Arc::clone(&barrier);
        let replacement_path = config_path.clone();
        let replace = std::thread::spawn(move || {
            replacement_barrier.wait();
            crate::config::storage::update_config_at(&replacement_path, |config| {
                config.recovery = Some(RecoveryConfig::V2(replacement));
                Ok(())
            })
            .unwrap();
        });

        barrier.wait();
        cleanup.join().unwrap();
        replace.join().unwrap();

        assert!(matches!(
            crate::config::storage::load_config_from(&config_path)
                .unwrap()
                .recovery,
            Some(RecoveryConfig::V2(RecoveryConfigV2 {
                vault_id: persisted_vault_id,
                ..
            })) if persisted_vault_id == vault_id
        ));
    }
}
