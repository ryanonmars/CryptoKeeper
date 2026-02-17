use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to vault file (default: ~/.cryptokeeper/vault.ck)
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
    format!("{}/.cryptokeeper/vault.ck", home)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Index of the preset recovery question (0, 1, or 2)
    pub question_index: u8,

    /// Argon2 hash of the normalized answer (for verification)
    pub answer_hash: Vec<u8>,

    /// Salt used for answer hashing
    pub answer_salt: Vec<u8>,

    /// Vault master key encrypted under recovery-derived key
    pub master_key_blob: Vec<u8>,

    /// Nonce for master key blob encryption
    pub master_key_blob_nonce: Vec<u8>,

    /// Salt for recovery key derivation
    pub master_key_blob_salt: Vec<u8>,
}

pub const RECOVERY_QUESTIONS: [&str; 3] = [
    "What was the name of your first pet?",
    "What city were you born in?",
    "What was your childhood nickname?",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = Config::default();
        assert_eq!(config.clipboard_timeout_secs, 10);
        assert!(!config.first_run_complete);
        assert!(config.recovery.is_none());
        assert!(config.vault_path.ends_with(".cryptokeeper/vault.ck"));
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
        let recovery = RecoveryConfig {
            question_index: 1,
            answer_hash: vec![1, 2, 3],
            answer_salt: vec![4, 5, 6],
            master_key_blob: vec![7, 8, 9],
            master_key_blob_nonce: vec![10, 11, 12],
            master_key_blob_salt: vec![13, 14, 15],
        };
        let config = Config {
            recovery: Some(recovery),
            ..Config::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();
        let r = loaded.recovery.unwrap();
        assert_eq!(r.question_index, 1);
        assert_eq!(r.answer_hash, vec![1, 2, 3]);
    }
}
