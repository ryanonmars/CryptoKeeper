use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};

pub const DEFAULT_M_COST: u32 = 65536; // 64 MB
pub const DEFAULT_T_COST: u32 = 3;     // 3 iterations
pub const DEFAULT_P_COST: u32 = 4;     // 4 parallel lanes

/// Derive a 32-byte key from password and salt using Argon2id.
pub fn derive_key(
    password: &[u8],
    salt: &[u8; 32],
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(m_cost, t_cost, p_cost, Some(32))
        .map_err(|e| CryptoKeeperError::Encryption(format!("Argon2 params error: {e}")))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(password, salt, key.as_mut())
        .map_err(|e| CryptoKeeperError::Encryption(format!("Argon2 derivation error: {e}")))?;

    Ok(key)
}

pub fn generate_salt() -> [u8; 32] {
    use rand::RngCore;
    let mut salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_key_deterministic() {
        let password = b"test-password";
        let salt = [42u8; 32];
        // Use reduced params for test speed
        let key1 = derive_key(password, &salt, 1024, 1, 1).unwrap();
        let key2 = derive_key(password, &salt, 1024, 1, 1).unwrap();
        assert_eq!(key1.as_ref(), key2.as_ref());
    }

    #[test]
    fn test_derive_key_different_passwords() {
        let salt = [42u8; 32];
        let key1 = derive_key(b"password1", &salt, 1024, 1, 1).unwrap();
        let key2 = derive_key(b"password2", &salt, 1024, 1, 1).unwrap();
        assert_ne!(key1.as_ref(), key2.as_ref());
    }

    #[test]
    fn test_derive_key_different_salts() {
        let password = b"test-password";
        let salt1 = [1u8; 32];
        let salt2 = [2u8; 32];
        let key1 = derive_key(password, &salt1, 1024, 1, 1).unwrap();
        let key2 = derive_key(password, &salt2, 1024, 1, 1).unwrap();
        assert_ne!(key1.as_ref(), key2.as_ref());
    }

    #[test]
    fn test_generate_salt_unique() {
        let salt1 = generate_salt();
        let salt2 = generate_salt();
        assert_ne!(salt1, salt2);
    }
}
