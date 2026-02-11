use colored::Colorize;

use crate::error::{CryptoKeeperError, Result};
use crate::vault::model::SecretType;
use crate::vault::storage;

pub fn run(query: &str) -> Result<()> {
    let (vault, _password) = storage::prompt_and_unlock()?;

    let query_lower = query.to_lowercase();
    let matches: Vec<_> = vault
        .entries
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&query_lower)
                || e.network.to_lowercase().contains(&query_lower)
                || e.notes.to_lowercase().contains(&query_lower)
        })
        .collect();

    if matches.is_empty() {
        return Err(CryptoKeeperError::NoSearchResults(query.to_string()));
    }

    println!();
    println!(
        "  Search results for '{}' ({} found):",
        query.cyan(),
        matches.len()
    );
    println!();
    println!(
        "  {:<30} {:<15} {}",
        "NAME".bold(),
        "NETWORK".bold(),
        "TYPE".bold()
    );
    println!("  {}", "─".repeat(60).dimmed());

    for entry in &matches {
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

    Ok(())
}
