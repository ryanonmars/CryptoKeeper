use colored::Colorize;

use crate::clipboard;
use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_success;
use crate::vault::model::VaultData;
use crate::vault::storage;

const CLEAR_AFTER_SECS: u64 = 10;

pub fn run(name: &str) -> Result<()> {
    let (vault, _password) = storage::prompt_and_unlock()?;
    run_with_vault(&vault, name, true)
}

/// Core copy logic without prompt_and_unlock (for REPL mode).
/// When `wait` is false (REPL mode), don't block waiting for clipboard clear.
pub fn run_with_vault(vault: &VaultData, name: &str, wait: bool) -> Result<()> {
    let entry = vault
        .find_entry_by_id(name)
        .ok_or_else(|| CryptoKeeperError::EntryNotFound(name.to_string()))?;

    clipboard::copy_and_clear(&entry.secret, CLEAR_AFTER_SECS)?;

    print_success(&format!(
        "Secret for '{}' copied to clipboard.",
        entry.name.cyan()
    ));
    println!(
        "{}",
        format!("  Clipboard will be cleared in {CLEAR_AFTER_SECS} seconds.").dimmed()
    );

    if wait {
        std::thread::sleep(std::time::Duration::from_secs(CLEAR_AFTER_SECS));
        println!("{}", "  Clipboard cleared.".dimmed());
    }

    Ok(())
}
