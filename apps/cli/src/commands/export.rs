use std::path::Path;

use colored::Colorize;
use dialoguer::Input;
use zeroize::Zeroizing;

use crate::error::{Result, TermKeyError};
use crate::ui::borders::print_box;
use crate::ui::theme::heading;
use crate::vault::model::VaultData;
use crate::vault::{session::VaultSession, storage};

const BACKUP_EXTENSION: &str = ".termkey";

pub(crate) fn backup_file_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'))
    {
        return Err(TermKeyError::ConfigError(
            "Backup name must be a filename without path separators or control characters."
                .to_string(),
        ));
    }

    if name.to_ascii_lowercase().ends_with(BACKUP_EXTENSION) {
        Ok(name.to_string())
    } else {
        Ok(format!("{name}{BACKUP_EXTENSION}"))
    }
}

pub fn run(file: &str) -> Result<()> {
    let session = VaultSession::prompt_and_open()?.session;
    run_with_vault(&session.vault, file)
}

/// Core export logic without prompt_and_unlock (for REPL mode).
pub fn run_with_vault(vault: &VaultData, directory: &str) -> Result<()> {
    println!();
    println!("  {}", heading("Export encrypted backup"));
    println!(
        "{}",
        "  Choose a password for this backup (can differ from master password).".dimmed()
    );
    println!();

    let backup_name: String = Input::new()
        .with_prompt("Backup name")
        .default("backup".to_string())
        .interact_text()
        .map_err(|error| TermKeyError::Io(std::io::Error::other(error)))?;
    let backup_name = backup_file_name(&backup_name)?;

    let export_password =
        Zeroizing::new(rpassword::prompt_password("Backup password: ").map_err(TermKeyError::Io)?);

    if export_password.is_empty() {
        return Err(TermKeyError::EmptyPassword);
    }

    let confirm = Zeroizing::new(
        rpassword::prompt_password("Confirm backup password: ").map_err(TermKeyError::Io)?,
    );

    if *export_password != *confirm {
        return Err(TermKeyError::PasswordMismatch);
    }

    let directory = directory.trim_matches(|c| c == '\'' || c == '"');
    let dir_path = Path::new(directory);

    if !dir_path.exists() {
        std::fs::create_dir_all(dir_path).map_err(TermKeyError::Io)?;
    }

    if !dir_path.is_dir() {
        return Err(TermKeyError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("'{}' is not a directory", directory),
        )));
    }

    let file_path = dir_path.join(backup_name);

    eprintln!("Encrypting backup...");
    storage::write_backup(vault, export_password.as_bytes(), &file_path)?;

    let lines = vec![
        format!(
            "{} Backup exported to '{}'",
            "✓".green().bold(),
            file_path.display().to_string().cyan()
        ),
        format!(
            "{} entries exported.",
            vault.entries.len().to_string().bold()
        ),
    ];
    println!();
    print_box(Some("Export Complete"), &lines);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_name_gets_termkey_extension() {
        assert_eq!(
            backup_file_name("family-wallet").unwrap(),
            "family-wallet.termkey"
        );
    }

    #[test]
    fn backup_name_does_not_duplicate_termkey_extension() {
        assert_eq!(
            backup_file_name("family-wallet.termkey").unwrap(),
            "family-wallet.termkey"
        );
    }

    #[test]
    fn backup_name_rejects_paths_and_unsafe_names() {
        for invalid in ["", ".", "..", "nested/name", r"nested\name", "bad\nname"] {
            assert!(
                backup_file_name(invalid).is_err(),
                "accepted unsafe backup name: {invalid:?}"
            );
        }
    }
}
