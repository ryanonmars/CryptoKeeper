use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::entry_key;
use crate::error::{Result, TermKeyError};

const PROTECTED_SECRET_MARKER: &str = "[encrypted]";
const WRAPPED_ENTRY_KEY_LEN: usize = 48;
const ENTRY_KEY_NONCE_LEN: usize = 24;
const ENTRY_KEY_SALT_LEN: usize = 32;
const ENCRYPTED_SECRET_NONCE_LEN: usize = 24;
const AEAD_TAG_LEN: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SecretType {
    PrivateKey,
    SeedPhrase,
    Password,
    Other(String),
}

impl SecretType {
    pub fn is_crypto_type(&self) -> bool {
        matches!(self, Self::PrivateKey | Self::SeedPhrase)
    }

    pub fn is_password_type(&self) -> bool {
        matches!(self, Self::Password)
    }

    pub fn is_other_type(&self) -> bool {
        matches!(self, Self::Other(_))
    }
}

impl fmt::Display for SecretType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretType::PrivateKey => write!(f, "Private Key"),
            SecretType::SeedPhrase => write!(f, "Seed Phrase"),
            SecretType::Password => write!(f, "Password"),
            SecretType::Other(label) => {
                if label.trim().is_empty() {
                    write!(f, "Other")
                } else {
                    write!(f, "{}", label.trim())
                }
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub secret: String,
    pub secret_type: SecretType,
    pub network: String,
    #[serde(default)]
    pub public_address: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub site_rules: Vec<String>,
    pub notes: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Secondary password fields (all serde(default) for backward compat)
    #[serde(default)]
    pub has_secondary_password: bool,
    #[serde(default)]
    pub entry_key_wrapped: Option<Vec<u8>>,
    #[serde(default)]
    pub entry_key_nonce: Option<Vec<u8>>,
    #[serde(default)]
    pub entry_key_salt: Option<Vec<u8>>,
    #[serde(default)]
    pub encrypted_secret: Option<Vec<u8>>,
    #[serde(default)]
    pub encrypted_secret_nonce: Option<Vec<u8>>,
}

impl Entry {
    pub fn validate(&self) -> Result<()> {
        if self.has_secondary_password {
            let protected_fields_valid = matches!(
                (
                    self.entry_key_wrapped.as_deref(),
                    self.entry_key_nonce.as_deref(),
                    self.entry_key_salt.as_deref(),
                    self.encrypted_secret.as_deref(),
                    self.encrypted_secret_nonce.as_deref(),
                ),
                (
                    Some(wrapped_key),
                    Some(key_nonce),
                    Some(key_salt),
                    Some(encrypted_secret),
                    Some(secret_nonce),
                ) if wrapped_key.len() == WRAPPED_ENTRY_KEY_LEN
                    && key_nonce.len() == ENTRY_KEY_NONCE_LEN
                    && key_salt.len() == ENTRY_KEY_SALT_LEN
                    && encrypted_secret.len() >= AEAD_TAG_LEN
                    && secret_nonce.len() == ENCRYPTED_SECRET_NONCE_LEN
            );

            if self.secret != PROTECTED_SECRET_MARKER || !protected_fields_valid {
                return Err(TermKeyError::InvalidEntry(
                    "protected secret fields are incomplete or malformed".to_string(),
                ));
            }
        } else if self.entry_key_wrapped.is_some()
            || self.entry_key_nonce.is_some()
            || self.entry_key_salt.is_some()
            || self.encrypted_secret.is_some()
            || self.encrypted_secret_nonce.is_some()
            || self.secret.is_empty()
        {
            return Err(TermKeyError::InvalidEntry(
                "unprotected secret fields are inconsistent".to_string(),
            ));
        }

        Ok(())
    }

    pub fn reveal_secret(&self, secondary_password: Option<&str>) -> Result<Zeroizing<String>> {
        self.validate()?;

        if !self.has_secondary_password {
            return Ok(Zeroizing::new(self.secret.clone()));
        }

        let password = secondary_password
            .filter(|password| !password.is_empty())
            .ok_or(TermKeyError::SecondaryPasswordRequired)?;
        let (Some(wrapped_key), Some(key_nonce), Some(key_salt)) = (
            self.entry_key_wrapped.as_deref(),
            self.entry_key_nonce.as_deref(),
            self.entry_key_salt.as_deref(),
        ) else {
            return Err(TermKeyError::InvalidEntry(
                "protected key fields are incomplete".to_string(),
            ));
        };
        let (Some(encrypted_secret), Some(secret_nonce)) = (
            self.encrypted_secret.as_deref(),
            self.encrypted_secret_nonce.as_deref(),
        ) else {
            return Err(TermKeyError::InvalidEntry(
                "protected secret fields are incomplete".to_string(),
            ));
        };

        let entry_key = entry_key::unwrap_entry_key(wrapped_key, key_nonce, key_salt, password)?;
        entry_key::decrypt_secret(&entry_key, encrypted_secret, secret_nonce)
    }

    pub fn replace_secret(&mut self, secret: &str, secondary_password: Option<&str>) -> Result<()> {
        self.validate()?;
        if secret.is_empty() || (self.has_secondary_password && secret == PROTECTED_SECRET_MARKER) {
            return Err(TermKeyError::InvalidEntry(
                "secret cannot be empty or use the protected marker".to_string(),
            ));
        }

        if self.has_secondary_password {
            let password = secondary_password
                .filter(|password| !password.is_empty())
                .ok_or(TermKeyError::SecondaryPasswordRequired)?;
            let (Some(wrapped_key), Some(key_nonce), Some(key_salt)) = (
                self.entry_key_wrapped.as_deref(),
                self.entry_key_nonce.as_deref(),
                self.entry_key_salt.as_deref(),
            ) else {
                return Err(TermKeyError::InvalidEntry(
                    "protected key fields are incomplete".to_string(),
                ));
            };

            let entry_key =
                entry_key::unwrap_entry_key(wrapped_key, key_nonce, key_salt, password)?;
            let (encrypted_secret, encrypted_secret_nonce) =
                entry_key::encrypt_secret(&entry_key, secret)?;

            self.secret.clear();
            self.secret.push_str(PROTECTED_SECRET_MARKER);
            self.encrypted_secret = Some(encrypted_secret);
            self.encrypted_secret_nonce = Some(encrypted_secret_nonce);
        } else {
            self.secret.zeroize();
            self.secret.push_str(secret);
        }

        self.updated_at = Utc::now();
        Ok(())
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.secret.zeroize();
        if let Some(ref mut wrapped) = self.entry_key_wrapped {
            wrapped.zeroize();
        }
        if let Some(ref mut secret) = self.encrypted_secret {
            secret.zeroize();
        }
    }
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("name", &self.name)
            .field("secret", &"[REDACTED]")
            .field("secret_type", &self.secret_type)
            .field("network", &self.network)
            .field("public_address", &self.public_address)
            .field("username", &self.username)
            .field("url", &self.url)
            .field("notes", &self.notes)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("has_secondary_password", &self.has_secondary_password)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryMeta {
    pub name: String,
    pub network: String,
    pub secret_type: SecretType,
    #[serde(default)]
    pub public_address: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub site_rules: Vec<String>,
    pub notes: String,
    #[serde(default)]
    pub has_secondary_password: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultData {
    pub entries: Vec<Entry>,
    pub version: u32,
    #[serde(default)]
    pub revision: u64,
}

impl Default for VaultData {
    fn default() -> Self {
        Self::new()
    }
}

impl VaultData {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            version: 1,
            revision: 0,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let mut names = HashSet::with_capacity(self.entries.len());

        for entry in &self.entries {
            entry.validate()?;
            if entry.name.is_empty() || entry.name.trim() != entry.name {
                return Err(TermKeyError::InvalidEntry(
                    "entry name must be nonempty and trimmed".to_string(),
                ));
            }

            let normalized_name = entry.name.to_lowercase();
            if !names.insert(normalized_name) {
                return Err(TermKeyError::EntryAlreadyExists(entry.name.clone()));
            }
        }

        Ok(())
    }

    pub(crate) fn normalize_legacy_entry_names(&mut self) -> Result<()> {
        for entry in &self.entries {
            entry.validate()?;
        }

        let mut names = HashSet::with_capacity(self.entries.len());
        for entry in &mut self.entries {
            let base_name = match entry.name.trim() {
                "" => "Untitled".to_string(),
                trimmed => trimmed.to_string(),
            };
            let mut candidate = base_name.clone();
            let mut suffix = 2usize;

            while !names.insert(candidate.to_lowercase()) {
                candidate = format!("{base_name} ({suffix})");
                suffix = suffix.checked_add(1).ok_or_else(|| {
                    TermKeyError::InvalidEntry("too many legacy entry name collisions".to_string())
                })?;
            }
            entry.name = candidate;
        }

        self.validate()
    }

    pub fn push_entry(&mut self, mut entry: Entry) -> Result<()> {
        entry.name = entry.name.trim().to_string();
        if entry.name.is_empty() {
            return Err(TermKeyError::InvalidEntry(
                "entry name cannot be empty".to_string(),
            ));
        }
        entry.validate()?;
        let normalized_name = entry.name.to_lowercase();
        if self
            .entries
            .iter()
            .any(|existing| existing.name.to_lowercase() == normalized_name)
        {
            return Err(TermKeyError::EntryAlreadyExists(entry.name.clone()));
        }

        self.entries.push(entry);
        Ok(())
    }

    fn replace_entry_internal(&mut self, original_name: &str, mut entry: Entry) -> Result<()> {
        let original_name = original_name.trim();
        let position = self
            .find_entry_index_by_name(original_name)
            .ok_or_else(|| TermKeyError::EntryNotFound(original_name.to_string()))?;

        entry.name = entry.name.trim().to_string();
        if entry.name.is_empty() {
            return Err(TermKeyError::InvalidEntry(
                "entry name cannot be empty".to_string(),
            ));
        }
        entry.validate()?;
        let normalized_name = entry.name.to_lowercase();
        if self.entries.iter().enumerate().any(|(index, existing)| {
            index != position && existing.name.to_lowercase() == normalized_name
        }) {
            return Err(TermKeyError::EntryAlreadyExists(entry.name.clone()));
        }

        self.entries[position] = entry;
        Ok(())
    }

    pub fn authorize_entry_mutation(
        &self,
        name: &str,
        secondary_password: Option<&str>,
    ) -> Result<()> {
        let entry = self
            .find_entry(name)
            .ok_or_else(|| TermKeyError::EntryNotFound(name.to_string()))?;
        if entry.has_secondary_password {
            let _revealed = entry.reveal_secret(secondary_password)?;
        } else {
            entry.validate()?;
        }
        Ok(())
    }

    pub fn remove_entry_authorized(
        &mut self,
        name: &str,
        secondary_password: Option<&str>,
    ) -> Result<Entry> {
        self.authorize_entry_mutation(name, secondary_password)?;
        let position = self
            .find_entry_index_by_name(name)
            .ok_or_else(|| TermKeyError::EntryNotFound(name.to_string()))?;
        Ok(self.entries.remove(position))
    }

    pub fn rename_entry_authorized(
        &mut self,
        old_name: &str,
        new_name: &str,
        secondary_password: Option<&str>,
    ) -> Result<()> {
        self.authorize_entry_mutation(old_name, secondary_password)?;

        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(TermKeyError::InvalidEntry(
                "entry name cannot be empty".to_string(),
            ));
        }
        let position = self
            .find_entry_index_by_name(old_name)
            .ok_or_else(|| TermKeyError::EntryNotFound(old_name.to_string()))?;
        let normalized_name = new_name.to_lowercase();
        if self
            .entries
            .iter()
            .enumerate()
            .any(|(index, entry)| index != position && entry.name.to_lowercase() == normalized_name)
        {
            return Err(TermKeyError::EntryAlreadyExists(new_name.to_string()));
        }

        self.entries[position].name.clear();
        self.entries[position].name.push_str(new_name);
        self.entries[position].updated_at = Utc::now();
        Ok(())
    }

    pub fn replace_entry_authorized(
        &mut self,
        original_name: &str,
        replacement: Entry,
        secondary_password: Option<&str>,
    ) -> Result<()> {
        self.authorize_entry_mutation(original_name, secondary_password)?;
        self.replace_entry_internal(original_name, replacement)
    }

    pub fn find_entry(&self, name: &str) -> Option<&Entry> {
        self.find_entry_index_by_name(name)
            .map(|index| &self.entries[index])
    }

    pub fn has_entry(&self, name: &str) -> bool {
        self.find_entry(name).is_some()
    }

    /// Resolve an identifier to a 0-based index: try 1-based numeric index first, then name match.
    fn resolve_index(&self, id: &str) -> Option<usize> {
        if let Ok(n) = id.parse::<usize>() {
            if n >= 1 && n <= self.entries.len() {
                return Some(n - 1);
            }
        }
        self.find_entry_index_by_name(id)
    }

    fn find_entry_index_by_name(&self, name: &str) -> Option<usize> {
        let normalized_name = name.trim().to_lowercase();
        self.entries
            .iter()
            .position(|entry| entry.name.to_lowercase() == normalized_name)
    }

    pub fn find_entry_by_id(&self, id: &str) -> Option<&Entry> {
        self.resolve_index(id).map(|i| &self.entries[i])
    }

    #[cfg(test)]
    pub fn find_entry_mut_by_id(&mut self, id: &str) -> Option<&mut Entry> {
        self.resolve_index(id).map(move |i| &mut self.entries[i])
    }

    /// Resolve an identifier to the entry's name (for display in prompts).
    pub fn resolve_entry_name(&self, id: &str) -> Option<String> {
        self.resolve_index(id).map(|i| self.entries[i].name.clone())
    }

    pub fn metadata(&self) -> Vec<EntryMeta> {
        self.entries
            .iter()
            .map(|e| EntryMeta {
                name: e.name.clone(),
                network: e.network.clone(),
                secret_type: e.secret_type.clone(),
                public_address: e.public_address.clone(),
                username: e.username.clone(),
                url: e.url.clone(),
                site_rules: e.site_rules.clone(),
                notes: e.notes.clone(),
                has_secondary_password: e.has_secondary_password,
            })
            .collect()
    }
}

pub struct VaultHeader;

impl VaultHeader {
    pub const MAGIC: &'static [u8; 4] = b"CKPR";
    pub const FORMAT_VERSION_V1: u32 = 1;
    pub const FORMAT_VERSION_V2: u32 = 2;
    /// V1: 4 (magic) + 4 (version) + 32 (salt) + 4 (m_cost) + 4 (t_cost) + 4 (p_cost) + 24 (nonce) + 4 (ct_len) = 80
    pub const HEADER_SIZE_V1: usize = 80;
}

pub struct BackupHeader;

impl BackupHeader {
    pub const MAGIC: &'static [u8; 4] = b"CKBK";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::entry_key;
    use crate::error::TermKeyError;
    use chrono::Utc;

    fn make_entry(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            secret: "secret".to_string(),
            secret_type: SecretType::PrivateKey,
            network: "Ethereum".to_string(),
            public_address: None,
            username: None,
            url: None,
            site_rules: Vec::new(),
            notes: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        }
    }

    fn make_vault(names: &[&str]) -> VaultData {
        let mut vault = VaultData::new();
        for name in names {
            vault.entries.push(make_entry(name));
        }
        vault
    }

    fn make_protected_entry(name: &str, secret: &str, password: &str) -> Entry {
        let entry_key_bytes = entry_key::generate_entry_key();
        let (encrypted_secret, encrypted_secret_nonce) =
            entry_key::encrypt_secret(&entry_key_bytes, secret).unwrap();
        let (entry_key_wrapped, entry_key_nonce, entry_key_salt) =
            entry_key::wrap_entry_key(&entry_key_bytes, password).unwrap();

        let mut entry = make_entry(name);
        entry.secret = "[encrypted]".to_string();
        entry.has_secondary_password = true;
        entry.entry_key_wrapped = Some(entry_key_wrapped);
        entry.entry_key_nonce = Some(entry_key_nonce);
        entry.entry_key_salt = Some(entry_key_salt);
        entry.encrypted_secret = Some(encrypted_secret);
        entry.encrypted_secret_nonce = Some(encrypted_secret_nonce);
        entry
    }

    #[test]
    fn protected_entry_reveal_requires_password() {
        let entry = make_protected_entry("Protected", "real secret", "view-password");

        assert!(matches!(
            entry.reveal_secret(None),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert!(matches!(
            entry.reveal_secret(Some("")),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
    }

    #[test]
    fn protected_entry_reveal_returns_decrypted_secret() {
        let entry = make_protected_entry("Protected", "real secret", "view-password");

        let revealed = entry.reveal_secret(Some("view-password")).unwrap();

        assert_eq!(&*revealed, "real secret");
        assert_ne!(&*revealed, "[encrypted]");
        assert!(matches!(
            entry.reveal_secret(Some("wrong-password")),
            Err(TermKeyError::SecondaryPasswordWrong)
        ));
    }

    #[test]
    fn protected_entry_replace_reencrypts_secret() {
        let mut entry = make_protected_entry("Protected", "old secret", "view-password");
        let original_wrapped_key = entry.entry_key_wrapped.clone();
        let original_key_nonce = entry.entry_key_nonce.clone();
        let original_key_salt = entry.entry_key_salt.clone();
        let original_ciphertext = entry.encrypted_secret.clone();
        let original_secret_nonce = entry.encrypted_secret_nonce.clone();

        assert!(matches!(
            entry.replace_secret("new secret", None),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert!(matches!(
            entry.replace_secret("new secret", Some("")),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert!(matches!(
            entry.replace_secret("new secret", Some("wrong-password")),
            Err(TermKeyError::SecondaryPasswordWrong)
        ));
        assert!(entry
            .replace_secret("[encrypted]", Some("view-password"))
            .is_err());
        assert_eq!(entry.secret, "[encrypted]");
        assert_eq!(entry.entry_key_wrapped, original_wrapped_key);
        assert_eq!(entry.entry_key_nonce, original_key_nonce);
        assert_eq!(entry.entry_key_salt, original_key_salt);
        assert_eq!(entry.encrypted_secret, original_ciphertext);
        assert_eq!(entry.encrypted_secret_nonce, original_secret_nonce);

        entry
            .replace_secret("new secret", Some("view-password"))
            .unwrap();

        assert_eq!(entry.secret, "[encrypted]");
        assert_eq!(entry.entry_key_wrapped, original_wrapped_key);
        assert_eq!(entry.entry_key_nonce, original_key_nonce);
        assert_eq!(entry.entry_key_salt, original_key_salt);
        assert_ne!(entry.encrypted_secret, original_ciphertext);
        assert_eq!(
            &*entry.reveal_secret(Some("view-password")).unwrap(),
            "new secret"
        );
    }

    #[test]
    fn protected_entry_remove_requires_correct_password_transactionally() {
        let mut vault = VaultData {
            entries: vec![make_protected_entry(
                "Protected",
                "real secret",
                "view-password",
            )],
            version: 1,
            revision: 0,
        };
        let original = serde_json::to_vec(&vault).unwrap();

        assert!(matches!(
            vault.remove_entry_authorized("Protected", None),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);
        assert!(matches!(
            vault.remove_entry_authorized("Protected", Some("wrong-password")),
            Err(TermKeyError::SecondaryPasswordWrong)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);

        let removed = vault
            .remove_entry_authorized("Protected", Some("view-password"))
            .unwrap();
        assert_eq!(removed.name, "Protected");
        assert!(vault.entries.is_empty());
    }

    #[test]
    fn protected_entry_rename_requires_correct_password_transactionally() {
        let mut vault = VaultData {
            entries: vec![make_protected_entry(
                "Protected",
                "real secret",
                "view-password",
            )],
            version: 1,
            revision: 0,
        };
        let original = serde_json::to_vec(&vault).unwrap();

        assert!(matches!(
            vault.rename_entry_authorized("Protected", "Renamed", None),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);
        assert!(matches!(
            vault.rename_entry_authorized("Protected", "Renamed", Some("wrong-password")),
            Err(TermKeyError::SecondaryPasswordWrong)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);

        vault
            .rename_entry_authorized("Protected", " Renamed ", Some("view-password"))
            .unwrap();
        assert_eq!(vault.entries[0].name, "Renamed");
    }

    #[test]
    fn protected_entry_replace_requires_correct_password_transactionally() {
        let original_entry = make_protected_entry("Protected", "real secret", "view-password");
        let mut replacement = make_entry("Protected");
        replacement.secret = "replacement secret".to_string();
        let mut vault = VaultData {
            entries: vec![original_entry],
            version: 1,
            revision: 0,
        };
        let original = serde_json::to_vec(&vault).unwrap();

        assert!(matches!(
            vault.replace_entry_authorized("Protected", replacement.clone(), None),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);
        assert!(matches!(
            vault.replace_entry_authorized(
                "Protected",
                replacement.clone(),
                Some("wrong-password")
            ),
            Err(TermKeyError::SecondaryPasswordWrong)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);

        vault
            .replace_entry_authorized("Protected", replacement, Some("view-password"))
            .unwrap();
        assert_eq!(vault.entries[0].secret, "replacement secret");
        assert!(!vault.entries[0].has_secondary_password);
    }

    #[test]
    fn protected_unicode_name_matching_is_consistent_across_mutations() {
        let protected = make_protected_entry("Éntry", "real secret", "view-password");
        let mut vault = VaultData {
            entries: vec![protected, make_entry("Café")],
            version: 1,
            revision: 0,
        };

        assert_eq!(vault.find_entry("éntry").unwrap().name, "Éntry");
        assert!(matches!(
            vault.push_entry(make_entry("éNTRY")),
            Err(TermKeyError::EntryAlreadyExists(_))
        ));

        let before_collision = serde_json::to_vec(&vault).unwrap();
        assert!(matches!(
            vault.rename_entry_authorized("éntry", "CAFÉ", Some("view-password")),
            Err(TermKeyError::EntryAlreadyExists(_))
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), before_collision);

        vault
            .rename_entry_authorized("éNTRY", "RÉNAMED", Some("view-password"))
            .unwrap();
        assert!(vault.find_entry("rénamed").is_some());

        let before_replace_collision = serde_json::to_vec(&vault).unwrap();
        assert!(matches!(
            vault.replace_entry_authorized("rénamed", make_entry("cAFÉ"), Some("view-password")),
            Err(TermKeyError::EntryAlreadyExists(_))
        ));
        assert_eq!(
            serde_json::to_vec(&vault).unwrap(),
            before_replace_collision
        );

        vault
            .replace_entry_authorized(
                "rÉNAMED",
                make_protected_entry("Éntry", "replacement secret", "new-password"),
                Some("view-password"),
            )
            .unwrap();
        assert_eq!(
            &*vault
                .find_entry("éntry")
                .unwrap()
                .reveal_secret(Some("new-password"))
                .unwrap(),
            "replacement secret"
        );

        let removed = vault
            .remove_entry_authorized("éNTRY", Some("new-password"))
            .unwrap();
        assert_eq!(removed.name, "Éntry");
        assert!(vault.find_entry("éntry").is_none());
        assert_eq!(vault.entries.len(), 1);
        assert_eq!(vault.entries[0].name, "Café");
    }

    #[test]
    fn unprotected_entry_mutations_require_no_secondary_password() {
        let mut vault = make_vault(&["Alpha", "Bravo"]);

        vault
            .rename_entry_authorized("Alpha", "Renamed", None)
            .unwrap();
        vault
            .replace_entry_authorized("Renamed", make_entry("Replacement"), None)
            .unwrap();
        let removed = vault.remove_entry_authorized("Replacement", None).unwrap();

        assert_eq!(removed.name, "Replacement");
        assert_eq!(vault.entries.len(), 1);
        assert_eq!(vault.entries[0].name, "Bravo");
    }

    #[test]
    fn invalid_protected_field_combination_is_rejected() {
        let valid = make_protected_entry("Protected", "real secret", "view-password");
        valid.validate().unwrap();

        for missing_field in 0..5 {
            let mut entry = valid.clone();
            match missing_field {
                0 => entry.entry_key_wrapped = None,
                1 => entry.entry_key_nonce = None,
                2 => entry.entry_key_salt = None,
                3 => entry.encrypted_secret = None,
                4 => entry.encrypted_secret_nonce = None,
                _ => unreachable!(),
            }
            assert!(entry.validate().is_err());
        }

        let mut wrong_marker = valid.clone();
        wrong_marker.secret = "plaintext".to_string();
        assert!(wrong_marker.validate().is_err());

        let mut wrong_flag = valid;
        wrong_flag.has_secondary_password = false;
        assert!(wrong_flag.validate().is_err());

        let mut unprotected_marker = make_entry("Marker");
        unprotected_marker.secret = "[encrypted]".to_string();
        unprotected_marker.validate().unwrap();
        assert_eq!(
            &*unprotected_marker.reveal_secret(None).unwrap(),
            "[encrypted]"
        );

        let mut empty_unprotected = make_entry("Empty");
        empty_unprotected.secret.clear();
        assert!(empty_unprotected.validate().is_err());

        for protected_field in 0..5 {
            let mut entry = make_entry("Unprotected");
            match protected_field {
                0 => entry.entry_key_wrapped = Some(vec![1]),
                1 => entry.entry_key_nonce = Some(vec![1]),
                2 => entry.entry_key_salt = Some(vec![1]),
                3 => entry.encrypted_secret = Some(vec![1]),
                4 => entry.encrypted_secret_nonce = Some(vec![1]),
                _ => unreachable!(),
            }
            assert!(entry.validate().is_err());
        }
    }

    #[test]
    fn invalid_wrapped_entry_key_lengths_are_rejected() {
        for invalid_length in [47, 49] {
            let mut entry = make_protected_entry("Protected", "real secret", "view-password");
            entry
                .entry_key_wrapped
                .as_mut()
                .unwrap()
                .resize(invalid_length, 0);

            assert!(matches!(
                entry.validate(),
                Err(TermKeyError::InvalidEntry(_))
            ));
        }
    }

    #[test]
    fn invalid_entry_key_nonce_lengths_are_rejected() {
        for invalid_length in [23, 25] {
            let mut entry = make_protected_entry("Protected", "real secret", "view-password");
            entry
                .entry_key_nonce
                .as_mut()
                .unwrap()
                .resize(invalid_length, 0);

            assert!(matches!(
                entry.validate(),
                Err(TermKeyError::InvalidEntry(_))
            ));
        }
    }

    #[test]
    fn invalid_entry_key_salt_lengths_are_rejected() {
        for invalid_length in [31, 33] {
            let mut entry = make_protected_entry("Protected", "real secret", "view-password");
            entry
                .entry_key_salt
                .as_mut()
                .unwrap()
                .resize(invalid_length, 0);

            assert!(matches!(
                entry.validate(),
                Err(TermKeyError::InvalidEntry(_))
            ));
        }
    }

    #[test]
    fn invalid_encrypted_secret_nonce_lengths_are_rejected() {
        for invalid_length in [23, 25] {
            let mut entry = make_protected_entry("Protected", "real secret", "view-password");
            entry
                .encrypted_secret_nonce
                .as_mut()
                .unwrap()
                .resize(invalid_length, 0);

            assert!(matches!(
                entry.validate(),
                Err(TermKeyError::InvalidEntry(_))
            ));
        }
    }

    #[test]
    fn encrypted_secret_shorter_than_aead_tag_is_rejected() {
        let mut entry = make_protected_entry("Protected", "real secret", "view-password");
        entry.encrypted_secret.as_mut().unwrap().resize(15, 0);

        assert!(matches!(
            entry.validate(),
            Err(TermKeyError::InvalidEntry(_))
        ));
    }

    #[test]
    fn empty_and_duplicate_names_are_rejected() {
        let mut vault = VaultData::new();
        vault.push_entry(make_entry("  Alice  ")).unwrap();
        vault.push_entry(make_entry("Bob")).unwrap();

        assert_eq!(vault.entries[0].name, "Alice");
        assert!(vault.push_entry(make_entry(" alice ")).is_err());
        assert!(vault.push_entry(make_entry("   ")).is_err());
        assert_eq!(vault.entries.len(), 2);

        let mut renamed = make_entry(" ALICE ");
        renamed.secret = "replacement".to_string();
        vault
            .replace_entry_authorized("alice", renamed, None)
            .unwrap();
        assert_eq!(vault.entries[0].name, "ALICE");

        let original_bob_secret = vault.entries[1].secret.clone();
        assert!(vault
            .replace_entry_authorized("Bob", make_entry("aLiCe"), None)
            .is_err());
        assert_eq!(vault.entries[1].name, "Bob");
        assert_eq!(vault.entries[1].secret, original_bob_secret);

        vault.validate().unwrap();

        let duplicate_vault = make_vault(&["same", "SAME"]);
        assert!(duplicate_vault.validate().is_err());

        let empty_name_vault = make_vault(&[""]);
        assert!(empty_name_vault.validate().is_err());
    }

    #[test]
    fn legacy_entry_without_protected_fields_deserializes() {
        let mut entry: Entry = serde_json::from_value(serde_json::json!({
            "name": "Legacy",
            "secret": "legacy secret",
            "secret_type": "PrivateKey",
            "network": "Ethereum",
            "notes": "",
            "created_at": "2026-07-24T00:00:00Z",
            "updated_at": "2026-07-24T00:00:00Z"
        }))
        .unwrap();

        entry.validate().unwrap();
        assert!(!entry.has_secondary_password);
        assert_eq!(&*entry.reveal_secret(None).unwrap(), "legacy secret");

        entry.replace_secret("updated legacy secret", None).unwrap();
        assert_eq!(
            &*entry.reveal_secret(None).unwrap(),
            "updated legacy secret"
        );
    }

    #[test]
    fn resolve_by_valid_index() {
        let vault = make_vault(&["Alice", "Bob", "Carol"]);
        assert_eq!(vault.find_entry_by_id("1").unwrap().name, "Alice");
        assert_eq!(vault.find_entry_by_id("2").unwrap().name, "Bob");
        assert_eq!(vault.find_entry_by_id("3").unwrap().name, "Carol");
    }

    #[test]
    fn resolve_by_boundary_indexes() {
        let vault = make_vault(&["Only"]);
        assert!(vault.find_entry_by_id("0").is_none());
        assert_eq!(vault.find_entry_by_id("1").unwrap().name, "Only");
        assert!(vault.find_entry_by_id("2").is_none());
    }

    #[test]
    fn resolve_out_of_range() {
        let vault = make_vault(&["A", "B"]);
        assert!(vault.find_entry_by_id("0").is_none());
        assert!(vault.find_entry_by_id("3").is_none());
        assert!(vault.find_entry_by_id("999").is_none());
    }

    #[test]
    fn resolve_by_name() {
        let vault = make_vault(&["MyWallet", "TestKey"]);
        assert_eq!(vault.find_entry_by_id("MyWallet").unwrap().name, "MyWallet");
        assert_eq!(vault.find_entry_by_id("mywallet").unwrap().name, "MyWallet");
        assert_eq!(vault.find_entry_by_id("TESTKEY").unwrap().name, "TestKey");
    }

    #[test]
    fn resolve_empty_vault() {
        let vault = make_vault(&[]);
        assert!(vault.find_entry_by_id("1").is_none());
        assert!(vault.find_entry_by_id("anything").is_none());
    }

    #[test]
    fn resolve_entry_name_by_index() {
        let vault = make_vault(&["Alpha", "Beta"]);
        assert_eq!(vault.resolve_entry_name("1").unwrap(), "Alpha");
        assert_eq!(vault.resolve_entry_name("2").unwrap(), "Beta");
        assert_eq!(vault.resolve_entry_name("Alpha").unwrap(), "Alpha");
        assert!(vault.resolve_entry_name("3").is_none());
    }

    #[test]
    fn remove_entry_by_id_index() {
        let mut vault = make_vault(&["A", "B", "C"]);
        let resolved = vault.resolve_entry_name("2").unwrap();
        let removed = vault.remove_entry_authorized(&resolved, None).unwrap();
        assert_eq!(removed.name, "B");
        assert_eq!(vault.entries.len(), 2);
    }

    #[test]
    fn remove_entry_by_id_name() {
        let mut vault = make_vault(&["A", "B", "C"]);
        let resolved = vault.resolve_entry_name("C").unwrap();
        let removed = vault.remove_entry_authorized(&resolved, None).unwrap();
        assert_eq!(removed.name, "C");
        assert_eq!(vault.entries.len(), 2);
    }

    #[test]
    fn find_entry_mut_by_id_modifies() {
        let mut vault = make_vault(&["Old"]);
        let entry = vault.find_entry_mut_by_id("1").unwrap();
        entry.name = "New".to_string();
        assert_eq!(vault.entries[0].name, "New");
    }

    #[test]
    fn numeric_name_index_wins() {
        // Entry named "2" at position 0 (index 1). Looking up "2" should get index 2 (position 1).
        let vault = make_vault(&["2", "other"]);
        // "2" as index resolves to position 1 (0-based), which is "other"
        assert_eq!(vault.find_entry_by_id("2").unwrap().name, "other");
        // To access the entry named "2", the user could use index "1"
        assert_eq!(vault.find_entry_by_id("1").unwrap().name, "2");
    }
}
