use colored::Colorize;
use dialoguer::Confirm;

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_box;
use crate::vault::storage;

pub fn run(name: &str) -> Result<()> {
    let (vault, _password) = storage::prompt_and_unlock()?;

    let entry = vault
        .find_entry_by_id(name)
        .ok_or_else(|| CryptoKeeperError::EntryNotFound(name.to_string()))?;

    let mut lines = vec![
        format!("{:<16} {}", "Name:".bold(), entry.name.cyan()),
        format!("{:<16} {}", "Type:".bold(), entry.secret_type),
        format!("{:<16} {}", "Network:".bold(), entry.network),
    ];
    if let Some(ref addr) = entry.public_address {
        lines.push(format!("{:<16} {}", "Public address:".bold(), addr));
    }
    if !entry.notes.is_empty() {
        lines.push(format!("{:<16} {}", "Notes:".bold(), entry.notes));
    }
    lines.push(format!(
        "{:<16} {}",
        "Created:".bold(),
        entry.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));
    lines.push(format!(
        "{:<16} {}",
        "Updated:".bold(),
        entry.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));
    lines.push(format!("{:<16} {}", "Secret:".bold(), "••••••••".dimmed()));

    println!();
    print_box(Some("Entry Details"), &lines);

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
