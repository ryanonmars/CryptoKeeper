use std::path::Path;

use colored::Colorize;
use zeroize::Zeroizing;

use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_box;
use crate::ui::theme::heading;
use crate::vault::model::VaultData;
use crate::vault::storage;

pub fn run(file: &str) -> Result<()> {
    let (vault, _password) = storage::prompt_and_unlock()?;
    run_with_vault(&vault, file)
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

    let export_password = Zeroizing::new(
        rpassword::prompt_password("Backup password: ").map_err(CryptoKeeperError::Io)?,
    );

    if export_password.is_empty() {
        return Err(CryptoKeeperError::EmptyPassword);
    }

    let confirm = Zeroizing::new(
        rpassword::prompt_password("Confirm backup password: ").map_err(CryptoKeeperError::Io)?,
    );

    if *export_password != *confirm {
        return Err(CryptoKeeperError::PasswordMismatch);
    }

    let directory = directory.trim_matches(|c| c == '\'' || c == '"');
    let dir_path = Path::new(directory);
    
    if !dir_path.exists() {
        std::fs::create_dir_all(dir_path).map_err(CryptoKeeperError::Io)?;
    }
    
    if !dir_path.is_dir() {
        return Err(CryptoKeeperError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("'{}' is not a directory", directory)
        )));
    }
    
    let file_path = dir_path.join("backup.ck");
    
    eprintln!("Encrypting backup...");
    storage::write_backup(&vault, export_password.as_bytes(), &file_path)?;

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
