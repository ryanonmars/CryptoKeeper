use crate::error::{CryptoKeeperError, Result};
use crate::vault::model::SecretType;

/// Derive a public address from a secret (private key or seed phrase).
/// Returns Ok(None) for unsupported network/type combos.
pub fn derive_address(
    secret: &str,
    secret_type: &SecretType,
    network: &str,
) -> Result<Option<String>> {
    let network_lower = network.to_lowercase();

    match (secret_type, network_lower.as_str()) {
        #[cfg(feature = "derive-eth")]
        (SecretType::PrivateKey, "ethereum" | "eth") => derive_eth_from_privkey(secret).map(Some),

        #[cfg(feature = "derive-eth")]
        (SecretType::SeedPhrase, "ethereum" | "eth") => derive_eth_from_seed(secret).map(Some),

        #[cfg(feature = "derive-btc")]
        (SecretType::PrivateKey, "bitcoin" | "btc") => derive_btc_from_privkey(secret).map(Some),

        #[cfg(feature = "derive-btc")]
        (SecretType::SeedPhrase, "bitcoin" | "btc") => derive_btc_from_seed(secret).map(Some),

        #[cfg(feature = "derive-sol")]
        (SecretType::PrivateKey, "solana" | "sol") => derive_sol_from_privkey(secret).map(Some),

        #[cfg(feature = "derive-sol")]
        (SecretType::SeedPhrase, "solana" | "sol") => derive_sol_from_seed(secret).map(Some),

        _ => Ok(None),
    }
}

// ─── Ethereum ────────────────────────────────────────────────────────

#[cfg(feature = "derive-eth")]
fn parse_hex_key(secret: &str) -> Result<[u8; 32]> {
    let hex_str = secret.trim().strip_prefix("0x").unwrap_or(secret.trim());
    let bytes = hex::decode(hex_str)
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("Invalid hex key: {}", e)))?;
    if bytes.len() != 32 {
        return Err(CryptoKeeperError::DerivationFailed(format!(
            "Expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

#[cfg(feature = "derive-eth")]
fn eth_address_from_pubkey_bytes(uncompressed: &[u8]) -> String {
    use sha3::Digest;
    // Skip the 0x04 prefix byte for uncompressed key
    let pubkey_bytes = if uncompressed.len() == 65 && uncompressed[0] == 0x04 {
        &uncompressed[1..]
    } else {
        uncompressed
    };
    let hash = sha3::Keccak256::digest(pubkey_bytes);
    let addr_bytes = &hash[12..];
    format!("0x{}", hex::encode(addr_bytes))
}

#[cfg(feature = "derive-eth")]
fn derive_eth_from_privkey(secret: &str) -> Result<String> {
    use k256::ecdsa::SigningKey;
    let key_bytes = parse_hex_key(secret)?;
    let signing_key = SigningKey::from_bytes((&key_bytes).into())
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("Invalid ETH private key: {}", e)))?;
    let verifying_key = signing_key.verifying_key();
    let point = verifying_key.to_encoded_point(false);
    Ok(eth_address_from_pubkey_bytes(point.as_bytes()))
}

#[cfg(feature = "derive-eth")]
fn derive_eth_from_seed(secret: &str) -> Result<String> {
    use k256::ecdsa::SigningKey;
    let mnemonic = bip39::Mnemonic::parse(secret.trim())
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("Invalid mnemonic: {}", e)))?;
    let seed = mnemonic.to_seed("");

    // BIP32 derivation: m/44'/60'/0'/0/0
    // Simple HMAC-SHA512 based derivation
    let key_bytes = bip32_derive_secp256k1(&seed, &[
        0x8000002C, // 44'
        0x8000003C, // 60'
        0x80000000, // 0'
        0x00000000, // 0
        0x00000000, // 0
    ])?;

    let signing_key = SigningKey::from_bytes((&key_bytes).into())
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("BIP32 key error: {}", e)))?;
    let verifying_key = signing_key.verifying_key();
    let point = verifying_key.to_encoded_point(false);
    Ok(eth_address_from_pubkey_bytes(point.as_bytes()))
}

// ─── Bitcoin ─────────────────────────────────────────────────────────

#[cfg(feature = "derive-btc")]
fn derive_btc_from_privkey(secret: &str) -> Result<String> {
    use bitcoin::key::PrivateKey;
    use bitcoin::{Address, CompressedPublicKey, Network};
    use std::str::FromStr;

    let privkey = PrivateKey::from_wif(secret.trim())
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("Invalid WIF key: {}", e)))?;

    let secp = bitcoin::secp256k1::Secp256k1::new();
    let pubkey = privkey.public_key(&secp);
    let compressed = CompressedPublicKey::from_str(&pubkey.to_string())
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("Compressed key error: {}", e)))?;

    let address = Address::p2wpkh(&compressed, Network::Bitcoin);
    Ok(address.to_string())
}

#[cfg(feature = "derive-btc")]
fn derive_btc_from_seed(secret: &str) -> Result<String> {
    use bitcoin::{Address, CompressedPublicKey, Network};
    use std::str::FromStr;

    let mnemonic = bip39::Mnemonic::parse(secret.trim())
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("Invalid mnemonic: {}", e)))?;
    let seed = mnemonic.to_seed("");

    // BIP32 derivation: m/84'/0'/0'/0/0 for native segwit
    let key_bytes = bip32_derive_secp256k1(&seed, &[
        0x80000054, // 84'
        0x80000000, // 0'
        0x80000000, // 0'
        0x00000000, // 0
        0x00000000, // 0
    ])?;

    let secp = bitcoin::secp256k1::Secp256k1::new();
    let secret_key = bitcoin::secp256k1::SecretKey::from_slice(&key_bytes)
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("Invalid derived key: {}", e)))?;
    let pubkey = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret_key);
    let compressed = CompressedPublicKey::from_str(&pubkey.to_string())
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("Compressed key error: {}", e)))?;

    let address = Address::p2wpkh(&compressed, Network::Bitcoin);
    Ok(address.to_string())
}

// ─── Solana ──────────────────────────────────────────────────────────

#[cfg(feature = "derive-sol")]
fn derive_sol_from_privkey(secret: &str) -> Result<String> {
    use ed25519_dalek::SigningKey;

    let trimmed = secret.trim();

    // Try base58-encoded keypair first (Solana CLI format: 64 bytes)
    if let Ok(bytes) = bs58::decode(trimmed).into_vec() {
        if bytes.len() == 64 {
            let mut key_bytes = [0u8; 32];
            key_bytes.copy_from_slice(&bytes[..32]);
            let signing_key = SigningKey::from_bytes(&key_bytes);
            let pubkey = signing_key.verifying_key();
            return Ok(bs58::encode(pubkey.as_bytes()).into_string());
        } else if bytes.len() == 32 {
            let mut key_bytes = [0u8; 32];
            key_bytes.copy_from_slice(&bytes);
            let signing_key = SigningKey::from_bytes(&key_bytes);
            let pubkey = signing_key.verifying_key();
            return Ok(bs58::encode(pubkey.as_bytes()).into_string());
        }
    }

    // Try hex-encoded key
    if let Ok(bytes) = hex::decode(trimmed.strip_prefix("0x").unwrap_or(trimmed)) {
        if bytes.len() == 32 || bytes.len() == 64 {
            let mut key_bytes = [0u8; 32];
            key_bytes.copy_from_slice(&bytes[..32]);
            let signing_key = SigningKey::from_bytes(&key_bytes);
            let pubkey = signing_key.verifying_key();
            return Ok(bs58::encode(pubkey.as_bytes()).into_string());
        }
    }

    // Try JSON array format [1,2,3,...] (Solana CLI keypair file)
    if trimmed.starts_with('[') {
        if let Ok(bytes) = serde_json::from_str::<Vec<u8>>(trimmed) {
            if bytes.len() >= 32 {
                let mut key_bytes = [0u8; 32];
                key_bytes.copy_from_slice(&bytes[..32]);
                let signing_key = SigningKey::from_bytes(&key_bytes);
                let pubkey = signing_key.verifying_key();
                return Ok(bs58::encode(pubkey.as_bytes()).into_string());
            }
        }
    }

    Err(CryptoKeeperError::DerivationFailed(
        "Unrecognized Solana private key format. Expected base58, hex, or JSON array.".into(),
    ))
}

#[cfg(feature = "derive-sol")]
fn derive_sol_from_seed(secret: &str) -> Result<String> {
    use ed25519_dalek::SigningKey;

    let mnemonic = bip39::Mnemonic::parse(secret.trim())
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("Invalid mnemonic: {}", e)))?;
    let seed = mnemonic.to_seed("");

    // Solana uses the first 32 bytes of the seed for the default derivation
    // (Phantom and other wallets use m/44'/501'/0'/0' with BIP44-Ed25519,
    //  but the simplest approach used by solana-keygen is the seed directly)
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&seed[..32]);
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let pubkey = signing_key.verifying_key();
    Ok(bs58::encode(pubkey.as_bytes()).into_string())
}

// ─── BIP32 secp256k1 derivation ──────────────────────────────────────

/// Minimal BIP32 derivation for secp256k1 keys.
/// Uses HMAC-SHA512 as specified in BIP32.
#[cfg(any(feature = "derive-eth", feature = "derive-btc"))]
fn bip32_derive_secp256k1(seed: &[u8], path: &[u32]) -> Result<[u8; 32]> {
    use hmac::{Hmac, Mac};
    use sha2::Sha512;

    type HmacSha512 = Hmac<Sha512>;

    // Master key derivation
    let mut mac = HmacSha512::new_from_slice(b"Bitcoin seed")
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("HMAC error: {}", e)))?;
    mac.update(seed);
    let result = mac.finalize().into_bytes();

    let mut key = [0u8; 32];
    let mut chain_code = [0u8; 32];
    key.copy_from_slice(&result[..32]);
    chain_code.copy_from_slice(&result[32..]);

    // Child key derivation
    for &index in path {
        let mut mac = HmacSha512::new_from_slice(&chain_code)
            .map_err(|e| CryptoKeeperError::DerivationFailed(format!("HMAC error: {}", e)))?;

        if index & 0x80000000 != 0 {
            // Hardened child
            mac.update(&[0x00]);
            mac.update(&key);
        } else {
            // Normal child: use compressed public key
            let pubkey = secp256k1_pubkey_compressed(&key)?;
            mac.update(&pubkey);
        }
        mac.update(&index.to_be_bytes());

        let result = mac.finalize().into_bytes();

        // Parse IL as 256-bit integer and add to parent key (mod n)
        let il = &result[..32];
        key = secp256k1_add_scalars(&key, il)?;
        chain_code.copy_from_slice(&result[32..]);
    }

    Ok(key)
}

#[cfg(any(feature = "derive-eth", feature = "derive-btc"))]
fn secp256k1_pubkey_compressed(key: &[u8; 32]) -> Result<[u8; 33]> {
    use k256::ecdsa::SigningKey;
    let signing_key = SigningKey::from_bytes(key.into())
        .map_err(|e| CryptoKeeperError::DerivationFailed(format!("Invalid key: {}", e)))?;
    let verifying_key = signing_key.verifying_key();
    let point = verifying_key.to_encoded_point(true);
    let bytes = point.as_bytes();
    let mut result = [0u8; 33];
    result.copy_from_slice(bytes);
    Ok(result)
}

#[cfg(any(feature = "derive-eth", feature = "derive-btc"))]
fn secp256k1_add_scalars(parent: &[u8; 32], tweak: &[u8]) -> Result<[u8; 32]> {
    use k256::elliptic_curve::ops::Reduce;
    use k256::Scalar;

    // Convert bytes to Scalar (which is mod n automatically)
    let parent_uint = k256::U256::from_be_slice(parent);
    let tweak_uint = k256::U256::from_be_slice(tweak);

    let parent_scalar = <Scalar as Reduce<k256::U256>>::reduce(parent_uint);
    let tweak_scalar = <Scalar as Reduce<k256::U256>>::reduce(tweak_uint);

    // Scalar addition is mod n
    let sum = parent_scalar + tweak_scalar;

    let mut output = [0u8; 32];
    let bytes = sum.to_bytes();
    output.copy_from_slice(&bytes);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_combo_returns_none() {
        let result = derive_address("some-password", &SecretType::Password, "Ethereum").unwrap();
        assert!(result.is_none());
    }

    #[cfg(feature = "derive-eth")]
    #[test]
    fn eth_privkey_derivation() {
        // Known test vector: this private key produces a known address
        let privkey = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let result = derive_address(privkey, &SecretType::PrivateKey, "Ethereum").unwrap();
        assert!(result.is_some());
        let addr = result.unwrap();
        assert!(addr.starts_with("0x"));
        assert_eq!(addr.len(), 42); // 0x + 40 hex chars
        // This is the first Hardhat/Anvil account
        assert_eq!(addr, "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");
    }

    #[cfg(feature = "derive-sol")]
    #[test]
    fn sol_privkey_derivation() {
        // Test that SOL derivation produces a valid base58 address
        let key_bytes = [1u8; 32];
        let privkey = bs58::encode(&key_bytes).into_string();
        let result = derive_address(&privkey, &SecretType::PrivateKey, "Solana").unwrap();
        assert!(result.is_some());
        let addr = result.unwrap();
        // Verify it's valid base58
        assert!(bs58::decode(&addr).into_vec().is_ok());
    }

    #[cfg(feature = "derive-btc")]
    #[test]
    fn btc_privkey_derivation() {
        // Test with a known WIF private key (mainnet compressed)
        let wif = "KwDiBf89QgGbjEhKnhXJuH7LrciVrZi3qYjgd9M7rFU73sVHnoWn";
        let result = derive_address(wif, &SecretType::PrivateKey, "Bitcoin").unwrap();
        assert!(result.is_some());
        let addr = result.unwrap();
        // P2WPKH address starts with bc1
        assert!(addr.starts_with("bc1"));
    }
}
