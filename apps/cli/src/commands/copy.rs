use colored::Colorize;
use std::time::Duration;

use crate::clipboard;
use crate::config::Config;
use crate::error::{Result, TermKeyError};
use crate::ui::borders::print_success;
use crate::vault::model::{Entry, VaultData};
use crate::vault::session::VaultSession;

pub fn run(name: &str) -> Result<()> {
    let session = VaultSession::prompt_and_open()?.session;
    let config = crate::config::load_config()?;
    run_with_vault_and_config(&session.vault, name, true, &config)
}

/// Core copy logic without prompt_and_unlock (for REPL mode).
/// When `wait` is false (REPL mode), don't block waiting for clipboard clear.
pub fn run_with_vault(vault: &VaultData, name: &str, wait: bool) -> Result<()> {
    let config = crate::config::load_config()?;
    run_with_vault_and_config(vault, name, wait, &config)
}

fn run_with_vault_and_config(
    vault: &VaultData,
    name: &str,
    wait: bool,
    config: &Config,
) -> Result<()> {
    run_with_vault_and_dependencies(
        vault,
        name,
        wait,
        config,
        CopyDependencies {
            copy_secret: clipboard::copy_and_clear,
            wait_for_cleanup: clipboard::ClipboardCleanup::wait,
            show_success: |message: String| print_success(&message),
            show_status: |message: String| println!("{}", message.dimmed()),
        },
    )
}

struct CopyDependencies<C, W, S, M> {
    copy_secret: C,
    wait_for_cleanup: W,
    show_success: S,
    show_status: M,
}

fn run_with_vault_and_dependencies<C, W, S, M, H>(
    vault: &VaultData,
    name: &str,
    wait: bool,
    config: &Config,
    dependencies: CopyDependencies<C, W, S, M>,
) -> Result<()>
where
    C: FnOnce(zeroize::Zeroizing<String>, Duration) -> Result<H>,
    W: FnOnce(H) -> Result<()>,
    S: FnMut(String),
    M: FnMut(String),
{
    let CopyDependencies {
        copy_secret,
        wait_for_cleanup,
        mut show_success,
        mut show_status,
    } = dependencies;
    let entry = vault
        .find_entry_by_id(name)
        .ok_or_else(|| TermKeyError::EntryNotFound(name.to_string()))?;

    let secondary_password = super::prompt_secondary_password(entry)?;
    let secret = secret_for_copy(
        entry,
        secondary_password
            .as_ref()
            .map(|password| password.as_str()),
    )?;
    let clear_after = Duration::from_secs(config.clipboard_timeout_secs);
    let cleanup = copy_secret(secret, clear_after)?;
    let clear_after_secs = clear_after.as_secs();

    show_success(format!(
        "Secret for '{}' copied to clipboard.",
        entry.name.cyan()
    ));
    show_status(format!(
        "  Clipboard will be cleared in {clear_after_secs} seconds if it is unchanged."
    ));

    if wait {
        wait_for_cleanup(cleanup)?;
        show_status("  Clipboard cleanup timer finished.".to_string());
    } else {
        drop(cleanup);
    }

    Ok(())
}

fn secret_for_copy(
    entry: &Entry,
    secondary_password: Option<&str>,
) -> Result<zeroize::Zeroizing<String>> {
    entry.reveal_secret(secondary_password)
}

#[cfg(test)]
mod tests {
    use super::{run_with_vault_and_dependencies, secret_for_copy, CopyDependencies};
    use crate::commands::test_support::protected_entry;
    use crate::config::Config;
    use crate::vault::model::{Entry, SecretType, VaultData};
    use chrono::Utc;
    use std::cell::{Cell, RefCell};
    use std::sync::mpsc;
    use std::time::Duration;

    fn plain_entry(name: &str, secret: &str) -> Entry {
        Entry {
            name: name.to_string(),
            secret: secret.to_string(),
            secret_type: SecretType::Password,
            network: String::new(),
            public_address: None,
            username: None,
            url: None,
            site_rules: Vec::new(),
            notes: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        }
    }

    #[test]
    fn cli_copy_protected_entry_uses_decrypted_value() {
        let entry = protected_entry("Protected", "clipboard-secret", "view-pass");

        let copied = secret_for_copy(&entry, Some("view-pass")).unwrap();

        assert_eq!(&*copied, "clipboard-secret");
        assert_ne!(&*copied, "[encrypted]");
    }

    #[test]
    fn cli_copy_uses_configured_timeout() {
        let config = Config {
            clipboard_timeout_secs: 37,
            ..Config::default()
        };
        let mut vault = VaultData::new();
        vault
            .push_entry(plain_entry("Configured", "configured secret"))
            .unwrap();
        let copied = RefCell::new(None);
        let cleanup_waited = Cell::new(false);
        let (cleanup_complete, cleanup) = mpsc::channel();
        let (wait_started, wait_started_receiver) = mpsc::channel();
        let cleanup_worker = std::thread::spawn(move || {
            wait_started_receiver.recv().unwrap();
            cleanup_complete.send(()).unwrap();
        });
        let status_messages = RefCell::new(Vec::new());

        run_with_vault_and_dependencies(
            &vault,
            "Configured",
            true,
            &config,
            CopyDependencies {
                copy_secret: |secret: zeroize::Zeroizing<String>, duration: Duration| {
                    copied.replace(Some((secret.to_string(), duration)));
                    Ok(cleanup)
                },
                wait_for_cleanup: |cleanup: mpsc::Receiver<()>| {
                    wait_started.send(()).unwrap();
                    cleanup.recv().unwrap();
                    cleanup_waited.set(true);
                    Ok(())
                },
                show_success: |_: String| {},
                show_status: |message: String| status_messages.borrow_mut().push(message),
            },
        )
        .unwrap();
        cleanup_worker.join().unwrap();

        assert_eq!(
            copied.into_inner(),
            Some(("configured secret".to_string(), Duration::from_secs(37)))
        );
        assert!(cleanup_waited.get());
        assert_eq!(
            status_messages.into_inner(),
            [
                "  Clipboard will be cleared in 37 seconds if it is unchanged.",
                "  Clipboard cleanup timer finished."
            ]
        );
    }
}
