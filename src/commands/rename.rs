use chrono::Utc;
use colored::Colorize;

use crate::error::{CryptoKeeperError, Result};
use crate::vault::storage;

pub fn run(old_name: &str, new_name: &str) -> Result<()> {
    let (mut vault, password) = storage::prompt_and_unlock()?;

    let new_name = new_name.trim().to_string();

    let resolved_old = vault
        .resolve_entry_name(old_name)
        .ok_or_else(|| CryptoKeeperError::EntryNotFound(old_name.to_string()))?;

    if vault.has_entry(&new_name) {
        return Err(CryptoKeeperError::EntryAlreadyExists(new_name));
    }

    let entry = vault.find_entry_mut_by_id(old_name).unwrap();
    entry.name = new_name.clone();
    entry.updated_at = Utc::now();

    eprintln!("Saving vault...");
    storage::save_vault(&vault, password.as_bytes())?;

    println!();
    println!(
        "{} Renamed '{}' → '{}'",
        "✓".green().bold(),
        resolved_old.dimmed(),
        new_name.cyan()
    );

    Ok(())
}
