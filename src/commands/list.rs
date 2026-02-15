use colored::{ColoredString, Colorize};
use dialoguer::Select;

use crate::error::{CryptoKeeperError, Result};
use crate::ui;
use crate::ui::borders::{print_table_box, truncate_display};
use crate::vault::model::{EntryMeta, SecretType};
use crate::vault::storage;

fn parse_type_filter(filter: &str) -> Option<SecretType> {
    match filter.to_lowercase().as_str() {
        "privatekey" | "private-key" | "private_key" => Some(SecretType::PrivateKey),
        "seedphrase" | "seed-phrase" | "seed_phrase" => Some(SecretType::SeedPhrase),
        "password" | "passwords" => Some(SecretType::Password),
        _ => None,
    }
}

fn type_str(st: &SecretType) -> String {
    match st {
        SecretType::PrivateKey => "Private Key".to_string(),
        SecretType::SeedPhrase => "Seed Phrase".to_string(),
        SecretType::Password => "Password".to_string(),
    }
}

fn type_color(s: &str) -> ColoredString {
    match s {
        "Private Key" => s.yellow(),
        "Seed Phrase" => s.magenta(),
        "Password" => s.green(),
        _ => s.normal(),
    }
}

fn build_row(i: usize, entry: &EntryMeta) -> Vec<String> {
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
        type_str(&entry.secret_type),
        username,
        addr_or_url,
    ]
}

fn col_styles() -> Vec<fn(&str) -> ColoredString> {
    vec![
        |s| s.dimmed(),       // #
        |s| s.cyan(),         // NAME
        |s| s.normal(),       // NETWORK
        |s| type_color(s),    // TYPE
        |s| s.normal(),       // USERNAME
        |s| s.dimmed(),       // ADDRESS / URL
    ]
}

const HEADERS: &[&str] = &["#", "NAME", "NETWORK", "TYPE", "USERNAME", "ADDRESS / URL"];

pub fn run(filter: Option<&str>) -> Result<()> {
    // Validate filter early if provided
    if let Some(f) = filter {
        if parse_type_filter(f).is_none() {
            eprintln!(
                "{}",
                format!(
                    "Unknown filter '{}'. Valid filters: privatekey, seedphrase, password",
                    f
                )
                .red()
            );
            return Ok(());
        }
    }

    if ui::is_interactive() {
        interactive_loop(filter)
    } else {
        print_table(filter)
    }
}

/// List entries from a cached vault (for REPL mode — no disk read needed).
pub fn run_with_vault(vault: &crate::vault::model::VaultData, filter: Option<&str>) -> Result<()> {
    if let Some(f) = filter {
        if parse_type_filter(f).is_none() {
            eprintln!(
                "{}",
                format!(
                    "Unknown filter '{}'. Valid filters: privatekey, seedphrase, password",
                    f
                )
                .red()
            );
            return Ok(());
        }
    }

    let meta = vault.metadata();
    print_meta_table(&meta, filter)
}

fn filter_meta(meta: &[EntryMeta], filter: Option<&str>) -> Vec<(usize, EntryMeta)> {
    let type_filter = filter.and_then(parse_type_filter);
    meta.iter()
        .enumerate()
        .filter(|(_, e)| {
            type_filter
                .as_ref()
                .map_or(true, |ft| e.secret_type == *ft)
        })
        .map(|(i, e)| (i, e.clone()))
        .collect()
}

fn print_table(filter: Option<&str>) -> Result<()> {
    let meta = storage::read_vault_metadata()?;
    print_meta_table(&meta, filter)
}

fn print_meta_table(meta: &[EntryMeta], filter: Option<&str>) -> Result<()> {
    if meta.is_empty() {
        println!();
        println!("{}", "No entries stored yet.".dimmed());
        println!(
            "{}",
            "Use `keeper add` to store your first key or phrase.".dimmed()
        );
        return Ok(());
    }

    let filtered = filter_meta(meta, filter);

    if filtered.is_empty() {
        println!();
        println!("{}", "No entries match the given filter.".dimmed());
        return Ok(());
    }

    let rows: Vec<Vec<String>> = filtered
        .iter()
        .map(|(i, entry)| build_row(*i, entry))
        .collect();

    let title = match filter {
        Some(f) => format!("Vault — {} ({} entries)", f, filtered.len()),
        None => format!("Vault ({} entries)", filtered.len()),
    };
    println!();
    print_table_box(Some(&title), HEADERS, &rows, &col_styles());

    Ok(())
}

fn interactive_loop(filter: Option<&str>) -> Result<()> {
    loop {
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

        let filtered = filter_meta(&meta, filter);

        if filtered.is_empty() {
            println!();
            println!("{}", "No entries match the given filter.".dimmed());
            return Ok(());
        }

        let rows: Vec<Vec<String>> = filtered
            .iter()
            .map(|(i, entry)| build_row(*i, entry))
            .collect();

        let title = match filter {
            Some(f) => format!("Vault — {} ({} entries)", f, filtered.len()),
            None => format!("Vault ({} entries)", filtered.len()),
        };
        println!();
        print_table_box(Some(&title), HEADERS, &rows, &col_styles());

        // Build selection items: entry names + Exit
        let mut items: Vec<String> = filtered
            .iter()
            .map(|(i, e)| format!("{}. {}", i + 1, e.name))
            .collect();
        items.push("Exit".to_string());

        let selection = Select::new()
            .with_prompt("Select an entry")
            .items(&items)
            .default(0)
            .interact_opt()
            .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let Some(idx) = selection else {
            return Ok(());
        };

        if idx >= filtered.len() {
            return Ok(());
        }

        // Use the original index for commands
        let (original_idx, _) = &filtered[idx];
        let index_str = format!("{}", original_idx + 1);
        let entry_name = &filtered[idx].1.name;

        let actions = &["View", "Copy to Clipboard", "Edit", "Delete", "Back"];
        let action = Select::new()
            .with_prompt(format!("Action for '{}'", entry_name))
            .items(actions)
            .default(0)
            .interact_opt()
            .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let Some(action_idx) = action else {
            continue;
        };

        match action_idx {
            0 => {
                if let Err(e) = super::view::run(&index_str) {
                    ui::borders::print_error(&e.to_string());
                }
            }
            1 => {
                if let Err(e) = super::copy::run(&index_str) {
                    ui::borders::print_error(&e.to_string());
                }
            }
            2 => {
                if let Err(e) = super::edit::run(&index_str) {
                    ui::borders::print_error(&e.to_string());
                }
            }
            3 => {
                if let Err(e) = super::delete::run(&index_str) {
                    ui::borders::print_error(&e.to_string());
                }
            }
            4 | _ => {}
        }
    }
}
