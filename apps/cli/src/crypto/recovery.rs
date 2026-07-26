use bip39::Mnemonic;
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use zeroize::Zeroizing;

use crate::config::model::RecoveryConfigV2;
use crate::crypto::kdf;
use crate::error::{Result, TermKeyError};
use crate::vault::format::VaultId;

const RECOVERY_VERSION: u8 = 2;
const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const DEK_LEN: usize = 32;
const AUTH_TAG_LEN: usize = 16;
const WRAPPED_DEK_LEN: usize = DEK_LEN + AUTH_TAG_LEN;
const RECOVERY_AAD_DOMAIN: &[u8] = b"termkey-recovery-v2";

pub fn generate_recovery_phrase() -> Result<Zeroizing<String>> {
    let mut entropy = Zeroizing::new([0u8; 32]);
    rand::thread_rng().fill_bytes(entropy.as_mut());
    let mnemonic = Mnemonic::from_entropy(entropy.as_ref())
        .map_err(|_| TermKeyError::RecoveryFailed("Could not generate recovery phrase".into()))?;
    Ok(Zeroizing::new(mnemonic.to_string()))
}

pub fn create_recovery_config(
    vault_id: VaultId,
    dek: &[u8; DEK_LEN],
    phrase: &str,
) -> Result<RecoveryConfigV2> {
    let canonical_phrase = canonicalize_phrase(phrase)?;
    let salt = kdf::generate_salt();
    let recovery_key = kdf::derive_key(
        canonical_phrase.as_bytes(),
        &salt,
        kdf::DEFAULT_M_COST,
        kdf::DEFAULT_T_COST,
        kdf::DEFAULT_P_COST,
    )?;
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let wrapped_dek = XChaCha20Poly1305::new((&*recovery_key).into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: dek,
                aad: &recovery_aad(vault_id),
            },
        )
        .map_err(|_| TermKeyError::Encryption("Recovery DEK wrapping failed".into()))?;

    Ok(RecoveryConfigV2 {
        version: RECOVERY_VERSION,
        vault_id,
        salt: salt.to_vec(),
        nonce: nonce.to_vec(),
        wrapped_dek,
    })
}

pub fn recover_dek(
    config: &RecoveryConfigV2,
    vault_id: VaultId,
    phrase: &str,
) -> Result<Zeroizing<[u8; 32]>> {
    if config.version != RECOVERY_VERSION {
        return Err(TermKeyError::RecoveryFailed(
            "Unsupported recovery configuration version".into(),
        ));
    }
    if config.vault_id != vault_id {
        return Err(TermKeyError::RecoveryFailed(
            "Recovery configuration belongs to a different vault".into(),
        ));
    }
    let salt: &[u8; SALT_LEN] = config
        .salt
        .as_slice()
        .try_into()
        .map_err(|_| TermKeyError::RecoveryFailed("Invalid recovery salt length".into()))?;
    let nonce: &[u8; NONCE_LEN] = config
        .nonce
        .as_slice()
        .try_into()
        .map_err(|_| TermKeyError::RecoveryFailed("Invalid recovery nonce length".into()))?;
    if config.wrapped_dek.len() != WRAPPED_DEK_LEN {
        return Err(TermKeyError::RecoveryFailed(
            "Invalid wrapped DEK length".into(),
        ));
    }

    let canonical_phrase = canonicalize_phrase(phrase)?;
    let recovery_key = kdf::derive_key(
        canonical_phrase.as_bytes(),
        salt,
        kdf::DEFAULT_M_COST,
        kdf::DEFAULT_T_COST,
        kdf::DEFAULT_P_COST,
    )?;
    let plaintext = XChaCha20Poly1305::new((&*recovery_key).into())
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: &config.wrapped_dek,
                aad: &recovery_aad(vault_id),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| {
            TermKeyError::RecoveryFailed("Incorrect phrase or corrupted recovery data".into())
        })?;
    if plaintext.len() != DEK_LEN {
        return Err(TermKeyError::RecoveryFailed(
            "Invalid recovered DEK length".into(),
        ));
    }
    let mut dek = Zeroizing::new([0u8; DEK_LEN]);
    dek.copy_from_slice(plaintext.as_slice());
    Ok(dek)
}

fn canonicalize_phrase(phrase: &str) -> Result<Zeroizing<String>> {
    let mnemonic = Mnemonic::parse(phrase)
        .map_err(|_| TermKeyError::RecoveryFailed("Invalid recovery phrase".into()))?;
    if mnemonic.word_count() != 24 {
        return Err(TermKeyError::RecoveryFailed(
            "Recovery phrase must contain exactly 24 words".into(),
        ));
    }
    Ok(Zeroizing::new(mnemonic.to_string()))
}

fn recovery_aad(vault_id: VaultId) -> Vec<u8> {
    let mut aad = Vec::with_capacity(
        RECOVERY_AAD_DOMAIN.len() + std::mem::size_of::<u8>() + vault_id.0.len(),
    );
    aad.extend_from_slice(RECOVERY_AAD_DOMAIN);
    aad.push(RECOVERY_VERSION);
    aad.extend_from_slice(&vault_id.0);
    aad
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::format::VaultId;

    const VALID_12_WORD_PHRASE: &str =
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    fn structurally_valid_config(vault_id: VaultId) -> RecoveryConfigV2 {
        RecoveryConfigV2 {
            version: RECOVERY_VERSION,
            vault_id,
            salt: vec![0x11; SALT_LEN],
            nonce: vec![0x22; NONCE_LEN],
            wrapped_dek: vec![0x33; WRAPPED_DEK_LEN],
        }
    }

    fn recovery_error(result: Result<Zeroizing<[u8; DEK_LEN]>>) -> String {
        match result {
            Ok(_) => panic!("recovery unexpectedly succeeded"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn generated_recovery_phrase_has_24_words() {
        let phrase = generate_recovery_phrase().unwrap();

        assert_eq!(phrase.split_whitespace().count(), 24);
        assert_eq!(
            bip39::Mnemonic::parse(phrase.as_str()).unwrap().to_string(),
            *phrase
        );
    }

    #[test]
    fn recovery_wrap_round_trip_returns_same_dek() {
        let vault_id = VaultId([0x11; 16]);
        let dek = [0x22; 32];
        let phrase = generate_recovery_phrase().unwrap();

        let config = create_recovery_config(vault_id, &dek, &phrase).unwrap();
        let recovered = recover_dek(&config, vault_id, &phrase).unwrap();

        assert_eq!(*recovered, dek);
    }

    #[test]
    fn wrong_phrase_fails_without_answer_verifier() {
        let vault_id = VaultId([0x33; 16]);
        let dek = [0x44; 32];
        let phrase = generate_recovery_phrase().unwrap();
        let wrong_phrase = generate_recovery_phrase().unwrap();
        let config = create_recovery_config(vault_id, &dek, &phrase).unwrap();

        assert!(recover_dek(&config, vault_id, &wrong_phrase).is_err());
        let serialized = serde_json::to_value(&config).unwrap();
        assert!(serialized.get("answer_hash").is_none());
        assert!(serialized.get("answer_salt").is_none());
    }

    #[test]
    fn recovery_config_rejects_different_vault_id() {
        let vault_id = VaultId([0x55; 16]);
        let different_vault_id = VaultId([0x56; 16]);
        let dek = [0x66; 32];
        let phrase = generate_recovery_phrase().unwrap();
        let config = create_recovery_config(vault_id, &dek, &phrase).unwrap();

        assert!(recover_dek(&config, different_vault_id, &phrase).is_err());
    }

    #[test]
    fn create_recovery_config_rejects_valid_12_word_phrase() {
        let result =
            create_recovery_config(VaultId([0x71; 16]), &[0x72; DEK_LEN], VALID_12_WORD_PHRASE);

        let error = result.unwrap_err().to_string();
        assert!(error.contains("24 words"), "unexpected error: {error}");
    }

    #[test]
    fn recover_dek_rejects_valid_12_word_phrase() {
        let vault_id = VaultId([0x73; 16]);
        let config = structurally_valid_config(vault_id);

        let error = recovery_error(recover_dek(&config, vault_id, VALID_12_WORD_PHRASE));
        assert!(error.contains("24 words"), "unexpected error: {error}");
    }

    #[test]
    fn recovery_phrase_is_canonicalized_before_derivation() {
        let vault_id = VaultId([0x74; 16]);
        let dek = [0x75; DEK_LEN];
        let phrase = generate_recovery_phrase().unwrap();
        let spaced_phrase = format!(
            "  {}  ",
            phrase.split_whitespace().collect::<Vec<_>>().join("   ")
        );

        let config_from_spaced = create_recovery_config(vault_id, &dek, &spaced_phrase).unwrap();
        let recovered_from_canonical = recover_dek(&config_from_spaced, vault_id, &phrase).unwrap();
        let config_from_canonical = create_recovery_config(vault_id, &dek, &phrase).unwrap();
        let recovered_from_spaced =
            recover_dek(&config_from_canonical, vault_id, &spaced_phrase).unwrap();

        assert_eq!(*recovered_from_canonical, dek);
        assert_eq!(*recovered_from_spaced, dek);
    }

    #[test]
    fn unsupported_version_is_rejected_before_phrase_parsing() {
        let vault_id = VaultId([0x76; 16]);
        let mut config = structurally_valid_config(vault_id);
        config.version = RECOVERY_VERSION + 1;

        let error = recovery_error(recover_dek(&config, vault_id, "not a mnemonic"));
        assert!(
            error.contains("Unsupported recovery configuration version"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn malformed_salt_is_rejected_before_phrase_parsing() {
        let vault_id = VaultId([0x77; 16]);
        let mut config = structurally_valid_config(vault_id);
        config.salt.pop();

        let error = recovery_error(recover_dek(&config, vault_id, "not a mnemonic"));
        assert!(
            error.contains("Invalid recovery salt length"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn malformed_nonce_is_rejected_before_phrase_parsing() {
        let vault_id = VaultId([0x78; 16]);
        let mut config = structurally_valid_config(vault_id);
        config.nonce.push(0);

        let error = recovery_error(recover_dek(&config, vault_id, "not a mnemonic"));
        assert!(
            error.contains("Invalid recovery nonce length"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn malformed_wrapped_dek_is_rejected_before_phrase_parsing() {
        let vault_id = VaultId([0x79; 16]);
        let mut config = structurally_valid_config(vault_id);
        config.wrapped_dek.clear();

        let error = recovery_error(recover_dek(&config, vault_id, "not a mnemonic"));
        assert!(
            error.contains("Invalid wrapped DEK length"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn vault_id_is_cryptographically_bound_after_equality_check() {
        let original_vault_id = VaultId([0x7a; 16]);
        let changed_vault_id = VaultId([0x7b; 16]);
        let dek = [0x7c; DEK_LEN];
        let phrase = generate_recovery_phrase().unwrap();
        let mut config = create_recovery_config(original_vault_id, &dek, &phrase).unwrap();
        config.vault_id = changed_vault_id;

        let error = recovery_error(recover_dek(&config, changed_vault_id, &phrase));
        assert!(
            error.contains("Incorrect phrase or corrupted recovery data"),
            "unexpected error: {error}"
        );
    }
}
