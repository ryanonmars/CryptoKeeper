use zeroize::Zeroizing;

use crate::crypto::{cipher, kdf};
use crate::error::{CryptoKeeperError, Result};

/// Argon2 params for per-entry key wrapping (lighter than vault KDF).
fn entry_key_params() -> (u32, u32, u32) {
    if cfg!(test) {
        (1024, 1, 1)
    } else {
        (16384, 2, 1) // 16 MB, 2 iterations, 1 lane
    }
}

/// Generate a random 32-byte per-entry encryption key.
pub fn generate_entry_key() -> Zeroizing<[u8; 32]> {
    use rand::RngCore;
    let mut key = Zeroizing::new([0u8; 32]);
    rand::thread_rng().fill_bytes(key.as_mut());
    key
}

/// Encrypt a secret with a per-entry key using XChaCha20-Poly1305.
/// Returns (ciphertext, nonce).
pub fn encrypt_secret(entry_key: &[u8; 32], plaintext: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let nonce = cipher::generate_nonce();
    let ciphertext = cipher::encrypt(entry_key, &nonce, plaintext.as_bytes())?;
    Ok((ciphertext, nonce.to_vec()))
}

/// Decrypt a secret with a per-entry key.
pub fn decrypt_secret(
    entry_key: &[u8; 32],
    ciphertext: &[u8],
    nonce: &[u8],
) -> Result<Zeroizing<String>> {
    let mut nonce_arr = [0u8; 24];
    let n = nonce.len().min(24);
    nonce_arr[..n].copy_from_slice(&nonce[..n]);
    let plaintext = cipher::decrypt(entry_key, &nonce_arr, ciphertext)
        .map_err(|_| CryptoKeeperError::SecondaryPasswordWrong)?;
    let s = String::from_utf8(plaintext.to_vec())
        .map_err(|_| CryptoKeeperError::Encryption("Invalid UTF-8 in decrypted secret".into()))?;
    Ok(Zeroizing::new(s))
}

/// Wrap (encrypt) a per-entry key under a view password using Argon2 + XChaCha20.
/// Returns (wrapped_key, nonce, salt).
pub fn wrap_entry_key(
    entry_key: &[u8; 32],
    view_password: &str,
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let salt = kdf::generate_salt();
    let (m, t, p) = entry_key_params();
    let wrapping_key = kdf::derive_key(view_password.as_bytes(), &salt, m, t, p)?;
    let nonce = cipher::generate_nonce();
    let wrapped = cipher::encrypt(&*wrapping_key, &nonce, entry_key)?;
    Ok((wrapped, nonce.to_vec(), salt.to_vec()))
}

/// Unwrap (decrypt) a per-entry key using a view password.
pub fn unwrap_entry_key(
    wrapped: &[u8],
    nonce: &[u8],
    salt: &[u8],
    view_password: &str,
) -> Result<Zeroizing<[u8; 32]>> {
    let mut salt_arr = [0u8; 32];
    let s = salt.len().min(32);
    salt_arr[..s].copy_from_slice(&salt[..s]);
    let (m, t, p) = entry_key_params();
    let wrapping_key = kdf::derive_key(view_password.as_bytes(), &salt_arr, m, t, p)?;
    let mut nonce_arr = [0u8; 24];
    let n = nonce.len().min(24);
    nonce_arr[..n].copy_from_slice(&nonce[..n]);
    let plaintext = cipher::decrypt(&*wrapping_key, &nonce_arr, wrapped)
        .map_err(|_| CryptoKeeperError::SecondaryPasswordWrong)?;
    if plaintext.len() != 32 {
        return Err(CryptoKeeperError::SecondaryPasswordWrong);
    }
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&plaintext);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_entry_key_unique() {
        let k1 = generate_entry_key();
        let k2 = generate_entry_key();
        assert_ne!(*k1, *k2);
    }

    #[test]
    fn test_encrypt_decrypt_secret_roundtrip() {
        let key = generate_entry_key();
        let secret = "my super secret private key";
        let (ct, nonce) = encrypt_secret(&key, secret).unwrap();
        let decrypted = decrypt_secret(&key, &ct, &nonce).unwrap();
        assert_eq!(&*decrypted, secret);
    }

    #[test]
    fn test_decrypt_secret_wrong_key() {
        let key1 = generate_entry_key();
        let key2 = generate_entry_key();
        let (ct, nonce) = encrypt_secret(&key1, "secret").unwrap();
        let result = decrypt_secret(&key2, &ct, &nonce);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrap_unwrap_entry_key_roundtrip() {
        let entry_key = generate_entry_key();
        let password = "viewpass123";
        let (wrapped, nonce, salt) = wrap_entry_key(&entry_key, password).unwrap();
        let unwrapped = unwrap_entry_key(&wrapped, &nonce, &salt, password).unwrap();
        assert_eq!(*unwrapped, *entry_key);
    }

    #[test]
    fn test_unwrap_entry_key_wrong_password() {
        let entry_key = generate_entry_key();
        let (wrapped, nonce, salt) = wrap_entry_key(&entry_key, "correct").unwrap();
        let result = unwrap_entry_key(&wrapped, &nonce, &salt, "wrong");
        assert!(result.is_err());
    }

    #[test]
    fn test_full_secondary_password_flow() {
        let entry_key = generate_entry_key();
        let view_password = "my-view-pass";
        let secret = "0xdeadbeefcafebabe";

        // Encrypt secret with entry key
        let (encrypted_secret, secret_nonce) = encrypt_secret(&entry_key, secret).unwrap();

        // Wrap entry key with view password
        let (wrapped_key, key_nonce, key_salt) =
            wrap_entry_key(&entry_key, view_password).unwrap();

        // Later: unwrap entry key with view password
        let recovered_key =
            unwrap_entry_key(&wrapped_key, &key_nonce, &key_salt, view_password).unwrap();

        // Decrypt secret with recovered entry key
        let decrypted = decrypt_secret(&recovered_key, &encrypted_secret, &secret_nonce).unwrap();
        assert_eq!(&*decrypted, secret);
    }
}
