use colored::Colorize;
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};
use crate::vault::model::VaultData;
use crate::vault::storage;

pub fn run() -> Result<()> {
    if storage::vault_exists() {
        return Err(CryptoKeeperError::VaultAlreadyExists(
            storage::vault_path().display().to_string(),
        ));
    }

    println!("{}", "Initializing new CryptoKeeper vault...".bold());
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

    println!();
    println!("{}", "Vault created successfully!".green().bold());
    println!(
        "Location: {}",
        storage::vault_path().display().to_string().cyan()
    );
    println!();
    println!(
        "{}",
        "Use `keeper add` to store your first key or phrase.".dimmed()
    );

    Ok(())
}
