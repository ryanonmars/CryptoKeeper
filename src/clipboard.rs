use std::thread;
use std::time::Duration;

use arboard::Clipboard;

use crate::error::{CryptoKeeperError, Result};

/// Copy text to clipboard and spawn a background thread to clear it after `clear_after` seconds.
pub fn copy_and_clear(text: &str, clear_after_secs: u64) -> Result<()> {
    let mut clipboard =
        Clipboard::new().map_err(|e| CryptoKeeperError::Clipboard(e.to_string()))?;

    clipboard
        .set_text(text)
        .map_err(|e| CryptoKeeperError::Clipboard(e.to_string()))?;

    // Spawn a background thread to clear the clipboard
    let duration = Duration::from_secs(clear_after_secs);
    thread::spawn(move || {
        thread::sleep(duration);
        if let Ok(mut cb) = Clipboard::new() {
            let _ = cb.set_text(String::new());
        }
    });

    Ok(())
}
