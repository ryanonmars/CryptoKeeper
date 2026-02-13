use colored::{ColoredString, Colorize};

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::{print_table_box, truncate_display};
use crate::vault::model::SecretType;
use crate::vault::storage;

pub fn run(query: &str) -> Result<()> {
    let meta = storage::read_vault_metadata()?;

    let query_lower = query.to_lowercase();
    let matches: Vec<_> = meta
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.name.to_lowercase().contains(&query_lower)
                || e.network.to_lowercase().contains(&query_lower)
                || e.notes.to_lowercase().contains(&query_lower)
        })
        .collect();

    if matches.is_empty() {
        return Err(CryptoKeeperError::NoSearchResults(query.to_string()));
    }

    let headers = &["#", "NAME", "NETWORK", "TYPE", "ADDRESS"];
    let rows: Vec<Vec<String>> = matches
        .iter()
        .map(|(i, entry)| {
            let type_str = match entry.secret_type {
                SecretType::PrivateKey => "Private Key".to_string(),
                SecretType::SeedPhrase => "Seed Phrase".to_string(),
            };
            let addr = entry
                .public_address
                .as_deref()
                .map(|s| truncate_display(s, 20))
                .unwrap_or_else(|| "-".to_string());
            vec![
                format!("{}", i + 1),
                entry.name.clone(),
                entry.network.clone(),
                type_str,
                addr,
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
            _ => s.normal(),
        },
        |s| s.dimmed(),
    ];

    let title = format!("Search: '{}' ({} found)", query, matches.len());
    println!();
    print_table_box(Some(&title), headers, &rows, &col_styles);

    Ok(())
}
