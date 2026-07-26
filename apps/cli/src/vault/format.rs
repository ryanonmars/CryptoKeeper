use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::crypto::{cipher, kdf};
use crate::error::{Result, TermKeyError};
use crate::vault::model::VaultData;

pub const FORMAT_VERSION_V3: u32 = 3;
pub const MAX_CIPHERTEXT_LEN: usize = 64 * 1024 * 1024;
pub const MIN_M_COST: u32 = 16 * 1024;
pub const MAX_M_COST: u32 = 1024 * 1024;
pub const MIN_T_COST: u32 = 1;
pub const MAX_T_COST: u32 = 10;
pub const MIN_P_COST: u32 = 1;
pub const MAX_P_COST: u32 = 16;

const MAGIC_LEN: usize = 4;
const VERSION_LEN: usize = 4;
const VAULT_ID_LEN: usize = 16;
const SALT_LEN: usize = 32;
const KDF_PARAM_LEN: usize = 4;
const NONCE_LEN: usize = 24;
const DEK_LEN: usize = 32;
const AUTH_TAG_LEN: usize = 16;
const WRAPPED_DEK_LEN: usize = DEK_LEN + AUTH_TAG_LEN;
const CIPHERTEXT_LEN_FIELD_LEN: usize = 4;

const V3_VAULT_ID_OFFSET: usize = MAGIC_LEN + VERSION_LEN;
const V3_SALT_OFFSET: usize = V3_VAULT_ID_OFFSET + VAULT_ID_LEN;
const V3_M_COST_OFFSET: usize = V3_SALT_OFFSET + SALT_LEN;
const V3_T_COST_OFFSET: usize = V3_M_COST_OFFSET + KDF_PARAM_LEN;
const V3_P_COST_OFFSET: usize = V3_T_COST_OFFSET + KDF_PARAM_LEN;
const V3_WRAP_NONCE_OFFSET: usize = V3_P_COST_OFFSET + KDF_PARAM_LEN;
const V3_WRAPPED_DEK_OFFSET: usize = V3_WRAP_NONCE_OFFSET + NONCE_LEN;
const V3_PAYLOAD_NONCE_OFFSET: usize = V3_WRAPPED_DEK_OFFSET + WRAPPED_DEK_LEN;
const V3_CIPHERTEXT_LEN_OFFSET: usize = V3_PAYLOAD_NONCE_OFFSET + NONCE_LEN;
const V3_HEADER_LEN: usize = V3_CIPHERTEXT_LEN_OFFSET + CIPHERTEXT_LEN_FIELD_LEN;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VaultId(pub [u8; VAULT_ID_LEN]);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V3Header {
    pub vault_id: VaultId,
    pub salt: [u8; SALT_LEN],
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
    pub wrap_nonce: [u8; NONCE_LEN],
    pub wrapped_dek: [u8; WRAPPED_DEK_LEN],
    pub payload_nonce: [u8; NONCE_LEN],
    pub ciphertext_len: u32,
}

pub struct OpenedV3 {
    pub vault: VaultData,
    pub vault_id: VaultId,
    pub dek: Zeroizing<[u8; DEK_LEN]>,
    pub header: V3Header,
}

pub fn validate_kdf_params(m: u32, t: u32, p: u32) -> Result<()> {
    if !(MIN_M_COST..=MAX_M_COST).contains(&m)
        || !(MIN_T_COST..=MAX_T_COST).contains(&t)
        || !(MIN_P_COST..=MAX_P_COST).contains(&p)
    {
        return Err(TermKeyError::KdfParametersOutOfPolicy);
    }
    Ok(())
}

pub fn parse_header(bytes: &[u8], expected_magic: &[u8; MAGIC_LEN]) -> Result<V3Header> {
    let magic = read_array::<MAGIC_LEN>(bytes, 0)?;
    if &magic != expected_magic {
        return Err(TermKeyError::InvalidVaultFormat);
    }

    let version = read_u32(bytes, MAGIC_LEN)?;
    if version != FORMAT_VERSION_V3 {
        return Err(TermKeyError::UnsupportedVaultVersion(version));
    }
    if bytes.len() < V3_HEADER_LEN {
        return Err(TermKeyError::InvalidVaultFormat);
    }

    let m_cost = read_u32(bytes, V3_M_COST_OFFSET)?;
    let t_cost = read_u32(bytes, V3_T_COST_OFFSET)?;
    let p_cost = read_u32(bytes, V3_P_COST_OFFSET)?;
    validate_kdf_params(m_cost, t_cost, p_cost)?;

    let ciphertext_len = read_u32(bytes, V3_CIPHERTEXT_LEN_OFFSET)?;
    let ciphertext_len_usize =
        usize::try_from(ciphertext_len).map_err(|_| TermKeyError::VaultTooLarge)?;
    if ciphertext_len_usize > MAX_CIPHERTEXT_LEN {
        return Err(TermKeyError::VaultTooLarge);
    }
    if ciphertext_len_usize < AUTH_TAG_LEN {
        return Err(TermKeyError::InvalidVaultFormat);
    }

    Ok(V3Header {
        vault_id: VaultId(read_array::<VAULT_ID_LEN>(bytes, V3_VAULT_ID_OFFSET)?),
        salt: read_array::<SALT_LEN>(bytes, V3_SALT_OFFSET)?,
        m_cost,
        t_cost,
        p_cost,
        wrap_nonce: read_array::<NONCE_LEN>(bytes, V3_WRAP_NONCE_OFFSET)?,
        wrapped_dek: read_array::<WRAPPED_DEK_LEN>(bytes, V3_WRAPPED_DEK_OFFSET)?,
        payload_nonce: read_array::<NONCE_LEN>(bytes, V3_PAYLOAD_NONCE_OFFSET)?,
        ciphertext_len,
    })
}

pub fn encode_v3(
    vault: &VaultData,
    password: &[u8],
    magic: &[u8; MAGIC_LEN],
    existing_dek: Option<&[u8; DEK_LEN]>,
    existing_vault_id: Option<VaultId>,
) -> Result<Zeroizing<Vec<u8>>> {
    vault.validate()?;
    validate_kdf_params(
        kdf::DEFAULT_M_COST,
        kdf::DEFAULT_T_COST,
        kdf::DEFAULT_P_COST,
    )?;

    let mut dek = Zeroizing::new([0u8; DEK_LEN]);
    match existing_dek {
        Some(existing) => dek.copy_from_slice(existing),
        None => rand::thread_rng().fill_bytes(dek.as_mut()),
    }

    let vault_id = existing_vault_id.unwrap_or_else(|| {
        let mut id = [0u8; VAULT_ID_LEN];
        rand::thread_rng().fill_bytes(&mut id);
        VaultId(id)
    });
    let salt = kdf::generate_salt();
    let wrap_nonce = cipher::generate_nonce();
    let payload_nonce = cipher::generate_nonce();
    let wrapping_key = kdf::derive_key(
        password,
        &salt,
        kdf::DEFAULT_M_COST,
        kdf::DEFAULT_T_COST,
        kdf::DEFAULT_P_COST,
    )?;

    let base_aad = payload_aad(magic, vault_id);
    let wrapping_aad = wrapping_aad(
        &base_aad,
        &salt,
        kdf::DEFAULT_M_COST,
        kdf::DEFAULT_T_COST,
        kdf::DEFAULT_P_COST,
    );
    let wrapped_dek =
        encrypt_authenticated(&wrapping_key, &wrap_nonce, dek.as_ref(), &wrapping_aad)?;
    let wrapped_dek: [u8; WRAPPED_DEK_LEN] = wrapped_dek
        .try_into()
        .map_err(|_| TermKeyError::InvalidVaultFormat)?;

    let plaintext = Zeroizing::new(serde_json::to_vec(vault)?);
    let expected_ciphertext_len = plaintext
        .len()
        .checked_add(AUTH_TAG_LEN)
        .ok_or(TermKeyError::VaultTooLarge)?;
    if expected_ciphertext_len > MAX_CIPHERTEXT_LEN {
        return Err(TermKeyError::VaultTooLarge);
    }
    let ciphertext = encrypt_authenticated(&dek, &payload_nonce, plaintext.as_ref(), &base_aad)?;
    if ciphertext.len() != expected_ciphertext_len {
        return Err(TermKeyError::Encryption(
            "Unexpected authenticated ciphertext length".to_string(),
        ));
    }
    let ciphertext_len =
        u32::try_from(ciphertext.len()).map_err(|_| TermKeyError::VaultTooLarge)?;

    let total_len = V3_HEADER_LEN
        .checked_add(ciphertext.len())
        .ok_or(TermKeyError::VaultTooLarge)?;
    let mut encoded = Zeroizing::new(Vec::with_capacity(total_len));
    encoded.extend_from_slice(magic);
    encoded.extend_from_slice(&FORMAT_VERSION_V3.to_le_bytes());
    encoded.extend_from_slice(&vault_id.0);
    encoded.extend_from_slice(&salt);
    encoded.extend_from_slice(&kdf::DEFAULT_M_COST.to_le_bytes());
    encoded.extend_from_slice(&kdf::DEFAULT_T_COST.to_le_bytes());
    encoded.extend_from_slice(&kdf::DEFAULT_P_COST.to_le_bytes());
    encoded.extend_from_slice(&wrap_nonce);
    encoded.extend_from_slice(&wrapped_dek);
    encoded.extend_from_slice(&payload_nonce);
    encoded.extend_from_slice(&ciphertext_len.to_le_bytes());
    encoded.extend_from_slice(&ciphertext);

    Ok(encoded)
}

pub fn decode_v3(password: &[u8], bytes: &[u8], magic: &[u8; MAGIC_LEN]) -> Result<OpenedV3> {
    let header = parse_header(bytes, magic)?;
    validate_encoded_len(bytes, &header)?;

    let wrapping_key = kdf::derive_key(
        password,
        &header.salt,
        header.m_cost,
        header.t_cost,
        header.p_cost,
    )?;
    let base_aad = payload_aad(magic, header.vault_id);
    let wrapping_aad = wrapping_aad(
        &base_aad,
        &header.salt,
        header.m_cost,
        header.t_cost,
        header.p_cost,
    );
    let unwrapped = decrypt_authenticated(
        &wrapping_key,
        &header.wrap_nonce,
        &header.wrapped_dek,
        &wrapping_aad,
    )?;
    if unwrapped.len() != DEK_LEN {
        return Err(TermKeyError::InvalidVaultFormat);
    }
    let mut dek = Zeroizing::new([0u8; DEK_LEN]);
    dek.copy_from_slice(&unwrapped);
    let (vault, vault_id) = decode_v3_with_dek(&dek, bytes, magic)?;

    Ok(OpenedV3 {
        vault,
        vault_id,
        dek,
        header,
    })
}

pub fn decode_v3_with_dek(
    dek: &[u8; DEK_LEN],
    bytes: &[u8],
    magic: &[u8; MAGIC_LEN],
) -> Result<(VaultData, VaultId)> {
    let header = parse_header(bytes, magic)?;
    let ciphertext = ciphertext(bytes, &header)?;
    let aad = payload_aad(magic, header.vault_id);
    let plaintext = decrypt_authenticated(dek, &header.payload_nonce, ciphertext, &aad)?;
    let vault: VaultData =
        serde_json::from_slice(&plaintext).map_err(|_| TermKeyError::InvalidVaultFormat)?;
    vault
        .validate()
        .map_err(|_| TermKeyError::InvalidVaultFormat)?;
    Ok((vault, header.vault_id))
}

#[cfg(test)]
pub(crate) fn encode_v3_unchecked_for_test(
    vault: &VaultData,
    password: &[u8],
    magic: &[u8; MAGIC_LEN],
    existing_dek: Option<&[u8; DEK_LEN]>,
    existing_vault_id: Option<VaultId>,
) -> Zeroizing<Vec<u8>> {
    let valid = VaultData::new();
    let mut encoded = encode_v3(&valid, password, magic, existing_dek, existing_vault_id).unwrap();
    let opened = decode_v3(password, &encoded, magic).unwrap();
    let plaintext = Zeroizing::new(serde_json::to_vec(vault).unwrap());
    let aad = payload_aad(magic, opened.vault_id);
    let ciphertext =
        encrypt_authenticated(&opened.dek, &opened.header.payload_nonce, &plaintext, &aad).unwrap();
    let ciphertext_len = u32::try_from(ciphertext.len()).unwrap();

    encoded.truncate(V3_HEADER_LEN);
    encoded[V3_CIPHERTEXT_LEN_OFFSET..V3_HEADER_LEN].copy_from_slice(&ciphertext_len.to_le_bytes());
    encoded.extend_from_slice(&ciphertext);
    encoded
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    let end = offset
        .checked_add(N)
        .ok_or(TermKeyError::InvalidVaultFormat)?;
    bytes
        .get(offset..end)
        .ok_or(TermKeyError::InvalidVaultFormat)?
        .try_into()
        .map_err(|_| TermKeyError::InvalidVaultFormat)
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(read_array::<KDF_PARAM_LEN>(
        bytes, offset,
    )?))
}

fn payload_aad(magic: &[u8; MAGIC_LEN], vault_id: VaultId) -> Vec<u8> {
    let mut aad = Vec::with_capacity(MAGIC_LEN + VERSION_LEN + VAULT_ID_LEN);
    aad.extend_from_slice(magic);
    aad.extend_from_slice(&FORMAT_VERSION_V3.to_le_bytes());
    aad.extend_from_slice(&vault_id.0);
    aad
}

fn wrapping_aad(
    base_aad: &[u8],
    salt: &[u8; SALT_LEN],
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
) -> Vec<u8> {
    let mut aad = Vec::with_capacity(base_aad.len() + SALT_LEN + 3 * KDF_PARAM_LEN);
    aad.extend_from_slice(base_aad);
    aad.extend_from_slice(salt);
    aad.extend_from_slice(&m_cost.to_le_bytes());
    aad.extend_from_slice(&t_cost.to_le_bytes());
    aad.extend_from_slice(&p_cost.to_le_bytes());
    aad
}

fn encrypt_authenticated(
    key: &[u8; DEK_LEN],
    nonce: &[u8; NONCE_LEN],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>> {
    XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| TermKeyError::Encryption("Authenticated encryption failed".to_string()))
}

fn decrypt_authenticated(
    key: &[u8; DEK_LEN],
    nonce: &[u8; NONCE_LEN],
    encrypted: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: encrypted,
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| TermKeyError::DecryptionFailed)
}

fn validate_encoded_len(bytes: &[u8], header: &V3Header) -> Result<()> {
    let ciphertext_len =
        usize::try_from(header.ciphertext_len).map_err(|_| TermKeyError::VaultTooLarge)?;
    let expected_len = V3_HEADER_LEN
        .checked_add(ciphertext_len)
        .ok_or(TermKeyError::VaultTooLarge)?;
    if bytes.len() != expected_len {
        return Err(TermKeyError::InvalidVaultFormat);
    }
    Ok(())
}

fn ciphertext<'a>(bytes: &'a [u8], header: &V3Header) -> Result<&'a [u8]> {
    validate_encoded_len(bytes, header)?;
    bytes
        .get(V3_HEADER_LEN..)
        .ok_or(TermKeyError::InvalidVaultFormat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::kdf;
    use crate::vault::model::{Entry, SecretType, VaultData, VaultHeader};
    use chrono::Utc;

    fn test_vault() -> VaultData {
        let mut vault = VaultData::new();
        vault.entries.push(Entry {
            name: "Encrypted entry name".to_string(),
            secret: "top-secret".to_string(),
            secret_type: SecretType::Password,
            network: String::new(),
            public_address: None,
            username: Some("encrypted-user".to_string()),
            url: Some("https://encrypted.example".to_string()),
            site_rules: Vec::new(),
            notes: "encrypted notes".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        });
        vault
    }

    #[test]
    fn rejects_unknown_version() {
        let mut bytes = Vec::from(VaultHeader::MAGIC.as_slice());
        bytes.extend_from_slice(&99u32.to_le_bytes());

        assert!(parse_header(&bytes, VaultHeader::MAGIC).is_err());
    }

    #[test]
    fn rejects_truncated_v3_header() {
        let mut bytes = Vec::from(VaultHeader::MAGIC.as_slice());
        bytes.extend_from_slice(&FORMAT_VERSION_V3.to_le_bytes());

        assert!(parse_header(&bytes, VaultHeader::MAGIC).is_err());
    }

    #[test]
    fn rejects_excessive_ciphertext_length() {
        let mut bytes = vec![0u8; V3_HEADER_LEN];
        bytes[..4].copy_from_slice(VaultHeader::MAGIC);
        bytes[4..8].copy_from_slice(&FORMAT_VERSION_V3.to_le_bytes());
        bytes[V3_M_COST_OFFSET..V3_T_COST_OFFSET].copy_from_slice(&MIN_M_COST.to_le_bytes());
        bytes[V3_T_COST_OFFSET..V3_P_COST_OFFSET].copy_from_slice(&MIN_T_COST.to_le_bytes());
        bytes[V3_P_COST_OFFSET..V3_WRAP_NONCE_OFFSET].copy_from_slice(&MIN_P_COST.to_le_bytes());
        bytes[V3_CIPHERTEXT_LEN_OFFSET..V3_HEADER_LEN]
            .copy_from_slice(&((MAX_CIPHERTEXT_LEN as u32) + 1).to_le_bytes());

        assert!(parse_header(&bytes, VaultHeader::MAGIC).is_err());
    }

    #[test]
    fn rejects_out_of_policy_kdf_parameters() {
        assert!(validate_kdf_params(MIN_M_COST - 1, MIN_T_COST, MIN_P_COST).is_err());
        assert!(validate_kdf_params(MAX_M_COST + 1, MIN_T_COST, MIN_P_COST).is_err());
        assert!(validate_kdf_params(MIN_M_COST, MIN_T_COST - 1, MIN_P_COST).is_err());
        assert!(validate_kdf_params(MIN_M_COST, MAX_T_COST + 1, MIN_P_COST).is_err());
        assert!(validate_kdf_params(MIN_M_COST, MIN_T_COST, MIN_P_COST - 1).is_err());
        assert!(validate_kdf_params(MIN_M_COST, MIN_T_COST, MAX_P_COST + 1).is_err());
    }

    #[test]
    fn v3_round_trip_keeps_metadata_encrypted() {
        let vault = test_vault();
        let password = b"test-password";
        let encoded = encode_v3(&vault, password, VaultHeader::MAGIC, None, None).unwrap();

        for metadata in [
            "Encrypted entry name",
            "encrypted-user",
            "https://encrypted.example",
            "encrypted notes",
        ] {
            assert!(
                !encoded
                    .windows(metadata.len())
                    .any(|window| window == metadata.as_bytes()),
                "{metadata:?} leaked into the serialized vault"
            );
        }

        let opened = decode_v3(password, &encoded, VaultHeader::MAGIC).unwrap();
        assert_eq!(opened.vault.entries.len(), 1);
        assert_eq!(opened.vault.entries[0].name, "Encrypted entry name");
        assert_eq!(
            opened.vault.entries[0].username.as_deref(),
            Some("encrypted-user")
        );
        assert_eq!(
            opened.vault.entries[0].url.as_deref(),
            Some("https://encrypted.example")
        );
        assert_eq!(opened.vault.entries[0].notes, "encrypted notes");
        assert_eq!(opened.header.m_cost, kdf::DEFAULT_M_COST);
    }

    #[test]
    fn v3_tampered_header_fails_authentication() {
        let vault = test_vault();
        let password = b"test-password";
        let mut encoded = encode_v3(&vault, password, VaultHeader::MAGIC, None, None).unwrap();

        encoded[V3_VAULT_ID_OFFSET] ^= 1;

        assert!(decode_v3(password, &encoded, VaultHeader::MAGIC).is_err());
    }

    #[test]
    fn v3_decode_rejects_malformed_protected_entry_as_corruption() {
        let mut vault = test_vault();
        vault.entries[0].has_secondary_password = true;
        let encoded =
            encode_v3_unchecked_for_test(&vault, b"password", VaultHeader::MAGIC, None, None);

        assert!(matches!(
            decode_v3(b"password", &encoded, VaultHeader::MAGIC),
            Err(TermKeyError::InvalidVaultFormat)
        ));
    }

    #[test]
    fn v3_decode_rejects_case_insensitive_duplicate_names_as_corruption() {
        let mut vault = test_vault();
        let mut duplicate = vault.entries[0].clone();
        duplicate.name = "encrypted entry name".to_string();
        vault.entries.push(duplicate);
        let encoded =
            encode_v3_unchecked_for_test(&vault, b"password", VaultHeader::MAGIC, None, None);

        assert!(matches!(
            decode_v3(b"password", &encoded, VaultHeader::MAGIC),
            Err(TermKeyError::InvalidVaultFormat)
        ));
    }

    #[test]
    fn v3_encode_rejects_invalid_vault_data() {
        let mut vault = test_vault();
        vault.entries[0].name = " untrimmed ".to_string();

        assert!(matches!(
            encode_v3(&vault, b"password", VaultHeader::MAGIC, None, None),
            Err(TermKeyError::InvalidEntry(_))
        ));
    }
}
