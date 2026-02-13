use colored::{ColoredString, Colorize};
use dialoguer::Select;

use crate::error::{CryptoKeeperError, Result};
use crate::ui;
use crate::ui::borders::{print_table_box, truncate_display};
use crate::vault::model::SecretType;
use crate::vault::storage;

pub fn run() -> Result<()> {
    if ui::is_interactive() {
        interactive_loop()
    } else {
        print_table()
    }
}

fn print_table() -> Result<()> {
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

    let headers = &["#", "NAME", "NETWORK", "TYPE", "ADDRESS"];
    let rows: Vec<Vec<String>> = meta
        .iter()
        .enumerate()
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
        |s| s.dimmed(),       // #
        |s| s.cyan(),         // NAME
        |s| s.normal(),       // NETWORK
        |s| match s {         // TYPE
            "Private Key" => s.yellow(),
            "Seed Phrase" => s.magenta(),
            _ => s.normal(),
        },
        |s| s.dimmed(),       // ADDRESS
    ];

    let title = format!("Vault ({} entries)", meta.len());
    println!();
    print_table_box(Some(&title), headers, &rows, &col_styles);

    Ok(())
}

fn interactive_loop() -> Result<()> {
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

        let headers = &["#", "NAME", "NETWORK", "TYPE", "ADDRESS"];
        let rows: Vec<Vec<String>> = meta
            .iter()
            .enumerate()
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

        let title = format!("Vault ({} entries)", meta.len());
        println!();
        print_table_box(Some(&title), headers, &rows, &col_styles);

        // Build selection items: entry names + Exit
        let mut items: Vec<String> = meta
            .iter()
            .enumerate()
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
            // User pressed Esc
            return Ok(());
        };

        if idx >= meta.len() {
            // "Exit" selected
            return Ok(());
        }

        // Show action sub-menu for selected entry
        let entry_name = &meta[idx].name;
        let index_str = format!("{}", idx + 1);

        let actions = &["View", "Copy to Clipboard", "Edit", "Delete", "Back"];
        let action = Select::new()
            .with_prompt(format!("Action for '{}'", entry_name))
            .items(actions)
            .default(0)
            .interact_opt()
            .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let Some(action_idx) = action else {
            // User pressed Esc — go back to list
            continue;
        };

        match action_idx {
            0 => {
                // View
                if let Err(e) = super::view::run(&index_str) {
                    ui::borders::print_error(&e.to_string());
                }
            }
            1 => {
                // Copy
                if let Err(e) = super::copy::run(&index_str) {
                    ui::borders::print_error(&e.to_string());
                }
            }
            2 => {
                // Edit
                if let Err(e) = super::edit::run(&index_str) {
                    ui::borders::print_error(&e.to_string());
                }
            }
            3 => {
                // Delete
                if let Err(e) = super::delete::run(&index_str) {
                    ui::borders::print_error(&e.to_string());
                }
            }
            4 | _ => {
                // Back — loop continues
            }
        }
    }
}
