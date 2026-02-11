use colored::Colorize;

use crate::error::{CryptoKeeperError, Result};
use crate::vault::model::SecretType;
use crate::vault::storage;

pub fn run(query: &str) -> Result<()> {
    let meta = storage::read_vault_metadata()?;

    let query_lower = query.to_lowercase();
    let matches: Vec<_> = meta
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
        "  {:<28} {:<12} {:<14} {}",
        "NAME".bold(),
        "NETWORK".bold(),
        "TYPE".bold(),
        "ADDRESS".bold()
    );
    println!("  {}", "─".repeat(80).dimmed());

    for entry in &matches {
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

    Ok(())
}
