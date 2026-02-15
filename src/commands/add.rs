use chrono::Utc;
use colored::Colorize;
use dialoguer::{Input, Select};
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_success;
use crate::ui::theme::heading;
use crate::vault::model::{Entry, SecretType, VaultData};
use crate::vault::storage;

pub fn run() -> Result<()> {
    let (mut vault, password) = storage::prompt_and_unlock()?;
    run_with_vault(&mut vault)?;
    eprintln!("Saving vault...");
    storage::save_vault(&vault, password.as_bytes())?;
    Ok(())
}

/// Core add logic without prompt_and_unlock or save (for REPL mode).
pub fn run_with_vault(vault: &mut VaultData) -> Result<()> {
    println!();
    println!("  {}", heading("Add a new entry"));
    println!();

    // Name
    let name: String = Input::new()
        .with_prompt("Entry name (e.g. \"MetaMask Main\")")
        .interact_text()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CryptoKeeperError::Cancelled);
    }

    if vault.has_entry(&name) {
        return Err(CryptoKeeperError::EntryAlreadyExists(name));
    }

    // Secret type
    let type_options = &["Private Key", "Seed Phrase", "Password"];
    let type_idx = Select::new()
        .with_prompt("Secret type")
        .items(type_options)
        .default(0)
        .interact()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    let secret_type = match type_idx {
        0 => SecretType::PrivateKey,
        1 => SecretType::SeedPhrase,
        _ => SecretType::Password,
    };

    // Secret (hidden input)
    let secret_label = match secret_type {
        SecretType::Password => "Password",
        _ => "Paste your secret",
    };
    let secret = Zeroizing::new(
        rpassword::prompt_password(format!("{} (hidden): ", secret_label))
            .map_err(CryptoKeeperError::Io)?,
    );

    if secret.is_empty() {
        return Err(CryptoKeeperError::Cancelled);
    }

    let confirm_label = match secret_type {
        SecretType::Password => "Confirm password",
        _ => "Confirm secret",
    };
    let confirm = Zeroizing::new(
        rpassword::prompt_password(format!("{} (hidden): ", confirm_label))
            .map_err(CryptoKeeperError::Io)?,
    );

    if *secret != *confirm {
        return Err(CryptoKeeperError::PasswordMismatch);
    }

    // Network & address (skip for Password type)
    let (network, public_address, username, url) = if secret_type == SecretType::Password {
        // Password: prompt for optional username and URL
        let uname: String = Input::new()
            .with_prompt("Username (optional, press Enter to skip)")
            .default(String::new())
            .interact_text()
            .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        let uname = uname.trim().to_string();

        let url_input: String = Input::new()
            .with_prompt("URL (optional, press Enter to skip)")
            .default(String::new())
            .interact_text()
            .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        let url_input = url_input.trim().to_string();

        (
            String::new(),
            None,
            if uname.is_empty() { None } else { Some(uname) },
            if url_input.is_empty() { None } else { Some(url_input) },
        )
    } else {
        // PrivateKey / SeedPhrase: network + optional address
        let network_options = &["Ethereum", "Bitcoin", "Solana", "Other"];
        let net_idx = Select::new()
            .with_prompt("Network")
            .items(network_options)
            .default(0)
            .interact()
            .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let network = if net_idx == 3 {
            let custom: String = Input::new()
                .with_prompt("Enter network name")
                .interact_text()
                .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
            custom.trim().to_string()
        } else {
            network_options[net_idx].to_string()
        };

        let public_address = match secret_type {
            SecretType::PrivateKey => {
                let addr: String = Input::new()
                    .with_prompt("Public address (optional, press Enter to skip)")
                    .default(String::new())
                    .interact_text()
                    .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
                let trimmed = addr.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            }
            _ => None,
        };

        (network, public_address, None, None)
    };

    // Notes (optional)
    let notes: String = Input::new()
        .with_prompt("Notes (optional, press Enter to skip)")
        .default(String::new())
        .interact_text()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    let now = Utc::now();
    let entry = Entry {
        name: name.clone(),
        secret: secret.to_string(),
        secret_type,
        network,
        public_address,
        username,
        url,
        notes: notes.trim().to_string(),
        created_at: now,
        updated_at: now,
    };

    vault.entries.push(entry);

    print_success(&format!(
        "Entry '{}' stored successfully.",
        name.cyan()
    ));

    Ok(())
}
