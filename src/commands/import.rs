use std::path::Path;

use colored::Colorize;
use dialoguer::Select;
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_box;
use crate::vault::storage;

pub fn run(file: &str) -> Result<()> {
    let (mut vault, vault_password) = storage::prompt_and_unlock()?;

    let path = Path::new(file);
    if !path.exists() {
        return Err(CryptoKeeperError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("File not found: {file}"),
        )));
    }

    println!();
    let backup_password = Zeroizing::new(
        rpassword::prompt_password("Backup password: ").map_err(CryptoKeeperError::Io)?,
    );

    eprintln!("Decrypting backup...");
    let backup = storage::read_backup(backup_password.as_bytes(), path)?;

    let mut imported = 0;
    let mut skipped = 0;

    for backup_entry in backup.entries {
        if vault.has_entry(&backup_entry.name) {
            println!();
            println!(
                "  {} Entry '{}' already exists.",
                "!".yellow().bold(),
                backup_entry.name.cyan()
            );

            let options = &["Skip", "Rename imported entry", "Overwrite existing"];
            let choice = Select::new()
                .with_prompt("How to resolve?")
                .items(options)
                .default(0)
                .interact()
                .map_err(|e| {
                    CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
                })?;

            match choice {
                0 => {
                    skipped += 1;
                    continue;
                }
                1 => {
                    // Find a unique name
                    let mut new_name = format!("{} (imported)", backup_entry.name);
                    let mut counter = 2;
                    while vault.has_entry(&new_name) {
                        new_name = format!("{} (imported {})", backup_entry.name, counter);
                        counter += 1;
                    }
                    println!("  Importing as '{}'", new_name.cyan());
                    let mut entry = backup_entry;
                    entry.name = new_name;
                    vault.entries.push(entry);
                    imported += 1;
                }
                2 => {
                    vault.remove_entry(&backup_entry.name);
                    vault.entries.push(backup_entry);
                    imported += 1;
                }
                _ => {
                    skipped += 1;
                    continue;
                }
            }
        } else {
            vault.entries.push(backup_entry);
            imported += 1;
        }
    }

    if imported > 0 {
        eprintln!("Saving vault...");
        storage::save_vault(&vault, vault_password.as_bytes())?;
    }

    let lines = vec![
        format!(
            "{} {} imported, {} skipped.",
            "✓".green().bold(),
            imported.to_string().bold(),
            skipped.to_string().bold()
        ),
    ];
    println!();
    print_box(Some("Import Complete"), &lines);

    Ok(())
}
