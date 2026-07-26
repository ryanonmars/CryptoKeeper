use colored::Colorize;
use dialoguer::{Confirm, Input};
use rand::seq::SliceRandom;
use zeroize::Zeroize;
use zeroize::Zeroizing;

use crate::config::RecoveryConfig;
use crate::error::{Result, TermKeyError};
use crate::ui::borders::print_box;
use crate::ui::theme::heading;
use crate::vault::model::VaultData;
use crate::vault::session::VaultSession;
use crate::vault::storage;

pub fn run() -> Result<()> {
    if storage::vault_exists() {
        return Err(TermKeyError::VaultAlreadyExists(
            storage::vault_path().display().to_string(),
        ));
    }

    println!("{}", heading("Initializing new TermKey vault..."));
    println!();

    let password = Zeroizing::new(
        rpassword::prompt_password("Choose a master password: ").map_err(TermKeyError::Io)?,
    );

    if password.is_empty() {
        return Err(TermKeyError::EmptyPassword);
    }

    let confirm = Zeroizing::new(
        rpassword::prompt_password("Confirm master password: ").map_err(TermKeyError::Io)?,
    );

    if *password != *confirm {
        return Err(TermKeyError::PasswordMismatch);
    }

    storage::ensure_vault_dir()?;

    let vault = VaultData::new();
    eprintln!("Encrypting vault...");
    let session = VaultSession::create(vault, password, storage::vault_path())?;

    let setup_recovery = Confirm::new()
        .with_prompt("Set up a 24-word recovery phrase now?")
        .default(true)
        .interact()
        .map_err(|error| TermKeyError::Io(std::io::Error::other(error)))?;
    let recovery = if setup_recovery {
        let phrase = crate::crypto::recovery::generate_recovery_phrase()?;
        println!();
        println!("{}", heading("Recovery Phrase — shown once"));
        println!("Write these words down in order and store them safely:");
        println!();
        println!("{}", phrase.as_str());
        println!();
        confirm_recovery_phrase(&phrase)?;
        Some(RecoveryConfig::V2(
            crate::crypto::recovery::create_recovery_config(
                session.vault_id(),
                session.dek(),
                &phrase,
            )?,
        ))
    } else {
        None
    };
    crate::config::update_config(|config| {
        config.first_run_complete = true;
        config.vault_path = storage::vault_path().display().to_string();
        config.recovery = recovery;
        Ok(())
    })?;

    let lines = vec![
        format!("{}", "Vault created successfully!".green().bold()),
        format!(
            "Location: {}",
            storage::vault_path().display().to_string().cyan()
        ),
        String::new(),
        format!(
            "{}",
            "Use `termkey add` to store your first key or phrase.".dimmed()
        ),
    ];
    println!();
    print_box(Some("Vault Initialized"), &lines);

    Ok(())
}

fn confirm_recovery_phrase(phrase: &str) -> Result<()> {
    let words: Vec<&str> = phrase.split_whitespace().collect();
    let mut positions: Vec<usize> = (0..words.len()).collect();
    positions.shuffle(&mut rand::thread_rng());
    for position in positions.into_iter().take(3) {
        loop {
            let mut answer = Zeroizing::new(
                Input::<String>::new()
                    .with_prompt(format!("Enter recovery word #{}", position + 1))
                    .interact_text()
                    .map_err(|error| TermKeyError::Io(std::io::Error::other(error)))?,
            );
            if answer.trim() == words[position] {
                answer.zeroize();
                break;
            }
            answer.zeroize();
            eprintln!("Incorrect word. Try again.");
        }
    }
    Ok(())
}
