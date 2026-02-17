use crate::config;
use crate::error::Result;
use crate::ui::borders::print_success;

pub fn run(show: bool, clipboard_timeout: Option<u64>) -> Result<()> {
    let mut cfg = config::load_config()?;

    if show || clipboard_timeout.is_none() {
        println!();
        println!("  CryptoKeeper Configuration");
        println!("  ─────────────────────────");
        println!("  Vault path:         {}", cfg.vault_path);
        println!("  Clipboard timeout:  {} seconds", cfg.clipboard_timeout_secs);
        println!("  First run complete: {}", cfg.first_run_complete);
        println!(
            "  Recovery question:  {}",
            if cfg.recovery.is_some() {
                "Configured"
            } else {
                "Not set"
            }
        );
        println!();
        return Ok(());
    }

    if let Some(timeout) = clipboard_timeout {
        cfg.clipboard_timeout_secs = timeout;
        config::save_config(&cfg)?;
        print_success(&format!("Clipboard timeout set to {} seconds.", timeout));
    }

    Ok(())
}
