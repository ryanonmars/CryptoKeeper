pub mod cli;
pub mod clipboard;
pub mod commands;
pub mod config;
pub mod crypto;
pub mod error;
pub mod links;
pub mod native;
pub mod repl;
pub mod ui;
pub mod update;
pub mod vault;

use std::path::Path;

pub fn apply_configured_vault_dir_override() {
    if let Ok(cfg) = config::load_config() {
        let default_cfg = config::Config::default();
        if cfg.vault_path != default_cfg.vault_path {
            if let Some(parent) = Path::new(&cfg.vault_path).parent() {
                std::env::set_var("TERMKEY_VAULT_DIR", parent);
            }
        }
    }
}
