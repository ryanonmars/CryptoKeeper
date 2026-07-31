use clap::Parser;

use termkey::cli::{Cli, Commands};
use termkey::crypto::secure;
use termkey::{apply_configured_vault_dir_override, commands, repl, ui, vault};

fn main() {
    secure::harden_process();
    vault::storage::migrate_vault_if_needed();

    let cli = Cli::parse();

    // In REPL mode, the REPL handles its own header display after auth.
    // In CLI mode, clear screen and show header immediately.
    if cli.command.is_some() {
        ui::setup_app_theme(true);
    }

    apply_configured_vault_dir_override();

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
            Commands::Update => commands::update_cmd::run(),
            Commands::Uninstall { yes } => commands::uninstall::run(yes),
            Commands::Config {
                show,
                ref clipboard_timeout,
            } => commands::config_cmd::run(show, *clipboard_timeout),
            Commands::Derive { ref name } => commands::derive::run(name),
            Commands::Browser { ref command } => commands::browser::run(command),
        },
    };

    if let Err(e) = result {
        ui::borders::print_error(&e.to_string() as &str);
        std::process::exit(1);
    }
}
