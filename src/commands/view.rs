use colored::Colorize;
use dialoguer::Confirm;

use crate::error::{CryptoKeeperError, Result};
use crate::vault::storage;

pub fn run(name: &str) -> Result<()> {
    let (vault, _password) = storage::prompt_and_unlock()?;

    let entry = vault
        .find_entry(name)
        .ok_or_else(|| CryptoKeeperError::EntryNotFound(name.to_string()))?;

    println!();
    println!("  {} {}", "Name:".bold(), entry.name.cyan());
    println!("  {} {}", "Type:".bold(), entry.secret_type);
    println!("  {} {}", "Network:".bold(), entry.network);
    if let Some(ref addr) = entry.public_address {
        println!("  {} {}", "Public address:".bold(), addr);
    }
    if !entry.notes.is_empty() {
        println!("  {} {}", "Notes:".bold(), entry.notes);
    }
    println!(
        "  {} {}",
        "Created:".bold(),
        entry.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "  {} {}",
        "Updated:".bold(),
        entry.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("  {} {}", "Secret:".bold(), "••••••••".dimmed());

    println!();
    let reveal = Confirm::new()
        .with_prompt("Reveal secret?")
        .default(false)
        .interact()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    if reveal {
        println!();
        println!("  {} {}", "Secret:".bold(), entry.secret.red());
        println!();
    }

    Ok(())
}
