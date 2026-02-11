use colored::Colorize;

use crate::clipboard;
use crate::error::{CryptoKeeperError, Result};
use crate::vault::storage;

const CLEAR_AFTER_SECS: u64 = 10;

pub fn run(name: &str) -> Result<()> {
    let (vault, _password) = storage::prompt_and_unlock()?;

    let entry = vault
        .find_entry_by_id(name)
        .ok_or_else(|| CryptoKeeperError::EntryNotFound(name.to_string()))?;

    clipboard::copy_and_clear(&entry.secret, CLEAR_AFTER_SECS)?;

    println!();
    println!(
        "{} Secret for '{}' copied to clipboard.",
        "✓".green().bold(),
        entry.name.cyan()
    );
    println!(
        "{}",
        format!("  Clipboard will be cleared in {CLEAR_AFTER_SECS} seconds.").dimmed()
    );

    // Wait for the clipboard-clear thread to finish
    std::thread::sleep(std::time::Duration::from_secs(CLEAR_AFTER_SECS));
    println!("{}", "  Clipboard cleared.".dimmed());

    Ok(())
}
