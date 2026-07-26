use crate::config;
use crate::error::Result;
use crate::ui::borders::print_success;

pub fn run(show: bool, clipboard_timeout: Option<u64>) -> Result<()> {
    let cfg = config::load_config()?;

    if show || clipboard_timeout.is_none() {
        println!();
        println!("  TermKey Configuration");
        println!("  ─────────────────────────");
        println!("  Vault path:         {}", cfg.vault_path);
        println!(
            "  Clipboard timeout:  {} seconds",
            cfg.clipboard_timeout_secs
        );
        println!("  First run complete: {}", cfg.first_run_complete);
        println!(
            "  Recovery phrase:    {}",
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
        config::update_config(|config| {
            config.clipboard_timeout_secs = timeout;
            Ok(())
        })?;
        print_success(&format!("Clipboard timeout set to {} seconds.", timeout));
    }

    Ok(())
}
