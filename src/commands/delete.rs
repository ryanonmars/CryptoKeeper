use colored::Colorize;
use dialoguer::Confirm;

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_success;
use crate::vault::model::VaultData;
use crate::vault::storage;

pub fn run(name: &str) -> Result<()> {
    let (mut vault, password) = storage::prompt_and_unlock()?;
    run_with_vault(&mut vault, name)?;
    eprintln!("Saving vault...");
    storage::save_vault(&vault, password.as_bytes())?;
    Ok(())
}

/// Core delete logic without prompt_and_unlock or save (for REPL mode).
pub fn run_with_vault(vault: &mut VaultData, name: &str) -> Result<()> {
    let resolved_name = vault
        .resolve_entry_name(name)
        .ok_or_else(|| CryptoKeeperError::EntryNotFound(name.to_string()))?;

    let confirm = Confirm::new()
        .with_prompt(format!(
            "Are you sure you want to delete '{}'? This cannot be undone",
            resolved_name
        ))
        .default(false)
        .interact()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    if !confirm {
        return Err(CryptoKeeperError::Cancelled);
    }

    vault.remove_entry_by_id(name);

    print_success(&format!("Entry '{}' deleted.", resolved_name.cyan()));

    Ok(())
}
