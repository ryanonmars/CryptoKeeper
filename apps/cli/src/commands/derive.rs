use crate::error::{Result, TermKeyError};
#[cfg(any(feature = "derive-eth", feature = "derive-btc", feature = "derive-sol"))]
use crate::ui::borders::print_success;
#[cfg(any(feature = "derive-eth", feature = "derive-btc", feature = "derive-sol"))]
use crate::vault::model::Entry;
use crate::vault::session::VaultSession;

pub fn run(name: &str) -> Result<()> {
    let session = VaultSession::prompt_and_open()?.session;

    let entry = session
        .vault
        .find_entry_by_id(name)
        .cloned()
        .ok_or_else(|| TermKeyError::EntryNotFound(name.to_string()))?;

    #[cfg(any(feature = "derive-eth", feature = "derive-btc", feature = "derive-sol"))]
    {
        let mut session = session;
        let secondary_password = super::prompt_secondary_password(&entry)?;
        match derive_entry_address(
            &entry,
            secondary_password
                .as_ref()
                .map(|password| password.as_str()),
        ) {
            Ok(Some(address)) => {
                println!("  Derived address: {}", address);
                let original_name = entry.name.clone();
                let mut updated = entry;
                updated.public_address = Some(address);
                updated.updated_at = chrono::Utc::now();
                session.vault.replace_entry_authorized(
                    &original_name,
                    updated,
                    secondary_password
                        .as_ref()
                        .map(|password| password.as_str()),
                )?;
                session.save()?;
                print_success("Address derived and saved.");
            }
            Ok(None) => {
                println!(
                    "  Address derivation not supported for {} / {}",
                    entry.secret_type, entry.network
                );
            }
            Err(e) => {
                return Err(TermKeyError::DerivationFailed(e.to_string()));
            }
        }
    }

    #[cfg(not(any(feature = "derive-eth", feature = "derive-btc", feature = "derive-sol")))]
    {
        let _ = entry.name.as_str();
        println!("  Address derivation features are not enabled.");
        println!("  Rebuild with: cargo build --features derive-eth,derive-btc,derive-sol");
    }

    Ok(())
}

#[cfg(any(feature = "derive-eth", feature = "derive-btc", feature = "derive-sol"))]
fn derive_entry_address(entry: &Entry, secondary_password: Option<&str>) -> Result<Option<String>> {
    let secret = entry.reveal_secret(secondary_password)?;
    crate::crypto::derive::derive_address(&secret, &entry.secret_type, &entry.network)
}

#[cfg(all(
    test,
    any(feature = "derive-eth", feature = "derive-btc", feature = "derive-sol")
))]
mod tests {
    use super::derive_entry_address;
    use crate::commands::test_support::protected_entry;
    use crate::vault::model::SecretType;

    #[test]
    fn derive_protected_entry_uses_decrypted_value() {
        let mut entry = protected_entry(
            "Protected key",
            "4c0883a69102937d6231471b5dbb6204fe5129617082794b824f4b49d5b5f0c1",
            "view-pass",
        );
        entry.secret_type = SecretType::PrivateKey;
        entry.network = "Ethereum".to_string();

        let address = derive_entry_address(&entry, Some("view-pass"))
            .unwrap()
            .expect("Ethereum derivation should be supported");

        assert!(address.starts_with("0x"));
    }
}
