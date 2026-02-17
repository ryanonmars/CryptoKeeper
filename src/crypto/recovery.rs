use zeroize::Zeroizing;

use crate::crypto::{cipher, kdf};
use crate::error::{CryptoKeeperError, Result};

pub const MIN_ANSWER_LENGTH: usize = 3;

/// Normalize a recovery answer: trim, lowercase, collapse whitespace.
pub fn normalize_answer(answer: &str) -> String {
    answer
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Argon2 params for recovery (lighter than vault KDF for interactive use).
fn recovery_params() -> (u32, u32, u32) {
    if cfg!(test) {
        (1024, 1, 1)
    } else {
        (16384, 2, 1) // 16 MB, 2 iterations, 1 lane
    }
}

/// Hash a normalized answer with Argon2 for verification.
pub fn hash_answer(answer: &str, salt: &[u8]) -> Result<Vec<u8>> {
    let mut salt_arr = [0u8; 32];
    let copy_len = salt.len().min(32);
    salt_arr[..copy_len].copy_from_slice(&salt[..copy_len]);
    let (m, t, p) = recovery_params();
    let key = kdf::derive_key(answer.as_bytes(), &salt_arr, m, t, p)?;
    Ok(key.to_vec())
}

/// Verify a normalized answer against a stored hash.
pub fn verify_answer(answer: &str, salt: &[u8], expected_hash: &[u8]) -> Result<bool> {
    let hash = hash_answer(answer, salt)?;
    Ok(hash == expected_hash)
}

/// Encrypt the master key under a recovery-answer-derived key.
pub fn create_recovery_blob(
    master_key: &[u8; 32],
    answer: &str,
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let salt = kdf::generate_salt();
    let (m, t, p) = recovery_params();
    let recovery_key = kdf::derive_key(answer.as_bytes(), &salt, m, t, p)?;
    let nonce = cipher::generate_nonce();
    let blob = cipher::encrypt(&*recovery_key, &nonce, master_key)?;
    Ok((blob, nonce.to_vec(), salt.to_vec()))
}

/// Decrypt the master key from a recovery blob using the answer.
pub fn decrypt_recovery_blob(
    blob: &[u8],
    nonce: &[u8],
    salt: &[u8],
    answer: &str,
) -> Result<Zeroizing<[u8; 32]>> {
    let mut salt_arr = [0u8; 32];
    let copy_len = salt.len().min(32);
    salt_arr[..copy_len].copy_from_slice(&salt[..copy_len]);
    let (m, t, p) = recovery_params();
    let recovery_key = kdf::derive_key(answer.as_bytes(), &salt_arr, m, t, p)?;
    let mut nonce_arr = [0u8; 24];
    let nonce_len = nonce.len().min(24);
    nonce_arr[..nonce_len].copy_from_slice(&nonce[..nonce_len]);
    let plaintext = cipher::decrypt(&*recovery_key, &nonce_arr, blob)
        .map_err(|_| CryptoKeeperError::RecoveryFailed("Incorrect answer or corrupted blob".into()))?;
    if plaintext.len() != 32 {
        return Err(CryptoKeeperError::RecoveryFailed(
            "Invalid master key length in recovery blob".into(),
        ));
    }
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&plaintext);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_answer() {
        assert_eq!(normalize_answer("  Fluffy  "), "fluffy");
        assert_eq!(normalize_answer("New  York  City"), "new york city");
        assert_eq!(normalize_answer("  BUDDY  "), "buddy");
        assert_eq!(normalize_answer(""), "");
        assert_eq!(normalize_answer("  a  b  c  "), "a b c");
    }

    #[test]
    fn test_hash_and_verify() {
        let salt = vec![42u8; 32];
        let hash = hash_answer("fluffy", &salt).unwrap();
        assert!(verify_answer("fluffy", &salt, &hash).unwrap());
        assert!(!verify_answer("wrong", &salt, &hash).unwrap());
    }

    #[test]
    fn test_recovery_blob_roundtrip() {
        let master_key = [0xABu8; 32];
        let answer = "fluffy";
        let (blob, nonce, salt) = create_recovery_blob(&master_key, answer).unwrap();
        let recovered = decrypt_recovery_blob(&blob, &nonce, &salt, answer).unwrap();
        assert_eq!(*recovered, master_key);
    }

    #[test]
    fn test_recovery_blob_wrong_answer() {
        let master_key = [0xABu8; 32];
        let (blob, nonce, salt) = create_recovery_blob(&master_key, "fluffy").unwrap();
        let result = decrypt_recovery_blob(&blob, &nonce, &salt, "wrong");
        assert!(result.is_err());
    }
}
