use chrono::Utc;
use colored::Colorize;
use dialoguer::{Input, Select};
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};
use crate::vault::model::{Entry, SecretType};
use crate::vault::storage;

pub fn run() -> Result<()> {
    let (mut vault, password) = storage::prompt_and_unlock()?;

    println!();
    println!("{}", "Add a new entry".bold());
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
    let type_options = &["Private Key", "Seed Phrase"];
    let type_idx = Select::new()
        .with_prompt("Secret type")
        .items(type_options)
        .default(0)
        .interact()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    let secret_type = match type_idx {
        0 => SecretType::PrivateKey,
        _ => SecretType::SeedPhrase,
    };

    // Secret (hidden input)
    let secret = Zeroizing::new(
        rpassword::prompt_password("Paste your secret (hidden): ")
            .map_err(CryptoKeeperError::Io)?,
    );

    if secret.is_empty() {
        return Err(CryptoKeeperError::Cancelled);
    }

    let confirm = Zeroizing::new(
        rpassword::prompt_password("Confirm secret (hidden): ")
            .map_err(CryptoKeeperError::Io)?,
    );

    if *secret != *confirm {
        return Err(CryptoKeeperError::PasswordMismatch);
    }

    // Network
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
        SecretType::SeedPhrase => None,
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
        notes: notes.trim().to_string(),
        created_at: now,
        updated_at: now,
    };

    vault.entries.push(entry);

    eprintln!("Saving vault...");
    storage::save_vault(&vault, password.as_bytes())?;

    println!();
    println!(
        "{} Entry '{}' stored successfully.",
        "✓".green().bold(),
        name.cyan()
    );

    Ok(())
}
