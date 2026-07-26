use chrono::Utc;
use colored::Colorize;
use dialoguer::{Confirm, Input, Select};
use zeroize::Zeroizing;

use crate::crypto::passwords;
use crate::error::{Result, TermKeyError};
use crate::ui::borders::{print_box, print_success};
use crate::ui::theme::heading;
use crate::vault::model::{Entry, SecretType, VaultData};
use crate::vault::session::VaultSession;

pub fn run() -> Result<()> {
    let mut session = VaultSession::prompt_and_open()?.session;
    run_with_vault(&mut session.vault)?;
    eprintln!("Saving vault...");
    session.save()?;
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
        .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(TermKeyError::Cancelled);
    }

    if vault.has_entry(&name) {
        return Err(TermKeyError::EntryAlreadyExists(name));
    }

    // Secret type
    let type_options = &["Private Key", "Seed Phrase", "Password", "Other", "Exit"];
    let type_idx = Select::new()
        .with_prompt("Secret type")
        .items(type_options)
        .default(0)
        .interact()
        .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

    if type_idx == 4 {
        return Err(TermKeyError::Cancelled);
    }

    let secret_type = match type_idx {
        0 => SecretType::PrivateKey,
        1 => SecretType::SeedPhrase,
        2 => SecretType::Password,
        _ => {
            let custom_type: String = Input::new()
                .with_prompt("Custom secret type")
                .interact_text()
                .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;
            let custom_type = custom_type.trim().to_string();
            if custom_type.is_empty() {
                return Err(TermKeyError::Cancelled);
            }
            SecretType::Other(custom_type)
        }
    };

    let (secret, confirm) = prompt_secret(&secret_type)?;

    if *secret != *confirm {
        return Err(TermKeyError::SecretMismatch);
    }

    // Network & address (skip for Password type)
    let (network, public_address, username, url) = if secret_type.is_password_type() {
        // Password: prompt for optional username and URL
        let uname: String = Input::new()
            .with_prompt("Username (optional, press Enter to skip)")
            .default(String::new())
            .interact_text()
            .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;
        let uname = uname.trim().to_string();

        let url_input: String = Input::new()
            .with_prompt("URL (optional, press Enter to skip)")
            .default(String::new())
            .interact_text()
            .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;
        let url_input = url_input.trim().to_string();

        (
            String::new(),
            None,
            if uname.is_empty() { None } else { Some(uname) },
            if url_input.is_empty() {
                None
            } else {
                Some(url_input)
            },
        )
    } else if secret_type.is_crypto_type() {
        // PrivateKey / SeedPhrase: network + optional address
        let network_options = &["Ethereum", "Bitcoin", "Solana", "Other", "Exit"];
        let net_idx = Select::new()
            .with_prompt("Network")
            .items(network_options)
            .default(0)
            .interact()
            .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

        if net_idx == 4 {
            return Err(TermKeyError::Cancelled);
        }

        let network = if net_idx == 3 {
            let custom: String = Input::new()
                .with_prompt("Enter network name")
                .interact_text()
                .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;
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
                    .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;
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
    } else {
        (String::new(), None, None, None)
    };

    // Notes (optional)
    let notes: String = Input::new()
        .with_prompt("Notes (optional, press Enter to skip)")
        .default(String::new())
        .interact_text()
        .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

    let now = Utc::now();
    let entry = Entry {
        name: name.clone(),
        secret: secret.to_string(),
        secret_type,
        network,
        public_address,
        username,
        url,
        site_rules: Vec::new(),
        notes: notes.trim().to_string(),
        created_at: now,
        updated_at: now,
        has_secondary_password: false,
        entry_key_wrapped: None,
        entry_key_nonce: None,
        entry_key_salt: None,
        encrypted_secret: None,
        encrypted_secret_nonce: None,
    };

    vault.push_entry(entry)?;

    print_success(&format!("Entry '{}' stored successfully.", name.cyan()));

    Ok(())
}

fn prompt_secret(secret_type: &SecretType) -> Result<(Zeroizing<String>, Zeroizing<String>)> {
    if secret_type.is_password_type() {
        let method_options = &["Enter manually", "Generate strong password", "Exit"];
        let method_idx = Select::new()
            .with_prompt("Password input")
            .items(method_options)
            .default(1)
            .interact()
            .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

        match method_idx {
            0 => {}
            1 => {
                let generated = Zeroizing::new(passwords::generate_password());
                print_box(
                    Some("Generated Password"),
                    &[
                        format!("  {}", generated.yellow()),
                        "  Save this now. The password will be stored after confirmation."
                            .to_string(),
                    ],
                );

                let use_generated = Confirm::new()
                    .with_prompt("Use this generated password?")
                    .default(true)
                    .interact()
                    .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

                if !use_generated {
                    return Err(TermKeyError::Cancelled);
                }

                return Ok((
                    Zeroizing::new(generated.to_string()),
                    Zeroizing::new(generated.to_string()),
                ));
            }
            _ => return Err(TermKeyError::Cancelled),
        }
    }

    let secret_label = match secret_type {
        SecretType::Password => "Password",
        SecretType::Other(_) => "Secret",
        _ => "Paste your secret",
    };
    let secret = Zeroizing::new(
        rpassword::prompt_password(format!("{} (hidden): ", secret_label))
            .map_err(TermKeyError::Io)?,
    );

    if secret.is_empty() {
        return Err(TermKeyError::Cancelled);
    }

    let confirm_label = match secret_type {
        SecretType::Password => "Confirm password",
        SecretType::Other(_) => "Confirm secret",
        _ => "Confirm secret",
    };
    let confirm = Zeroizing::new(
        rpassword::prompt_password(format!("{} (hidden): ", confirm_label))
            .map_err(TermKeyError::Io)?,
    );

    Ok((secret, confirm))
}
