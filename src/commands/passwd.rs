use colored::Colorize;
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};
use crate::vault::storage;

pub fn run() -> Result<()> {
    let (vault, _old_password) = storage::prompt_and_unlock()?;

    println!();
    println!("{}", "Change master password".bold());
    println!();

    let new_password = Zeroizing::new(
        rpassword::prompt_password("New master password: ").map_err(CryptoKeeperError::Io)?,
    );

    if new_password.is_empty() {
        return Err(CryptoKeeperError::EmptyPassword);
    }

    let confirm = Zeroizing::new(
        rpassword::prompt_password("Confirm new password: ").map_err(CryptoKeeperError::Io)?,
    );

    if *new_password != *confirm {
        return Err(CryptoKeeperError::PasswordMismatch);
    }

    eprintln!("Re-encrypting vault with new password...");
    storage::save_vault(&vault, new_password.as_bytes())?;

    println!();
    println!(
        "{} Master password changed successfully.",
        "✓".green().bold()
    );

    Ok(())
}
