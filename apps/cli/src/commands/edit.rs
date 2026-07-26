use chrono::Utc;
use colored::Colorize;
use dialoguer::{Input, Select};
use zeroize::Zeroizing;

use crate::error::{Result, TermKeyError};
use crate::ui::borders::print_success;
use crate::ui::theme::heading;
use crate::vault::model::{SecretType, VaultData};
use crate::vault::session::VaultSession;

pub fn run(name: &str) -> Result<()> {
    let mut session = VaultSession::prompt_and_open()?.session;
    run_with_vault(&mut session.vault, name)?;
    eprintln!("Saving vault...");
    session.save()?;
    Ok(())
}

/// Core edit logic without prompt_and_unlock or save (for REPL mode).
pub fn run_with_vault(vault: &mut VaultData, name: &str) -> Result<()> {
    let entry = vault
        .find_entry_by_id(name)
        .cloned()
        .ok_or_else(|| TermKeyError::EntryNotFound(name.to_string()))?;

    println!();
    println!(
        "  {}",
        heading("Edit entry (press Enter to keep current value)")
    );
    println!();

    // Name
    let new_name: String = Input::new()
        .with_prompt(format!("Name [{}]", entry.name))
        .default(entry.name.clone())
        .interact_text()
        .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

    let new_name = new_name.trim().to_string();

    // Check for duplicate if name changed
    // Secret type
    let current_type_idx = match entry.secret_type {
        SecretType::PrivateKey => 0,
        SecretType::SeedPhrase => 1,
        SecretType::Password => 2,
        SecretType::Other(_) => 3,
    };
    let type_options = &["Private Key", "Seed Phrase", "Password", "Other", "Exit"];
    let type_idx = Select::new()
        .with_prompt(format!("Secret type [{}]", entry.secret_type))
        .items(type_options)
        .default(current_type_idx)
        .interact()
        .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

    if type_idx == 4 {
        return Err(TermKeyError::Cancelled);
    }

    let new_type = match type_idx {
        0 => SecretType::PrivateKey,
        1 => SecretType::SeedPhrase,
        2 => SecretType::Password,
        _ => {
            let current_other = match &entry.secret_type {
                SecretType::Other(label) => label.clone(),
                _ => String::new(),
            };
            let custom_type: String = Input::new()
                .with_prompt(format!(
                    "Custom secret type [{}]",
                    if current_other.is_empty() {
                        "(none)"
                    } else {
                        &current_other
                    }
                ))
                .default(current_other)
                .interact_text()
                .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;
            let custom_type = custom_type.trim().to_string();
            if custom_type.is_empty() {
                return Err(TermKeyError::Cancelled);
            }
            SecretType::Other(custom_type)
        }
    };

    let old_type = entry.secret_type.clone();

    // Secret (optional change)
    println!("  {} {}", "Current secret:".dimmed(), "••••••••".dimmed());
    let change_secret = dialoguer::Confirm::new()
        .with_prompt("Change secret?")
        .default(false)
        .interact()
        .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

    let new_secret = if change_secret {
        let secret = Zeroizing::new(
            rpassword::prompt_password("New secret (hidden): ").map_err(TermKeyError::Io)?,
        );
        let confirm = Zeroizing::new(
            rpassword::prompt_password("Confirm secret (hidden): ").map_err(TermKeyError::Io)?,
        );
        if *secret != *confirm {
            return Err(TermKeyError::PasswordMismatch);
        }
        Some(secret)
    } else {
        None
    };

    // Type-specific fields
    let (new_network, new_public_address, new_username, new_url) = if new_type.is_password_type() {
        // Password type: prompt for username/url, clear network/address
        let current_uname = if old_type.is_password_type() {
            entry.username.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let current_url = if old_type.is_password_type() {
            entry.url.clone().unwrap_or_default()
        } else {
            String::new()
        };

        let uname: String = Input::new()
            .with_prompt(format!(
                "Username [{}]",
                if current_uname.is_empty() {
                    "(none)"
                } else {
                    &current_uname
                }
            ))
            .default(current_uname)
            .interact_text()
            .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;
        let uname = uname.trim().to_string();

        let url_val: String = Input::new()
            .with_prompt(format!(
                "URL [{}]",
                if current_url.is_empty() {
                    "(none)"
                } else {
                    &current_url
                }
            ))
            .default(current_url)
            .interact_text()
            .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;
        let url_val = url_val.trim().to_string();

        (
            String::new(),
            None,
            if uname.is_empty() { None } else { Some(uname) },
            if url_val.is_empty() {
                None
            } else {
                Some(url_val)
            },
        )
    } else if new_type.is_crypto_type() {
        // PrivateKey / SeedPhrase: prompt for network/address, clear username/url
        let default_network = if old_type.is_crypto_type() {
            entry.network.clone()
        } else {
            String::new()
        };

        let new_network: String = Input::new()
            .with_prompt(format!(
                "Network [{}]",
                if default_network.is_empty() {
                    "(none)"
                } else {
                    &default_network
                }
            ))
            .default(default_network)
            .interact_text()
            .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

        let new_public_address = if new_type == SecretType::PrivateKey {
            let current = if old_type.is_crypto_type() {
                entry.public_address.as_deref().unwrap_or("")
            } else {
                ""
            };
            let default_addr = if old_type.is_crypto_type() {
                entry.public_address.clone().unwrap_or_default()
            } else {
                String::new()
            };
            let addr: String = Input::new()
                .with_prompt(format!(
                    "Public address [{}]",
                    if current.is_empty() {
                        "(none)"
                    } else {
                        current
                    }
                ))
                .default(default_addr)
                .interact_text()
                .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;
            let trimmed = addr.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        } else {
            None
        };

        (
            new_network.trim().to_string(),
            new_public_address,
            None,
            None,
        )
    } else {
        (String::new(), None, None, None)
    };

    // Notes
    let new_notes: String = Input::new()
        .with_prompt(format!(
            "Notes [{}]",
            if entry.notes.is_empty() {
                "(empty)"
            } else {
                &entry.notes
            }
        ))
        .default(entry.notes.clone())
        .interact_text()
        .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

    // Apply changes
    let original_name = entry.name.clone();
    let secondary_password = super::prompt_secondary_password(&entry)?;
    let mut updated = entry;
    updated.name = new_name.clone();
    updated.secret_type = new_type;
    updated.network = new_network;
    updated.public_address = new_public_address;
    updated.username = new_username;
    updated.url = new_url;
    updated.notes = new_notes.trim().to_string();

    apply_entry_edit(
        vault,
        &original_name,
        updated,
        new_secret.as_ref().map(|secret| secret.as_str()),
        secondary_password
            .as_ref()
            .map(|password| password.as_str()),
    )?;

    print_success(&format!(
        "Entry '{}' updated successfully.",
        new_name.cyan()
    ));

    Ok(())
}

fn apply_entry_edit(
    vault: &mut VaultData,
    original_name: &str,
    mut updated: crate::vault::model::Entry,
    new_secret: Option<&str>,
    secondary_password: Option<&str>,
) -> Result<()> {
    if let Some(secret) = new_secret {
        updated.replace_secret(secret, secondary_password)?;
    } else {
        updated.updated_at = Utc::now();
    }
    vault.replace_entry_authorized(original_name, updated, secondary_password)
}

#[cfg(test)]
mod tests {
    use super::apply_entry_edit;
    use crate::commands::test_support::protected_entry;
    use crate::vault::model::VaultData;

    #[test]
    fn cli_edit_protected_entry_updates_encrypted_value() {
        let entry = protected_entry("Protected", "old-secret", "view-pass");
        let original_ciphertext = entry.encrypted_secret.clone();
        let mut vault = VaultData {
            entries: vec![entry.clone()],
            version: 1,
            revision: 0,
        };
        let mut updated = entry;
        updated.notes = "edited".to_string();

        apply_entry_edit(
            &mut vault,
            "Protected",
            updated,
            Some("new-secret"),
            Some("view-pass"),
        )
        .unwrap();

        let saved = vault.find_entry("Protected").unwrap();
        assert_ne!(saved.encrypted_secret, original_ciphertext);
        assert_eq!(
            &*saved.reveal_secret(Some("view-pass")).unwrap(),
            "new-secret"
        );
        assert_eq!(saved.secret, "[encrypted]");
    }
}
