use serde::{Deserialize, Serialize};

use crate::vault::format::VaultId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Path to vault file (default: ~/.termkey/vault.ck)
    #[serde(default = "default_vault_path")]
    pub vault_path: String,

    /// Seconds before clipboard auto-clears (default: 10)
    #[serde(default = "default_clipboard_timeout")]
    pub clipboard_timeout_secs: u64,

    /// Whether the first-run wizard has been completed
    #[serde(default)]
    pub first_run_complete: bool,

    /// Password recovery configuration (None if not set up)
    #[serde(default)]
    pub recovery: Option<RecoveryConfig>,
}

fn default_vault_path() -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    format!("{}/.termkey/vault.ck", home)
}

fn default_clipboard_timeout() -> u64 {
    10
}

impl Default for Config {
    fn default() -> Self {
        Self {
            vault_path: default_vault_path(),
            clipboard_timeout_secs: default_clipboard_timeout(),
            first_run_complete: false,
            recovery: None,
        }
    }
}

impl Config {
    pub fn has_active_recovery_for(&self, vault_id: VaultId) -> bool {
        matches!(
            self.recovery.as_ref(),
            Some(RecoveryConfig::V2(recovery)) if recovery.is_valid_for(vault_id)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RecoveryConfig {
    V2(RecoveryConfigV2),
    Legacy(LegacyRecoveryConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryConfigV2 {
    pub version: u8,
    pub vault_id: VaultId,
    pub salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub wrapped_dek: Vec<u8>,
}

impl RecoveryConfigV2 {
    pub const VERSION: u8 = 2;
    pub const SALT_LEN: usize = 32;
    pub const NONCE_LEN: usize = 24;
    pub const WRAPPED_DEK_LEN: usize = 48;

    pub fn is_valid_for(&self, vault_id: VaultId) -> bool {
        self.version == Self::VERSION
            && self.vault_id == vault_id
            && self.salt.len() == Self::SALT_LEN
            && self.nonce.len() == Self::NONCE_LEN
            && self.wrapped_dek.len() == Self::WRAPPED_DEK_LEN
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRecoveryConfig {
    question_index: u8,
    answer_hash: Vec<u8>,
    answer_salt: Vec<u8>,
    master_key_blob: Vec<u8>,
    master_key_blob_nonce: Vec<u8>,
    master_key_blob_salt: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = Config::default();
        assert_eq!(config.clipboard_timeout_secs, 10);
        assert!(!config.first_run_complete);
        assert!(config.recovery.is_none());
        assert!(config.vault_path.ends_with(".termkey/vault.ck"));
    }

    #[test]
    fn config_roundtrip_json() {
        let config = Config {
            vault_path: "/custom/path/vault.ck".to_string(),
            clipboard_timeout_secs: 30,
            first_run_complete: true,
            recovery: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.vault_path, "/custom/path/vault.ck");
        assert_eq!(loaded.clipboard_timeout_secs, 30);
        assert!(loaded.first_run_complete);
    }

    #[test]
    fn config_deserialize_missing_fields() {
        let json = r#"{}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.clipboard_timeout_secs, 10);
        assert!(!config.first_run_complete);
        assert!(config.recovery.is_none());
    }

    #[test]
    fn recovery_config_roundtrip() {
        let recovery = RecoveryConfig::V2(RecoveryConfigV2 {
            version: 2,
            vault_id: VaultId([1; 16]),
            salt: vec![2; 32],
            nonce: vec![3; 24],
            wrapped_dek: vec![4; 48],
        });
        let config = Config {
            recovery: Some(recovery),
            ..Config::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();
        let Some(RecoveryConfig::V2(recovery)) = loaded.recovery else {
            panic!("v2 recovery config did not round-trip as v2");
        };
        assert_eq!(recovery.version, 2);
        assert_eq!(recovery.vault_id, VaultId([1; 16]));
        assert_eq!(recovery.salt, vec![2; 32]);
        assert_eq!(recovery.nonce, vec![3; 24]);
        assert_eq!(recovery.wrapped_dek, vec![4; 48]);
    }

    #[test]
    fn legacy_recovery_deserializes_as_unsupported() {
        let json = r#"{
            "recovery": {
                "question_index": 1,
                "answer_hash": [1, 2, 3],
                "answer_salt": [4, 5, 6],
                "master_key_blob": [7, 8, 9],
                "master_key_blob_nonce": [10, 11, 12],
                "master_key_blob_salt": [13, 14, 15]
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert!(matches!(config.recovery, Some(RecoveryConfig::Legacy(_))));
    }

    #[test]
    fn mixed_v2_and_legacy_recovery_object_is_rejected() {
        let json = r#"{
            "recovery": {
                "version": 2,
                "vault_id": [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
                "salt": [2, 2],
                "nonce": [3, 3],
                "wrapped_dek": [4, 4],
                "question_index": 1,
                "answer_hash": [5],
                "answer_salt": [6],
                "master_key_blob": [7],
                "master_key_blob_nonce": [8],
                "master_key_blob_salt": [9]
            }
        }"#;

        assert!(serde_json::from_str::<Config>(json).is_err());
    }

    #[test]
    fn active_v2_recovery_requires_matching_vault_and_valid_structure() {
        let vault_id = VaultId([0x61; 16]);
        assert!(!Config::default().has_active_recovery_for(vault_id));
        let legacy: Config = serde_json::from_str(
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
        assert!(!legacy.has_active_recovery_for(vault_id));

        let valid = RecoveryConfigV2 {
            version: 2,
            vault_id,
            salt: vec![0x62; 32],
            nonce: vec![0x63; 24],
            wrapped_dek: vec![0x64; 48],
        };
        let config = Config {
            recovery: Some(RecoveryConfig::V2(valid.clone())),
            ..Config::default()
        };

        assert!(config.has_active_recovery_for(vault_id));
        assert!(!config.has_active_recovery_for(VaultId([0x65; 16])));

        for invalid in [
            RecoveryConfigV2 {
                version: 1,
                ..valid.clone()
            },
            RecoveryConfigV2 {
                salt: vec![0; 31],
                ..valid.clone()
            },
            RecoveryConfigV2 {
                nonce: vec![0; 23],
                ..valid.clone()
            },
            RecoveryConfigV2 {
                wrapped_dek: vec![0; 47],
                ..valid
            },
        ] {
            let config = Config {
                recovery: Some(RecoveryConfig::V2(invalid)),
                ..Config::default()
            };
            assert!(!config.has_active_recovery_for(vault_id));
        }
    }
}
