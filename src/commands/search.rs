use colored::{ColoredString, Colorize};

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::{print_table_box, truncate_display};
use crate::vault::model::{EntryMeta, SecretType, VaultData};
use crate::vault::storage;

pub fn run(query: &str) -> Result<()> {
    let meta = storage::read_vault_metadata()?;
    run_with_meta(&meta, query)
}

/// Core search logic using pre-loaded metadata (for REPL mode).
pub fn run_with_vault(vault: &VaultData, query: &str) -> Result<()> {
    let meta = vault.metadata();
    run_with_meta(&meta, query)
}

fn run_with_meta(meta: &[EntryMeta], query: &str) -> Result<()> {

    let query_lower = query.to_lowercase();
    let matches: Vec<_> = meta
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.name.to_lowercase().contains(&query_lower)
                || e.network.to_lowercase().contains(&query_lower)
                || e.notes.to_lowercase().contains(&query_lower)
                || e.username
                    .as_deref()
                    .map_or(false, |u| u.to_lowercase().contains(&query_lower))
                || e.url
                    .as_deref()
                    .map_or(false, |u| u.to_lowercase().contains(&query_lower))
        })
        .collect();

    if matches.is_empty() {
        return Err(CryptoKeeperError::NoSearchResults(query.to_string()));
    }

    let headers = &["#", "NAME", "NETWORK", "TYPE", "USERNAME", "ADDRESS / URL"];
    let rows: Vec<Vec<String>> = matches
        .iter()
        .map(|(i, entry)| {
            let type_str = match entry.secret_type {
                SecretType::PrivateKey => "Private Key".to_string(),
                SecretType::SeedPhrase => "Seed Phrase".to_string(),
                SecretType::Password => "Password".to_string(),
            };
            let addr_or_url = if entry.secret_type == SecretType::Password {
                entry
                    .url
                    .as_deref()
                    .map(|s| truncate_display(s, 20))
                    .unwrap_or_else(|| "-".to_string())
            } else {
                entry
                    .public_address
                    .as_deref()
                    .map(|s| truncate_display(s, 20))
                    .unwrap_or_else(|| "-".to_string())
            };
            let network = if entry.network.is_empty() {
                "-".to_string()
            } else {
                entry.network.clone()
            };
            let username = entry
                .username
                .as_deref()
                .unwrap_or("-")
                .to_string();
            vec![
                format!("{}", i + 1),
                entry.name.clone(),
                network,
                type_str,
                username,
                addr_or_url,
            ]
        })
        .collect();

    let col_styles: Vec<fn(&str) -> ColoredString> = vec![
        |s| s.dimmed(),
        |s| s.cyan(),
        |s| s.normal(),
        |s| match s {
            "Private Key" => s.yellow(),
            "Seed Phrase" => s.magenta(),
            "Password" => s.green(),
            _ => s.normal(),
        },
        |s| s.normal(),
        |s| s.dimmed(),
    ];

    let title = format!("Search: '{}' ({} found)", query, matches.len());
    println!();
    print_table_box(Some(&title), headers, &rows, &col_styles);

    Ok(())
}
