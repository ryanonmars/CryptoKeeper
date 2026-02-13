use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_success;
use crate::ui::theme::heading;
use crate::vault::storage;

pub fn run() -> Result<()> {
    let (vault, _old_password) = storage::prompt_and_unlock()?;

    println!();
    println!("  {}", heading("Change master password"));
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

    print_success("Master password changed successfully.");

    Ok(())
}
