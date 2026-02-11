use colored::Colorize;

use crate::error::Result;
use crate::vault::model::SecretType;
use crate::vault::storage;

pub fn run() -> Result<()> {
    let meta = storage::read_vault_metadata()?;

    if meta.is_empty() {
        println!();
        println!("{}", "No entries stored yet.".dimmed());
        println!(
            "{}",
            "Use `keeper add` to store your first key or phrase.".dimmed()
        );
        return Ok(());
    }

    println!();
    println!(
        "  {:<30} {:<15} {}",
        "NAME".bold(),
        "NETWORK".bold(),
        "TYPE".bold()
    );
    println!("  {}", "─".repeat(60).dimmed());

    for entry in &meta {
        let type_str = match entry.secret_type {
            SecretType::PrivateKey => "Private Key".yellow(),
            SecretType::SeedPhrase => "Seed Phrase".magenta(),
        };
        println!(
            "  {:<30} {:<15} {}",
            entry.name.cyan(),
            entry.network,
            type_str
        );
    }

    println!();
    println!(
        "  {} total entries",
        meta.len().to_string().bold()
    );

    Ok(())
}
