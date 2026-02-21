use colored::Colorize;
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_box;
use crate::ui::theme::heading;
use crate::vault::model::VaultData;
use crate::vault::storage;

pub fn run() -> Result<()> {
    if storage::vault_exists() {
        return Err(CryptoKeeperError::VaultAlreadyExists(
            storage::vault_path().display().to_string(),
        ));
    }

    println!("{}", heading("Initializing new CryptoKeeper vault..."));
    println!();

    let password = Zeroizing::new(
        rpassword::prompt_password("Choose a master password: ")
            .map_err(CryptoKeeperError::Io)?,
    );

    if password.is_empty() {
        return Err(CryptoKeeperError::EmptyPassword);
    }

    let confirm = Zeroizing::new(
        rpassword::prompt_password("Confirm master password: ")
            .map_err(CryptoKeeperError::Io)?,
    );

    if *password != *confirm {
        return Err(CryptoKeeperError::PasswordMismatch);
    }

    storage::ensure_vault_dir()?;

    let vault = VaultData::new();
    eprintln!("Encrypting vault...");
    storage::save_vault(&vault, password.as_bytes())?;

    let lines = vec![
        format!("{}", "Vault created successfully!".green().bold()),
        format!(
            "Location: {}",
            storage::vault_path().display().to_string().cyan()
        ),
        String::new(),
        format!(
            "{}",
            "Use `cryptokeeper add` to store your first key or phrase.".dimmed()
        ),
    ];
    println!();
    print_box(Some("Vault Initialized"), &lines);

    Ok(())
}
