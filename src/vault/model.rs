use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroize;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SecretType {
    PrivateKey,
    SeedPhrase,
}

impl fmt::Display for SecretType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretType::PrivateKey => write!(f, "Private Key"),
            SecretType::SeedPhrase => write!(f, "Seed Phrase"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub secret: String,
    pub secret_type: SecretType,
    pub network: String,
    pub notes: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("name", &self.name)
            .field("secret", &"[REDACTED]")
            .field("secret_type", &self.secret_type)
            .field("network", &self.network)
            .field("notes", &self.notes)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryMeta {
    pub name: String,
    pub network: String,
    pub secret_type: SecretType,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultData {
    pub entries: Vec<Entry>,
    pub version: u32,
}

impl VaultData {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            version: 1,
        }
    }

    pub fn find_entry(&self, name: &str) -> Option<&Entry> {
        let name_lower = name.to_lowercase();
        self.entries.iter().find(|e| e.name.to_lowercase() == name_lower)
    }

    pub fn find_entry_mut(&mut self, name: &str) -> Option<&mut Entry> {
        let name_lower = name.to_lowercase();
        self.entries.iter_mut().find(|e| e.name.to_lowercase() == name_lower)
    }

    pub fn remove_entry(&mut self, name: &str) -> Option<Entry> {
        let name_lower = name.to_lowercase();
        if let Some(pos) = self.entries.iter().position(|e| e.name.to_lowercase() == name_lower) {
            Some(self.entries.remove(pos))
        } else {
            None
        }
    }

    pub fn has_entry(&self, name: &str) -> bool {
        self.find_entry(name).is_some()
    }

    pub fn metadata(&self) -> Vec<EntryMeta> {
        self.entries
            .iter()
            .map(|e| EntryMeta {
                name: e.name.clone(),
                network: e.network.clone(),
                secret_type: e.secret_type.clone(),
                notes: e.notes.clone(),
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
