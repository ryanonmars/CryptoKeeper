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
        "  {:<28} {:<12} {:<14} {}",
        "NAME".bold(),
        "NETWORK".bold(),
        "TYPE".bold(),
        "ADDRESS".bold()
    );
    println!("  {}", "─".repeat(80).dimmed());

    for entry in &meta {
        let type_str = match entry.secret_type {
            SecretType::PrivateKey => "Private Key".yellow(),
            SecretType::SeedPhrase => "Seed Phrase".magenta(),
        };
        let addr = entry
            .public_address
            .as_deref()
            .map(|s| if s.len() > 20 { format!("{}…", &s[..19]) } else { s.to_string() })
            .unwrap_or_else(|| "-".to_string());
        println!(
            "  {:<28} {:<12} {:<14} {}",
            entry.name.cyan(),
            entry.network,
            type_str,
            addr.dimmed()
        );
    }

    println!();
    println!(
        "  {} total entries",
        meta.len().to_string().bold()
    );

    Ok(())
}
