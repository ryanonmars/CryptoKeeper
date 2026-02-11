mod cli;
mod clipboard;
mod commands;
mod crypto;
mod error;
mod vault;

use clap::Parser;
use colored::Colorize;

use cli::{Cli, Commands};
use crypto::secure;

fn main() {
    // Harden the process (disable core dumps, ptrace)
    secure::harden_process();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => commands::init::run(),
        Commands::Add => commands::add::run(),
        Commands::List => commands::list::run(),
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
        eprintln!("{} {}", "Error:".red().bold(), e);
        std::process::exit(1);
    }
}
