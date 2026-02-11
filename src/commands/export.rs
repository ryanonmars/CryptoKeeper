use std::path::Path;

use colored::Colorize;
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};
use crate::vault::storage;

pub fn run(file: &str) -> Result<()> {
    let (vault, _password) = storage::prompt_and_unlock()?;

    println!();
    println!("{}", "Export encrypted backup".bold());
    println!(
        "{}",
        "Choose a password for this backup (can differ from master password).".dimmed()
    );
    println!();

    let export_password = Zeroizing::new(
        rpassword::prompt_password("Backup password: ").map_err(CryptoKeeperError::Io)?,
    );

    if export_password.is_empty() {
        return Err(CryptoKeeperError::EmptyPassword);
    }

    let confirm = Zeroizing::new(
        rpassword::prompt_password("Confirm backup password: ").map_err(CryptoKeeperError::Io)?,
    );

    if *export_password != *confirm {
        return Err(CryptoKeeperError::PasswordMismatch);
    }

    let path = Path::new(file);
    eprintln!("Encrypting backup...");
    storage::write_backup(&vault, export_password.as_bytes(), path)?;

    println!();
    println!(
        "{} Backup exported to '{}'",
        "✓".green().bold(),
        file.cyan()
    );
    println!(
        "  {} entries exported.",
        vault.entries.len().to_string().bold()
    );

    Ok(())
}
