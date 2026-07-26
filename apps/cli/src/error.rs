use thiserror::Error;

#[derive(Error, Debug)]
pub enum TermKeyError {
    #[error("Vault not found. Run `termkey init` first.")]
    VaultNotFound,

    #[error("Vault already exists at {0}")]
    VaultAlreadyExists(String),

    #[error("Invalid master password — decryption failed.")]
    DecryptionFailed,

    #[error("Invalid vault file — corrupted or wrong format.")]
    InvalidVaultFormat,

    #[error("Unsupported vault format version: {0}.")]
    UnsupportedVaultVersion(u32),

    #[error("Vault KDF parameters are outside the accepted policy.")]
    KdfParametersOutOfPolicy,

    #[error("Vault ciphertext exceeds the maximum supported size.")]
    VaultTooLarge,

    #[error("Vault changed on disk. Reopen it before saving.")]
    VaultConflict,

    #[error("Entry '{0}' not found. Use `termkey list` to see entries with their index numbers.")]
    EntryNotFound(String),

    #[error("Entry '{0}' already exists.")]
    EntryAlreadyExists(String),

    #[error("Invalid entry: {0}")]
    InvalidEntry(String),

    #[error("No entries match '{0}'.")]
    NoSearchResults(String),

    #[error("Passwords do not match.")]
    PasswordMismatch,

    #[error("Secrets do not match. Please enter the same value twice.")]
    SecretMismatch,

    #[error("Password cannot be empty.")]
    EmptyPassword,

    #[error("Operation cancelled.")]
    Cancelled,

    #[error("Clipboard error: {0}")]
    Clipboard(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("This entry requires a secondary password to view.")]
    SecondaryPasswordRequired,

    #[error("Incorrect secondary password.")]
    SecondaryPasswordWrong,

    #[error("Password recovery is not configured. Set it up via settings or `termkey init`.")]
    RecoveryNotConfigured,

    #[error(
        "Legacy security-question recovery is no longer supported. Unlock with your master password, then configure a new recovery phrase."
    )]
    LegacyRecoveryUnsupported,

    #[error("Recovery failed: {0}")]
    RecoveryFailed(String),

    #[error("Address derivation failed: {0}")]
    DerivationFailed(String),

    #[error("Config error: {0}")]
    ConfigError(String),
}

pub type Result<T> = std::result::Result<T, TermKeyError>;
