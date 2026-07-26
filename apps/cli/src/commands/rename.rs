use colored::Colorize;

use crate::error::{Result, TermKeyError};
use crate::ui::borders::print_success;
use crate::vault::model::VaultData;
use crate::vault::session::VaultSession;

pub fn run(old_name: &str, new_name: &str) -> Result<()> {
    let mut session = VaultSession::prompt_and_open()?.session;
    run_with_vault(&mut session.vault, old_name, new_name)?;
    eprintln!("Saving vault...");
    session.save()?;
    Ok(())
}

/// Core rename logic without prompt_and_unlock or save (for REPL mode).
pub fn run_with_vault(vault: &mut VaultData, old_name: &str, new_name: &str) -> Result<()> {
    let new_name = new_name.trim().to_string();

    let resolved_old = vault
        .resolve_entry_name(old_name)
        .ok_or_else(|| TermKeyError::EntryNotFound(old_name.to_string()))?;
    let secondary_password = {
        let entry = vault
            .find_entry(&resolved_old)
            .ok_or_else(|| TermKeyError::EntryNotFound(resolved_old.clone()))?;
        super::prompt_secondary_password(entry)?
    };
    rename_entry(
        vault,
        &resolved_old,
        &new_name,
        secondary_password
            .as_ref()
            .map(|password| password.as_str()),
    )?;

    print_success(&format!(
        "Renamed '{}' → '{}'",
        resolved_old.dimmed(),
        new_name.cyan()
    ));

    Ok(())
}

fn rename_entry(
    vault: &mut VaultData,
    old_name: &str,
    new_name: &str,
    secondary_password: Option<&str>,
) -> Result<()> {
    vault.rename_entry_authorized(old_name, new_name, secondary_password)
}

#[cfg(test)]
mod tests {
    use super::{rename_entry, run_with_vault};
    use crate::commands::test_support::protected_entry;
    use crate::error::TermKeyError;
    use crate::vault::model::{Entry, SecretType, VaultData};
    use chrono::{Duration, Utc};

    fn entry(name: &str) -> Entry {
        let now = Utc::now();
        Entry {
            name: name.to_string(),
            secret: "secret".to_string(),
            secret_type: SecretType::Password,
            network: String::new(),
            public_address: None,
            username: None,
            url: None,
            site_rules: Vec::new(),
            notes: String::new(),
            created_at: now - Duration::days(2),
            updated_at: now - Duration::days(1),
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        }
    }

    #[test]
    fn rename_rejects_whitespace_only_name_without_mutation() {
        let original = entry("Original");
        let mut vault = VaultData {
            entries: vec![original.clone()],
            version: 1,
            revision: 0,
        };

        let result = run_with_vault(&mut vault, "Original", "   ");

        assert!(matches!(result, Err(TermKeyError::InvalidEntry(_))));
        assert_eq!(vault.entries[0].name, original.name);
        assert_eq!(vault.entries[0].updated_at, original.updated_at);
    }

    #[test]
    fn rename_allows_case_only_change_and_updates_timestamp() {
        let original = entry("Original");
        let created_at = original.created_at;
        let updated_at = original.updated_at;
        let mut vault = VaultData {
            entries: vec![original],
            version: 1,
            revision: 0,
        };

        run_with_vault(&mut vault, "1", "ORIGINAL").unwrap();

        assert_eq!(vault.entries[0].name, "ORIGINAL");
        assert_eq!(vault.entries[0].created_at, created_at);
        assert!(vault.entries[0].updated_at > updated_at);
    }

    #[test]
    fn rename_collision_is_transactional() {
        let alpha = entry("Alpha");
        let bravo = entry("Bravo");
        let bravo_updated_at = bravo.updated_at;
        let mut vault = VaultData {
            entries: vec![alpha, bravo],
            version: 1,
            revision: 0,
        };

        let result = run_with_vault(&mut vault, "Bravo", " alpha ");

        assert!(matches!(result, Err(TermKeyError::EntryAlreadyExists(_))));
        assert_eq!(vault.entries[1].name, "Bravo");
        assert_eq!(vault.entries[1].updated_at, bravo_updated_at);
    }

    #[test]
    fn cli_rename_protected_entry_requires_correct_password_transactionally() {
        let protected = protected_entry("Protected", "secret", "view-pass");
        let mut vault = VaultData {
            entries: vec![protected],
            version: 1,
            revision: 0,
        };
        let original = serde_json::to_vec(&vault).unwrap();

        assert!(matches!(
            rename_entry(&mut vault, "Protected", "Renamed", None),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);
        assert!(matches!(
            rename_entry(&mut vault, "Protected", "Renamed", Some("wrong-pass")),
            Err(TermKeyError::SecondaryPasswordWrong)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);

        rename_entry(&mut vault, "Protected", "Renamed", Some("view-pass")).unwrap();
        assert_eq!(vault.entries[0].name, "Renamed");
    }
}
