use chrono::Utc;
use colored::Colorize;
use dialoguer::{Input, Select};
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_success;
use crate::ui::theme::heading;
use crate::vault::model::{SecretType, VaultData};
use crate::vault::storage;

pub fn run(name: &str) -> Result<()> {
    let (mut vault, password) = storage::prompt_and_unlock()?;
    run_with_vault(&mut vault, name)?;
    eprintln!("Saving vault...");
    storage::save_vault(&vault, password.as_bytes())?;
    Ok(())
}

/// Core edit logic without prompt_and_unlock or save (for REPL mode).
pub fn run_with_vault(vault: &mut VaultData, name: &str) -> Result<()> {
    let entry = vault
        .find_entry_mut_by_id(name)
        .ok_or_else(|| CryptoKeeperError::EntryNotFound(name.to_string()))?;

    println!();
    println!("  {}", heading("Edit entry (press Enter to keep current value)"));
    println!();

    // Name
    let new_name: String = Input::new()
        .with_prompt(format!("Name [{}]", entry.name))
        .default(entry.name.clone())
        .interact_text()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    let new_name = new_name.trim().to_string();

    // Check for duplicate if name changed
    if new_name.to_lowercase() != entry.name.to_lowercase() && vault.has_entry(&new_name) {
        return Err(CryptoKeeperError::EntryAlreadyExists(new_name));
    }

    // Re-fetch the entry after borrow checker satisfaction
    let entry = vault.find_entry_mut_by_id(name).unwrap();

    // Secret type
    let current_type_idx = match entry.secret_type {
        SecretType::PrivateKey => 0,
        SecretType::SeedPhrase => 1,
        SecretType::Password => 2,
    };
    let type_options = &["Private Key", "Seed Phrase", "Password"];
    let type_idx = Select::new()
        .with_prompt(format!("Secret type [{}]", entry.secret_type))
        .items(type_options)
        .default(current_type_idx)
        .interact()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    let new_type = match type_idx {
        0 => SecretType::PrivateKey,
        1 => SecretType::SeedPhrase,
        _ => SecretType::Password,
    };

    let old_type = entry.secret_type.clone();

    // Secret (optional change)
    println!(
        "  {} {}",
        "Current secret:".dimmed(),
        "••••••••".dimmed()
    );
    let change_secret = dialoguer::Confirm::new()
        .with_prompt("Change secret?")
        .default(false)
        .interact()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    let new_secret = if change_secret {
        let secret = Zeroizing::new(
            rpassword::prompt_password("New secret (hidden): ")
                .map_err(CryptoKeeperError::Io)?,
        );
        let confirm = Zeroizing::new(
            rpassword::prompt_password("Confirm secret (hidden): ")
                .map_err(CryptoKeeperError::Io)?,
        );
        if *secret != *confirm {
            return Err(CryptoKeeperError::PasswordMismatch);
        }
        Some(secret)
    } else {
        None
    };

    // Type-specific fields
    let (new_network, new_public_address, new_username, new_url) = if new_type == SecretType::Password {
        // Password type: prompt for username/url, clear network/address
        let current_uname = if old_type == SecretType::Password {
            entry.username.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let current_url = if old_type == SecretType::Password {
            entry.url.clone().unwrap_or_default()
        } else {
            String::new()
        };

        let uname: String = Input::new()
            .with_prompt(format!("Username [{}]", if current_uname.is_empty() { "(none)" } else { &current_uname }))
            .default(current_uname)
            .interact_text()
            .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        let uname = uname.trim().to_string();

        let url_val: String = Input::new()
            .with_prompt(format!("URL [{}]", if current_url.is_empty() { "(none)" } else { &current_url }))
            .default(current_url)
            .interact_text()
            .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        let url_val = url_val.trim().to_string();

        (
            String::new(),
            None,
            if uname.is_empty() { None } else { Some(uname) },
            if url_val.is_empty() { None } else { Some(url_val) },
        )
    } else {
        // PrivateKey / SeedPhrase: prompt for network/address, clear username/url
        let default_network = if old_type == SecretType::Password {
            String::new()
        } else {
            entry.network.clone()
        };

        let new_network: String = Input::new()
            .with_prompt(format!("Network [{}]", if default_network.is_empty() { "(none)" } else { &default_network }))
            .default(default_network)
            .interact_text()
            .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let new_public_address = if new_type == SecretType::PrivateKey {
            let current = if old_type == SecretType::Password {
                ""
            } else {
                entry.public_address.as_deref().unwrap_or("")
            };
            let default_addr = if old_type == SecretType::Password {
                String::new()
            } else {
                entry.public_address.clone().unwrap_or_default()
            };
            let addr: String = Input::new()
                .with_prompt(format!("Public address [{}]", if current.is_empty() { "(none)" } else { current }))
                .default(default_addr)
                .interact_text()
                .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
            let trimmed = addr.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        } else {
            None
        };

        (new_network.trim().to_string(), new_public_address, None, None)
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
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    // Apply changes
    entry.name = new_name.clone();
    entry.secret_type = new_type;
    if let Some(secret) = new_secret {
        entry.secret = secret.to_string();
    }
    entry.network = new_network;
    entry.public_address = new_public_address;
    entry.username = new_username;
    entry.url = new_url;
    entry.notes = new_notes.trim().to_string();
    entry.updated_at = Utc::now();

    print_success(&format!(
        "Entry '{}' updated successfully.",
        new_name.cyan()
    ));

    Ok(())
}
