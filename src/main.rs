mod cli;
mod clipboard;
mod commands;
mod config;
mod crypto;
mod error;
mod repl;
mod ui;
mod vault;

use clap::Parser;

use cli::{Cli, Commands};
use crypto::secure;

fn main() {
    secure::harden_process();

    let cli = Cli::parse();

    // In REPL mode, the REPL handles its own header display after auth.
    // In CLI mode, clear screen and show header immediately.
    if cli.command.is_some() {
        ui::setup_app_theme(true);
    }

    // Load config and set vault path env var if customized
    if let Ok(cfg) = config::load_config() {
        // Only override if user has a custom vault path (not the default)
        let default_cfg = config::Config::default();
        if cfg.vault_path != default_cfg.vault_path {
            if let Some(parent) = std::path::Path::new(&cfg.vault_path).parent() {
                std::env::set_var("CRYPTOKEEPER_VAULT_DIR", parent);
            }
        }
    }

    let result = match cli.command {
        None => repl::run(),
        Some(cmd) => match cmd {
            Commands::Init => commands::init::run(),
            Commands::Add => commands::add::run(),
            Commands::List { ref filter } => commands::list::run(filter.as_deref()),
            Commands::View { ref name } => commands::view::run(name),
            Commands::Edit { ref name } => commands::edit::run(name),
            Commands::Rename {
                ref old_name,
                ref new_name,
            } => commands::rename::run(old_name, new_name),
            Commands::Delete { ref name } => commands::delete::run(name),
            Commands::Copy { ref name } => commands::copy::run(name),
            Commands::Search { ref query } => commands::search::run(query),
            Commands::Export { ref directory } => commands::export::run(directory),
            Commands::Import { ref file } => commands::import::run(file),
            Commands::Passwd => commands::passwd::run(),
            Commands::Recover => commands::recover::run(),
            Commands::Config {
                show,
                ref clipboard_timeout,
            } => commands::config_cmd::run(show, *clipboard_timeout),
            Commands::Derive { ref name } => commands::derive::run(name),
        },
    };

    if let Err(e) = result {
        ui::borders::print_error(&e.to_string() as &str);
        std::process::exit(1);
    }
}
