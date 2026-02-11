use colored::Colorize;
use dialoguer::Confirm;

use crate::error::{CryptoKeeperError, Result};
use crate::vault::storage;

pub fn run(name: &str) -> Result<()> {
    let (mut vault, password) = storage::prompt_and_unlock()?;

    if !vault.has_entry(name) {
        return Err(CryptoKeeperError::EntryNotFound(name.to_string()));
    }

    let confirm = Confirm::new()
        .with_prompt(format!(
            "Are you sure you want to delete '{}'? This cannot be undone",
            name
        ))
        .default(false)
        .interact()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    if !confirm {
        return Err(CryptoKeeperError::Cancelled);
    }

    vault.remove_entry(name);

    eprintln!("Saving vault...");
    storage::save_vault(&vault, password.as_bytes())?;

    println!();
    println!(
        "{} Entry '{}' deleted.",
        "✓".green().bold(),
        name.cyan()
    );

    Ok(())
}
