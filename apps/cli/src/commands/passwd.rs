use zeroize::Zeroizing;

use crate::error::{Result, TermKeyError};
use crate::ui::borders::print_success;
use crate::ui::theme::heading;
use crate::vault::session::VaultSession;

pub fn run() -> Result<()> {
    let mut session = VaultSession::prompt_and_open()?.session;
    let new_password = prompt_new_password()?;
    eprintln!("Re-encrypting vault with new password...");
    session.change_master_password(new_password)?;
    let active_recovery = crate::config::load_config()
        .ok()
        .is_some_and(|config| config.has_active_recovery_for(session.vault_id()));
    print_success(password_change_success_message(active_recovery));
    Ok(())
}

pub(crate) fn password_change_success_message(active_recovery: bool) -> &'static str {
    if active_recovery {
        "Master password changed successfully. Your recovery phrase remains active."
    } else {
        "Master password changed successfully. Configure a recovery phrase in Settings."
    }
}

/// Prompt for a new master password (for both CLI and REPL mode).
pub fn prompt_new_password() -> Result<Zeroizing<String>> {
    println!();
    println!("  {}", heading("Change master password"));
    println!();

    let new_password = Zeroizing::new(
        rpassword::prompt_password("New master password: ").map_err(TermKeyError::Io)?,
    );

    if new_password.is_empty() {
        return Err(TermKeyError::EmptyPassword);
    }

    let confirm = Zeroizing::new(
        rpassword::prompt_password("Confirm new password: ").map_err(TermKeyError::Io)?,
    );

    if *new_password != *confirm {
        return Err(TermKeyError::PasswordMismatch);
    }

    Ok(new_password)
}

#[cfg(test)]
mod tests {
    use super::password_change_success_message;

    #[test]
    fn password_change_message_only_claims_recovery_when_active() {
        assert!(password_change_success_message(true)
            .to_ascii_lowercase()
            .contains("remains active"));
        let without_recovery = password_change_success_message(false).to_ascii_lowercase();
        assert!(!without_recovery.contains("remains active"));
        assert!(without_recovery.contains("configure"));
    }
}
