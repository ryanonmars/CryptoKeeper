use colored::Colorize;
use dialoguer::Confirm;

use crate::error::{Result, TermKeyError};
use crate::ui::borders::print_success;
use crate::vault::model::VaultData;
use crate::vault::session::VaultSession;

pub fn run(name: &str) -> Result<()> {
    let mut session = VaultSession::prompt_and_open()?.session;
    run_with_vault(&mut session.vault, name)?;
    eprintln!("Saving vault...");
    session.save()?;
    Ok(())
}

/// Core delete logic without prompt_and_unlock or save (for REPL mode).
pub fn run_with_vault(vault: &mut VaultData, name: &str) -> Result<()> {
    let resolved_name = vault
        .resolve_entry_name(name)
        .ok_or_else(|| TermKeyError::EntryNotFound(name.to_string()))?;

    let confirm = Confirm::new()
        .with_prompt(format!(
            "Are you sure you want to delete '{}'? This cannot be undone",
            resolved_name
        ))
        .default(false)
        .interact()
        .map_err(|e| TermKeyError::Io(std::io::Error::other(e)))?;

    if !confirm {
        return Err(TermKeyError::Cancelled);
    }

    let secondary_password = {
        let entry = vault
            .find_entry(&resolved_name)
            .ok_or_else(|| TermKeyError::EntryNotFound(resolved_name.clone()))?;
        super::prompt_secondary_password(entry)?
    };
    delete_entry(
        vault,
        &resolved_name,
        secondary_password
            .as_ref()
            .map(|password| password.as_str()),
    )?;

    print_success(&format!("Entry '{}' deleted.", resolved_name.cyan()));

    Ok(())
}

fn delete_entry(vault: &mut VaultData, name: &str, secondary_password: Option<&str>) -> Result<()> {
    vault.remove_entry_authorized(name, secondary_password)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::delete_entry;
    use crate::commands::test_support::protected_entry;
    use crate::error::TermKeyError;
    use crate::vault::model::VaultData;

    #[test]
    fn cli_delete_protected_entry_requires_correct_password_transactionally() {
        let entry = protected_entry("Protected", "secret", "view-pass");
        let mut vault = VaultData {
            entries: vec![entry],
            version: 1,
            revision: 0,
        };
        let original = serde_json::to_vec(&vault).unwrap();

        assert!(matches!(
            delete_entry(&mut vault, "Protected", None),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);
        assert!(matches!(
            delete_entry(&mut vault, "Protected", Some("wrong-pass")),
            Err(TermKeyError::SecondaryPasswordWrong)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);

        delete_entry(&mut vault, "Protected", Some("view-pass")).unwrap();
        assert!(vault.entries.is_empty());
    }
}
