use std::path::Path;

use colored::Colorize;
use dialoguer::Select;
use zeroize::Zeroizing;

use crate::error::{Result, TermKeyError};
use crate::ui::borders::print_box;
use crate::vault::model::VaultData;
use crate::vault::{session::VaultSession, storage};

pub fn run(file: &str) -> Result<()> {
    let mut session = VaultSession::prompt_and_open()?.session;
    let modified = run_with_vault(&mut session.vault, file)?;
    if modified {
        eprintln!("Saving vault...");
        session.save()?;
    }
    Ok(())
}

/// Core import logic without prompt_and_unlock or save (for REPL mode).
/// Returns true if the vault was modified and needs saving.
pub fn run_with_vault(vault: &mut VaultData, file: &str) -> Result<bool> {
    let file = file.trim_matches(|c| c == '\'' || c == '"');
    let path = Path::new(file);
    if !path.exists() {
        return Err(TermKeyError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("File not found: {file}"),
        )));
    }

    println!();
    let backup_password =
        Zeroizing::new(rpassword::prompt_password("Backup password: ").map_err(TermKeyError::Io)?);

    eprintln!("Decrypting backup...");
    let backup = storage::read_backup(backup_password.as_bytes(), path)?;
    backup.validate()?;

    let mut imported = 0;
    let mut skipped = 0;
    let mut candidate = vault.clone();

    for backup_entry in backup.entries {
        if candidate.has_entry(&backup_entry.name) {
            println!();
            println!(
                "  {} Entry '{}' already exists.",
                "!".yellow().bold(),
                backup_entry.name.cyan()
            );

            let options = &[
                "Skip",
                "Rename imported entry",
                "Overwrite existing",
                "Exit",
            ];
            let choice = Select::new()
                .with_prompt("How to resolve?")
                .items(options)
                .default(0)
                .interact()
                .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

            match choice {
                0 => {
                    skipped += 1;
                    continue;
                }
                1 => {
                    // Find a unique name
                    let mut new_name = format!("{} (imported)", backup_entry.name);
                    let mut counter = 2;
                    while candidate.has_entry(&new_name) {
                        new_name = format!("{} (imported {})", backup_entry.name, counter);
                        counter += 1;
                    }
                    println!("  Importing as '{}'", new_name.cyan());
                    let mut entry = backup_entry;
                    entry.name = new_name;
                    candidate.push_entry(entry)?;
                    imported += 1;
                }
                2 => {
                    let secondary_password = {
                        let existing =
                            candidate.find_entry(&backup_entry.name).ok_or_else(|| {
                                TermKeyError::EntryNotFound(backup_entry.name.clone())
                            })?;
                        super::prompt_secondary_password(existing)?
                    };
                    replace_imported_entry(
                        &mut candidate,
                        backup_entry,
                        secondary_password
                            .as_ref()
                            .map(|password| password.as_str()),
                    )?;
                    imported += 1;
                }
                _ => {
                    return Err(TermKeyError::Cancelled);
                }
            }
        } else {
            candidate.push_entry(backup_entry)?;
            imported += 1;
        }
    }

    candidate.validate()?;
    *vault = candidate;

    let lines = vec![format!(
        "{} {} imported, {} skipped.",
        "✓".green().bold(),
        imported.to_string().bold(),
        skipped.to_string().bold()
    )];
    println!();
    print_box(Some("Import Complete"), &lines);

    Ok(imported > 0)
}

fn replace_imported_entry(
    vault: &mut VaultData,
    replacement: crate::vault::model::Entry,
    secondary_password: Option<&str>,
) -> Result<()> {
    let original_name = replacement.name.clone();
    vault.replace_entry_authorized(&original_name, replacement, secondary_password)
}

pub(crate) fn merge_non_conflicting_entries(
    vault: &mut VaultData,
    backup: VaultData,
) -> Result<usize> {
    backup.validate()?;
    let mut candidate = vault.clone();
    let mut imported = 0;

    for entry in backup.entries {
        if candidate.has_entry(&entry.name) {
            continue;
        }
        candidate.push_entry(entry)?;
        imported += 1;
    }

    candidate.validate()?;
    *vault = candidate;
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::{merge_non_conflicting_entries, replace_imported_entry};
    use crate::commands::test_support::protected_entry;
    use crate::error::TermKeyError;
    use crate::vault::model::VaultData;

    #[test]
    fn import_rejects_invalid_protected_entry() {
        let mut invalid = protected_entry("Invalid", "secret", "view-pass");
        invalid.encrypted_secret_nonce = None;
        let backup = VaultData {
            entries: vec![invalid],
            version: 1,
            revision: 0,
        };
        let mut current = VaultData::new();

        let result = merge_non_conflicting_entries(&mut current, backup);

        assert!(matches!(result, Err(TermKeyError::InvalidEntry(_))));
        assert!(current.entries.is_empty());
    }

    #[test]
    fn import_rejects_internal_duplicates_without_partial_mutation() {
        let first = protected_entry("Duplicate", "one", "view-pass");
        let mut second = protected_entry("duplicate", "two", "view-pass");
        second.name = "duplicate".to_string();
        let backup = VaultData {
            entries: vec![first, second],
            version: 1,
            revision: 0,
        };
        let mut current = VaultData::new();

        let result = merge_non_conflicting_entries(&mut current, backup);

        assert!(matches!(result, Err(TermKeyError::EntryAlreadyExists(_))));
        assert!(current.entries.is_empty());
    }

    #[test]
    fn import_overwrite_protected_entry_requires_correct_password_transactionally() {
        let existing = protected_entry("Existing", "protected secret", "view-pass");
        let replacement = protected_entry("Existing", "imported secret", "import-pass");
        let mut current = VaultData {
            entries: vec![existing],
            version: 1,
            revision: 0,
        };
        let original = serde_json::to_vec(&current).unwrap();

        assert!(matches!(
            replace_imported_entry(&mut current, replacement.clone(), None),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert_eq!(serde_json::to_vec(&current).unwrap(), original);
        assert!(matches!(
            replace_imported_entry(&mut current, replacement.clone(), Some("wrong-pass")),
            Err(TermKeyError::SecondaryPasswordWrong)
        ));
        assert_eq!(serde_json::to_vec(&current).unwrap(), original);

        replace_imported_entry(&mut current, replacement, Some("view-pass")).unwrap();
        assert_eq!(
            &*current.entries[0]
                .reveal_secret(Some("import-pass"))
                .unwrap(),
            "imported secret"
        );
    }
}
