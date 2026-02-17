use crate::error::{CryptoKeeperError, Result};
use crate::ui::borders::print_success;
use crate::vault::storage;

pub fn run(name: &str) -> Result<()> {
    let (mut vault, password) = storage::prompt_and_unlock()?;

    let entry = vault
        .find_entry_mut_by_id(name)
        .ok_or_else(|| CryptoKeeperError::EntryNotFound(name.to_string()))?;

    #[cfg(any(feature = "derive-eth", feature = "derive-btc", feature = "derive-sol"))]
    {
        use crate::crypto::derive;

        match derive::derive_address(&entry.secret, &entry.secret_type, &entry.network) {
            Ok(Some(address)) => {
                println!("  Derived address: {}", address);
                entry.public_address = Some(address);
                entry.updated_at = chrono::Utc::now();
                storage::save_vault(&vault, password.as_bytes())?;
                print_success("Address derived and saved.");
            }
            Ok(None) => {
                println!(
                    "  Address derivation not supported for {} / {}",
                    entry.secret_type, entry.network
                );
            }
            Err(e) => {
                return Err(CryptoKeeperError::DerivationFailed(e.to_string()));
            }
        }
    }

    #[cfg(not(any(feature = "derive-eth", feature = "derive-btc", feature = "derive-sol")))]
    {
        let _ = &password;
        let _ = entry.name.as_str();
        println!("  Address derivation features are not enabled.");
        println!("  Rebuild with: cargo build --features derive-eth,derive-btc,derive-sol");
    }

    Ok(())
}
