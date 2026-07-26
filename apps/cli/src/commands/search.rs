use colored::{ColoredString, Colorize};

use crate::error::{Result, TermKeyError};
use crate::ui::borders::{print_table_box, truncate_display};
use crate::vault::model::{EntryMeta, VaultData};
use crate::vault::session::VaultSession;

fn display_name(entry: &EntryMeta) -> String {
    if entry.has_secondary_password {
        format!("{} [locked]", entry.name)
    } else {
        entry.name.clone()
    }
}

pub fn run(query: &str) -> Result<()> {
    let session = VaultSession::prompt_and_open()?.session;
    run_with_vault(&session.vault, query)
}

pub(crate) fn run_with_vault(vault: &VaultData, query: &str) -> Result<()> {
    run_with_meta(&vault.metadata(), query)
}

fn run_with_meta(meta: &[EntryMeta], query: &str) -> Result<()> {
    let query_lower = query.to_lowercase();
    let matches: Vec<_> = meta
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.name.to_lowercase().contains(&query_lower)
                || e.secret_type
                    .to_string()
                    .to_lowercase()
                    .contains(&query_lower)
                || e.network.to_lowercase().contains(&query_lower)
                || e.notes.to_lowercase().contains(&query_lower)
                || e.username
                    .as_deref()
                    .is_some_and(|u| u.to_lowercase().contains(&query_lower))
                || e.url
                    .as_deref()
                    .is_some_and(|u| u.to_lowercase().contains(&query_lower))
        })
        .collect();

    if matches.is_empty() {
        return Err(TermKeyError::NoSearchResults(query.to_string()));
    }

    let headers = &["#", "NAME", "TYPE"];
    let rows: Vec<Vec<String>> = matches
        .iter()
        .map(|(i, entry)| {
            vec![
                format!("{}", i + 1),
                truncate_display(&display_name(entry), 48),
                entry.secret_type.to_string(),
            ]
        })
        .collect();

    let col_styles: Vec<fn(&str) -> ColoredString> =
        vec![|s| s.dimmed(), |s| s.cyan(), |s| match s {
            "Private Key" => s.yellow(),
            "Seed Phrase" => s.magenta(),
            "Password" => s.green(),
            "Other" => s.blue(),
            _ => s.normal(),
        }];

    let title = format!("Search: '{}' ({} found)", query, matches.len());
    println!();
    print_table_box(Some(&title), headers, &rows, &col_styles);

    Ok(())
}
