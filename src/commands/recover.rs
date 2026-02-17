use crate::config;
use crate::config::model::RECOVERY_QUESTIONS;
use crate::crypto::recovery;
use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::{print_error, print_success};
use crate::ui::theme::heading;
use crate::vault::storage;
use zeroize::Zeroizing;

const MAX_ATTEMPTS: u32 = 5;

pub fn run() -> Result<()> {
    let cfg = config::load_config()?;
    let recovery = cfg
        .recovery
        .as_ref()
        .ok_or(CryptoKeeperError::RecoveryNotConfigured)?;

    let question = RECOVERY_QUESTIONS
        .get(recovery.question_index as usize)
        .ok_or_else(|| CryptoKeeperError::RecoveryFailed("Invalid question index".into()))?;

    println!();
    println!("  {}", heading("Password Recovery"));
    println!();
    println!("  Recovery question:");
    println!("  {}", question);
    println!();

    let mut attempts = 0;
    let master_key = loop {
        attempts += 1;
        if attempts > MAX_ATTEMPTS {
            return Err(CryptoKeeperError::RecoveryFailed(
                "Too many failed attempts.".into(),
            ));
        }

        let answer = Zeroizing::new(
            rpassword::prompt_password("Your answer: ").map_err(CryptoKeeperError::Io)?,
        );

        let normalized = recovery::normalize_answer(&answer);
        if !recovery::verify_answer(&normalized, &recovery.answer_salt, &recovery.answer_hash)? {
            print_error("Incorrect answer. Try again.");
            continue;
        }

        // Decrypt master key blob
        match recovery::decrypt_recovery_blob(
            &recovery.master_key_blob,
            &recovery.master_key_blob_nonce,
            &recovery.master_key_blob_salt,
            &normalized,
        ) {
            Ok(key) => break key,
            Err(_) => {
                print_error("Failed to recover master key.");
                continue;
            }
        }
    };

    // Verify we can decrypt the vault with the recovered key
    let vault_path = storage::vault_path();
    let data = std::fs::read(&vault_path)?;
    let vault = storage::read_vault_with_key(&master_key, &data)?;

    println!();
    println!("  Recovery successful! Set a new master password.");
    println!();

    let new_password = crate::commands::passwd::prompt_new_password()?;

    // Re-encrypt vault with new password
    storage::save_vault(&vault, new_password.as_bytes())?;

    // Update recovery config with new master key
    let mut cfg = cfg;
    if let Some(ref recovery_cfg) = cfg.recovery {
        let normalized_answer = {
            let answer = Zeroizing::new(
                rpassword::prompt_password("Re-enter recovery answer to update recovery: ")
                    .map_err(CryptoKeeperError::Io)?,
            );
            recovery::normalize_answer(&answer)
        };

        let (blob, nonce, salt) =
            recovery::create_recovery_blob(&master_key, &normalized_answer)?;
        cfg.recovery = Some(config::RecoveryConfig {
            question_index: recovery_cfg.question_index,
            answer_hash: recovery_cfg.answer_hash.clone(),
            answer_salt: recovery_cfg.answer_salt.clone(),
            master_key_blob: blob,
            master_key_blob_nonce: nonce,
            master_key_blob_salt: salt,
        });
        config::save_config(&cfg)?;
    }

    print_success("Password changed and recovery updated successfully.");
    Ok(())
}
