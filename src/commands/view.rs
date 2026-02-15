use colored::Colorize;
use dialoguer::{Confirm, Select};

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_box;
use crate::vault::model::{SecretType, VaultData};
use crate::vault::storage;

pub fn run(name: &str) -> Result<()> {
    let (vault, _password) = storage::prompt_and_unlock()?;
    run_with_vault(&vault, name)
}

/// Core view logic without prompt_and_unlock (for REPL mode).
pub fn run_with_vault(vault: &VaultData, name: &str) -> Result<()> {
    let entry = vault
        .find_entry_by_id(name)
        .ok_or_else(|| CryptoKeeperError::EntryNotFound(name.to_string()))?;

    let mut lines = vec![
        format!("{:<16} {}", "Name:".bold(), entry.name.cyan()),
        format!("{:<16} {}", "Type:".bold(), entry.secret_type),
    ];
    if !entry.network.is_empty() {
        lines.push(format!("{:<16} {}", "Network:".bold(), entry.network));
    }
    if let Some(ref addr) = entry.public_address {
        lines.push(format!("{:<16} {}", "Public address:".bold(), addr));
    }
    if entry.secret_type == SecretType::Password {
        if let Some(ref uname) = entry.username {
            lines.push(format!("{:<16} {}", "Username:".bold(), uname));
        }
        if let Some(ref url) = entry.url {
            lines.push(format!("{:<16} {}", "URL:".bold(), url));
        }
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
        
        let options = &["Clear screen and continue", "Keep visible"];
        let clear_choice = Select::new()
            .with_prompt("What would you like to do?")
            .items(options)
            .default(0)
            .interact()
            .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        
        if clear_choice == 0 {
            use crossterm::{execute, terminal::{Clear, ClearType}, cursor::MoveTo};
            execute!(
                std::io::stdout(),
                Clear(ClearType::All),
                MoveTo(0, 0)
            ).map_err(CryptoKeeperError::Io)?;
        }
    }

    Ok(())
}
