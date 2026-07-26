pub mod add;
pub mod browser;
pub mod config_cmd;
pub mod copy;
pub mod delete;
pub mod derive;
pub mod edit;
pub mod export;
pub mod import;
pub mod init;
pub mod list;
pub mod passwd;
pub mod recover;
pub mod rename;
pub mod search;
pub mod update_cmd;
pub mod view;

use zeroize::Zeroizing;

use crate::error::{Result, TermKeyError};
use crate::vault::model::Entry;

pub fn prompt_secondary_password(entry: &Entry) -> Result<Option<Zeroizing<String>>> {
    entry.validate()?;
    if !entry.has_secondary_password {
        return Ok(None);
    }

    let password = rpassword::prompt_password("Secondary password: ").map_err(TermKeyError::Io)?;
    Ok(Some(Zeroizing::new(password)))
}

#[cfg(test)]
pub(crate) mod test_support {
    use chrono::Utc;

    use crate::crypto::entry_key;
    use crate::vault::model::{Entry, SecretType};

    pub fn protected_entry(name: &str, secret: &str, secondary_password: &str) -> Entry {
        let key = entry_key::generate_entry_key();
        let (encrypted_secret, encrypted_secret_nonce) =
            entry_key::encrypt_secret(&key, secret).unwrap();
        let (entry_key_wrapped, entry_key_nonce, entry_key_salt) =
            entry_key::wrap_entry_key(&key, secondary_password).unwrap();

        Entry {
            name: name.to_string(),
            secret: "[encrypted]".to_string(),
            secret_type: SecretType::Password,
            network: String::new(),
            public_address: None,
            username: None,
            url: None,
            site_rules: Vec::new(),
            notes: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            has_secondary_password: true,
            entry_key_wrapped: Some(entry_key_wrapped),
            entry_key_nonce: Some(entry_key_nonce),
            entry_key_salt: Some(entry_key_salt),
            encrypted_secret: Some(encrypted_secret),
            encrypted_secret_nonce: Some(encrypted_secret_nonce),
        }
    }
}

#[cfg(test)]
mod prompt_tests {
    use super::{prompt_secondary_password, test_support::protected_entry};
    use crate::error::TermKeyError;

    #[test]
    fn secondary_password_prompt_router_skips_unprotected_entries() {
        let mut entry = protected_entry("Unprotected", "secret", "unused");
        entry.secret = "secret".to_string();
        entry.has_secondary_password = false;
        entry.entry_key_wrapped = None;
        entry.entry_key_nonce = None;
        entry.entry_key_salt = None;
        entry.encrypted_secret = None;
        entry.encrypted_secret_nonce = None;

        let routed_password = prompt_secondary_password(&entry).unwrap();

        assert!(routed_password.is_none());
    }

    #[test]
    fn secondary_password_prompt_router_rejects_malformed_protection_before_prompting() {
        let mut entry = protected_entry("Malformed", "secret", "view-pass");
        entry.encrypted_secret_nonce = None;

        let result = prompt_secondary_password(&entry);

        assert!(matches!(result, Err(TermKeyError::InvalidEntry(_))));
    }
}
