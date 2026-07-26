use colored::Colorize;
use dialoguer::{Confirm, Select};

use crate::error::{Result, TermKeyError};
use crate::links;
use crate::ui::borders::{print_success, print_trusted_box, BoxTextStyle, TrustedBoxLine};
use crate::vault::model::Entry;
use crate::vault::model::VaultData;
use crate::vault::session::VaultSession;

pub fn run(name: &str) -> Result<()> {
    let session = VaultSession::prompt_and_open()?.session;
    run_with_vault(&session.vault, name)
}

/// Core view logic without prompt_and_unlock (for REPL mode).
pub fn run_with_vault(vault: &VaultData, name: &str) -> Result<()> {
    let entry = vault
        .find_entry_by_id(name)
        .ok_or_else(|| TermKeyError::EntryNotFound(name.to_string()))?;

    let lines = entry_display_lines(entry, crate::ui::is_interactive());

    println!();
    print_trusted_box(Some("Entry Details"), &lines);

    if let Some(url) = entry.url.as_deref().filter(|url| links::is_web_url(url)) {
        loop {
            let options = &["Reveal secret", "Open URL", "Done"];
            let choice = Select::new()
                .with_prompt("What would you like to do?")
                .items(options)
                .default(0)
                .interact()
                .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

            match choice {
                0 => reveal_secret(entry)?,
                1 => {
                    links::open_url(url)?;
                    print_success("Opened URL in your browser.");
                }
                _ => break,
            }
        }
    } else {
        let reveal = Confirm::new()
            .with_prompt("Reveal secret?")
            .default(false)
            .interact()
            .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

        if reveal {
            reveal_secret(entry)?;
        }
    }

    Ok(())
}

fn entry_display_lines(entry: &Entry, interactive: bool) -> Vec<TrustedBoxLine> {
    let mut name = labeled_entry_line("Name:");
    name.push_styled(&entry.name, BoxTextStyle::Cyan);
    let mut secret_type = labeled_entry_line("Type:");
    secret_type.push_text(&entry.secret_type.to_string());
    let mut lines = vec![name, secret_type];

    if !entry.network.is_empty() {
        let mut line = labeled_entry_line("Network:");
        line.push_text(&entry.network);
        lines.push(line);
    }
    if let Some(ref addr) = entry.public_address {
        let mut line = labeled_entry_line("Public address:");
        line.push_text(addr);
        lines.push(line);
    }
    if entry.secret_type.is_password_type() {
        if let Some(ref uname) = entry.username {
            let mut line = labeled_entry_line("Username:");
            line.push_text(uname);
            lines.push(line);
        }
        if let Some(ref url) = entry.url {
            let mut line = labeled_entry_line("URL:");
            if interactive {
                line.push_hyperlink(url, url);
            } else {
                line.push_text(url);
            }
            lines.push(line);
        }
    }
    if !entry.notes.is_empty() {
        let mut line = labeled_entry_line("Notes:");
        line.push_text(&entry.notes);
        lines.push(line);
    }
    let mut created = labeled_entry_line("Created:");
    created.push_text(&entry.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string());
    lines.push(created);
    let mut updated = labeled_entry_line("Updated:");
    updated.push_text(&entry.updated_at.format("%Y-%m-%d %H:%M:%S UTC").to_string());
    lines.push(updated);
    let mut secret = labeled_entry_line("Secret:");
    secret.push_styled("••••••••", BoxTextStyle::Dimmed);
    lines.push(secret);

    lines
}

fn labeled_entry_line(label: &str) -> TrustedBoxLine {
    let mut line = TrustedBoxLine::new();
    line.push_styled_padded(label, 16, BoxTextStyle::Bold);
    line
}

fn reveal_secret(entry: &Entry) -> Result<()> {
    let secondary_password = super::prompt_secondary_password(entry)?;
    let secret = secret_for_view(
        entry,
        secondary_password
            .as_ref()
            .map(|password| password.as_str()),
    )?;

    println!();
    println!(
        "  {} {}",
        "Secret:".bold(),
        revealed_secret_display(secret.as_str()).red()
    );
    println!();

    let options = &["Clear screen and continue", "Keep visible"];
    let clear_choice = Select::new()
        .with_prompt("What would you like to do?")
        .items(options)
        .default(0)
        .interact()
        .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

    if clear_choice == 0 {
        use crossterm::{
            cursor::MoveTo,
            execute,
            terminal::{Clear, ClearType},
        };
        execute!(std::io::stdout(), Clear(ClearType::All), MoveTo(0, 0))
            .map_err(TermKeyError::Io)?;
    }

    Ok(())
}

fn secret_for_view(
    entry: &Entry,
    secondary_password: Option<&str>,
) -> Result<zeroize::Zeroizing<String>> {
    entry.reveal_secret(secondary_password)
}

fn revealed_secret_display(secret: &str) -> String {
    links::sanitize_terminal_text(secret)
}

#[cfg(test)]
mod tests {
    use super::{entry_display_lines, revealed_secret_display, secret_for_view};
    use crate::commands::test_support::protected_entry;
    use crate::vault::model::{Entry, SecretType};
    use chrono::Utc;

    fn contains_terminal_control(value: &str) -> bool {
        value
            .chars()
            .any(|ch| matches!(ch as u32, 0x00..=0x1f | 0x7f..=0x9f))
    }

    #[test]
    fn cli_view_protected_entry_never_prints_marker() {
        let entry = protected_entry("Protected", "actual-secret", "view-pass");

        let revealed = secret_for_view(&entry, Some("view-pass")).unwrap();

        assert_eq!(&*revealed, "actual-secret");
        assert_ne!(&*revealed, "[encrypted]");
    }

    #[test]
    fn cli_view_rendering_sanitizes_every_entry_derived_display_field() {
        let entry = Entry {
            name: "na\u{0007}me".to_string(),
            secret: "unused".to_string(),
            secret_type: SecretType::Password,
            network: "net\u{0085}work".to_string(),
            public_address: Some("pub\u{0007}lic".to_string()),
            username: Some("us\u{0085}er".to_string()),
            url: Some("https://example.com/a\u{0007}b".to_string()),
            site_rules: Vec::new(),
            notes: "no\u{0000}tes".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        };

        let lines = entry_display_lines(&entry, false);
        let rendered = lines
            .iter()
            .map(|line| line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("name"));
        assert!(rendered.contains("network"));
        assert!(rendered.contains("public"));
        assert!(rendered.contains("user"));
        assert!(rendered.contains("https://example.com/ab"));
        assert!(rendered.contains("notes"));
        assert!(lines
            .iter()
            .all(|line| !contains_terminal_control(line.as_str())));

        let revealed = revealed_secret_display("sec\u{001b}]0;owned\u{0007}ret");
        assert_eq!(revealed, "sec]0;ownedret");
        assert!(!contains_terminal_control(&revealed));
    }
}
