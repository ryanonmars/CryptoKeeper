use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};

/// Encrypt plaintext with XChaCha20-Poly1305.
/// Returns ciphertext with appended Poly1305 tag (16 bytes).
pub fn encrypt(key: &[u8; 32], nonce: &[u8; 24], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let xnonce = XNonce::from_slice(nonce);
    cipher
        .encrypt(xnonce, plaintext)
        .map_err(|e| CryptoKeeperError::Encryption(format!("Encryption failed: {e}")))
}

/// Decrypt ciphertext (with appended Poly1305 tag) using XChaCha20-Poly1305.
/// Returns plaintext wrapped in Zeroizing for automatic cleanup.
pub fn decrypt(key: &[u8; 32], nonce: &[u8; 24], ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let xnonce = XNonce::from_slice(nonce);
    let plaintext = cipher
        .decrypt(xnonce, ciphertext)
        .map_err(|_| CryptoKeeperError::DecryptionFailed)?;
    Ok(Zeroizing::new(plaintext))
}

pub fn generate_nonce() -> [u8; 24] {
    use rand::RngCore;
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = [0xABu8; 32];
        let nonce = [0xCDu8; 24];
        let plaintext = b"Hello, CryptoKeeper!";

        let ciphertext = encrypt(&key, &nonce, plaintext).unwrap();
        assert_ne!(ciphertext.as_slice(), plaintext);

        let decrypted = decrypt(&key, &nonce, &ciphertext).unwrap();
        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn test_decrypt_wrong_key() {
        let key = [0xABu8; 32];
        let wrong_key = [0xCDu8; 32];
        let nonce = [0xEFu8; 24];
        let plaintext = b"secret data";

        let ciphertext = encrypt(&key, &nonce, plaintext).unwrap();
        let result = decrypt(&wrong_key, &nonce, &ciphertext);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_wrong_nonce() {
        let key = [0xABu8; 32];
        let nonce = [0xCDu8; 24];
        let wrong_nonce = [0xEFu8; 24];
        let plaintext = b"secret data";

        let ciphertext = encrypt(&key, &nonce, plaintext).unwrap();
        let result = decrypt(&key, &wrong_nonce, &ciphertext);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_tampered_ciphertext() {
        let key = [0xABu8; 32];
        let nonce = [0xCDu8; 24];
        let plaintext = b"secret data";

        let mut ciphertext = encrypt(&key, &nonce, plaintext).unwrap();
        // Tamper with ciphertext
        if let Some(byte) = ciphertext.first_mut() {
            *byte ^= 0xFF;
        }
        let result = decrypt(&key, &nonce, &ciphertext);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_nonce_unique() {
        let n1 = generate_nonce();
        let n2 = generate_nonce();
        assert_ne!(n1, n2);
    }
}
