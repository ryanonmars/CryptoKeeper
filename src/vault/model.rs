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
    #[serde(default)]
    pub public_address: Option<String>,
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
            .field("public_address", &self.public_address)
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
    #[serde(default)]
    pub public_address: Option<String>,
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

    /// Resolve an identifier to a 0-based index: try 1-based numeric index first, then name match.
    fn resolve_index(&self, id: &str) -> Option<usize> {
        if let Ok(n) = id.parse::<usize>() {
            if n >= 1 && n <= self.entries.len() {
                return Some(n - 1);
            }
        }
        let id_lower = id.to_lowercase();
        self.entries.iter().position(|e| e.name.to_lowercase() == id_lower)
    }

    pub fn find_entry_by_id(&self, id: &str) -> Option<&Entry> {
        self.resolve_index(id).map(|i| &self.entries[i])
    }

    pub fn find_entry_mut_by_id(&mut self, id: &str) -> Option<&mut Entry> {
        self.resolve_index(id).map(move |i| &mut self.entries[i])
    }

    pub fn remove_entry_by_id(&mut self, id: &str) -> Option<Entry> {
        self.resolve_index(id).map(|i| self.entries.remove(i))
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_entry(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            secret: "secret".to_string(),
            secret_type: SecretType::PrivateKey,
            network: "Ethereum".to_string(),
            public_address: None,
            notes: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_vault(names: &[&str]) -> VaultData {
        let mut vault = VaultData::new();
        for name in names {
            vault.entries.push(make_entry(name));
        }
        vault
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
        let removed = vault.remove_entry_by_id("2").unwrap();
        assert_eq!(removed.name, "B");
        assert_eq!(vault.entries.len(), 2);
    }

    #[test]
    fn remove_entry_by_id_name() {
        let mut vault = make_vault(&["A", "B", "C"]);
        let removed = vault.remove_entry_by_id("C").unwrap();
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
