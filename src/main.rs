mod cli;
mod clipboard;
mod commands;
mod crypto;
mod error;
mod ui;
mod vault;

use clap::Parser;

use cli::{Cli, Commands};
use crypto::secure;

fn main() {
    secure::harden_process();

    ui::setup_app_theme(true);

    let cli = Cli::parse();

    let result = match cli.command {
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
        Commands::Export { ref file } => commands::export::run(file),
        Commands::Import { ref file } => commands::import::run(file),
        Commands::Passwd => commands::passwd::run(),
    };

    if let Err(e) = result {
        ui::borders::print_error(&e.to_string());
        std::process::exit(1);
    }
}
