use colored::Colorize;
use dialoguer::{Input, Select};
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{Config, EditMode, Editor};
use zeroize::Zeroizing;

use crate::commands;
use crate::crypto::kdf;
use crate::error::{CryptoKeeperError, Result};
use crate::ui;
use crate::ui::borders::{print_error, print_success};
use crate::vault::model::VaultData;
use crate::vault::storage;

/// Cached session state for the REPL — avoids Argon2 re-derivation on every command.
struct Session {
    vault: VaultData,
    password: Zeroizing<String>,
    key: Zeroizing<[u8; 32]>,
    salt: [u8; 32],
}

impl Session {
    /// Save the vault using the cached key (no Argon2 derivation).
    fn save(&self) -> Result<()> {
        storage::save_vault_with_key(&self.vault, &*self.key, &self.salt)
    }

    /// Re-derive key with a new password (for /passwd).
    fn change_password(&mut self, new_password: Zeroizing<String>) -> Result<()> {
        let salt = kdf::generate_salt();
        let key = kdf::derive_key(
            new_password.as_bytes(),
            &salt,
            kdf::DEFAULT_M_COST,
            kdf::DEFAULT_T_COST,
            kdf::DEFAULT_P_COST,
        )?;
        self.password = new_password;
        self.key = key;
        self.salt = salt;
        self.save()
    }
}

/// Command definitions for the interactive menu.
const MENU_COMMANDS: &[(&str, &str)] = &[
    ("list", "List all entries"),
    ("add", "Add a new entry"),
    ("view", "View entry details"),
    ("edit", "Edit an existing entry"),
    ("rename", "Rename an entry"),
    ("delete", "Delete an entry"),
    ("copy", "Copy secret to clipboard"),
    ("search", "Search entries"),
    ("export", "Export encrypted backup"),
    ("import", "Import from backup"),
    ("passwd", "Change master password"),
    ("help", "Show available commands"),
    ("quit", "Exit CryptoKeeper"),
];

/// Entry point for REPL mode (called when `cryptokeeper` is invoked with no args).
pub fn run() -> Result<()> {
    // Show header
    ui::setup_app_theme(true);

    // Check vault exists
    if !storage::vault_exists() {
        println!();
        println!(
            "{}",
            "No vault found. Run `cryptokeeper init` to create one.".yellow()
        );
        return Ok(());
    }

    // Authenticate
    let password = Zeroizing::new(
        rpassword::prompt_password("Master password: ").map_err(CryptoKeeperError::Io)?,
    );

    if password.is_empty() {
        return Err(CryptoKeeperError::EmptyPassword);
    }

    eprintln!("Unlocking vault...");
    let (vault, key, salt) = storage::unlock_vault_returning_key(password.as_bytes())?;

    let entry_count = vault.entries.len();
    let mut session = Session {
        vault,
        password,
        key,
        salt,
    };

    print_success(&format!(
        "Vault unlocked ({} {})",
        entry_count,
        if entry_count == 1 { "entry" } else { "entries" }
    ));
    println!();

    // Set up rustyline editor (no completer — we use dialoguer menus instead)
    let config = Config::builder()
        .edit_mode(EditMode::Emacs)
        .build();

    let mut rl: Editor<(), DefaultHistory> = Editor::with_config(config).map_err(|e| {
        CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
    })?;

    // Main REPL loop
    loop {
        let readline = rl.readline("cryptokeeper> ");
        match readline {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() || line == "/" {
                    // Show interactive command menu
                    match select_command() {
                        Ok(cmd) => {
                            let result = dispatch(&mut session, &cmd);
                            handle_result(result);
                        }
                        Err(CryptoKeeperError::Cancelled) => {}
                        Err(e) => print_error(&e.to_string()),
                    }
                    println!();
                    continue;
                }

                rl.add_history_entry(line).ok();

                let result = dispatch(&mut session, line);
                handle_result(result);
                println!();
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C: ignore, don't exit
                println!("{}", "  (Use /quit or Ctrl+D to exit)".dimmed());
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D: exit
                println!("Goodbye!");
                break;
            }
            Err(e) => {
                print_error(&format!("Input error: {e}"));
                break;
            }
        }
    }

    Ok(())
}

/// Handle a dispatch result, silently ignoring cancellations.
fn handle_result(result: Result<()>) {
    if let Err(e) = result {
        match e {
            CryptoKeeperError::Cancelled => {}
            _ => print_error(&e.to_string()),
        }
    }
}

/// Show an interactive command menu and return the selected command string.
fn select_command() -> Result<String> {
    let items: Vec<String> = MENU_COMMANDS
        .iter()
        .map(|(cmd, desc)| format!("/{:<10} {}", cmd, desc))
        .collect();

    let idx = Select::new()
        .with_prompt("Select a command")
        .items(&items)
        .default(0)
        .interact_opt()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    match idx {
        Some(i) => Ok(MENU_COMMANDS[i].0.to_string()),
        None => Err(CryptoKeeperError::Cancelled),
    }
}

/// Show an interactive entry selection menu and return a 1-based index string.
fn select_entry(vault: &VaultData) -> Result<String> {
    if vault.entries.is_empty() {
        print_error("No entries in vault. Use /add to create one.");
        return Err(CryptoKeeperError::Cancelled);
    }

    let items: Vec<String> = vault
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| format!("{}. {}", i + 1, e.name))
        .collect();

    let idx = Select::new()
        .with_prompt("Select an entry")
        .items(&items)
        .default(0)
        .interact_opt()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    match idx {
        Some(i) => Ok((i + 1).to_string()),
        None => Err(CryptoKeeperError::Cancelled),
    }
}

/// Prompt the user for a text input with a given prompt string.
fn prompt_input(prompt: &str) -> Result<String> {
    let value: String = Input::new()
        .with_prompt(prompt)
        .interact_text()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    if value.is_empty() {
        Err(CryptoKeeperError::Cancelled)
    } else {
        Ok(value)
    }
}

/// Parse and dispatch a REPL command.
fn dispatch(session: &mut Session, line: &str) -> Result<()> {
    // Parse: /command [args...]
    let line = if line.starts_with('/') {
        &line[1..]
    } else {
        line
    };

    let (cmd, args) = parse_command(line);

    match cmd {
        "help" | "h" | "?" => {
            print_help();
            Ok(())
        }
        "quit" | "exit" | "q" => {
            println!("Goodbye!");
            std::process::exit(0);
        }
        "list" | "ls" | "l" => {
            let filter = args.first().map(|s| s.as_str());
            commands::list::run_with_vault(&session.vault, filter)
        }
        "add" | "a" => {
            commands::add::run_with_vault(&mut session.vault)?;
            eprintln!("Saving vault...");
            session.save()?;
            Ok(())
        }
        "view" | "v" => {
            let name = if args.is_empty() {
                select_entry(&session.vault)?
            } else {
                args.join(" ")
            };
            commands::view::run_with_vault(&session.vault, &name)
        }
        "edit" | "e" => {
            let name = if args.is_empty() {
                select_entry(&session.vault)?
            } else {
                args.join(" ")
            };
            commands::edit::run_with_vault(&mut session.vault, &name)?;
            eprintln!("Saving vault...");
            session.save()?;
            Ok(())
        }
        "rename" | "rn" => {
            let old_name = if args.is_empty() {
                select_entry(&session.vault)?
            } else {
                args[0].clone()
            };
            let new_name = if args.len() >= 2 {
                args[1].clone()
            } else {
                prompt_input("New name")?
            };
            commands::rename::run_with_vault(&mut session.vault, &old_name, &new_name)?;
            eprintln!("Saving vault...");
            session.save()?;
            Ok(())
        }
        "delete" | "del" | "rm" => {
            let name = if args.is_empty() {
                select_entry(&session.vault)?
            } else {
                args.join(" ")
            };
            commands::delete::run_with_vault(&mut session.vault, &name)?;
            eprintln!("Saving vault...");
            session.save()?;
            Ok(())
        }
        "copy" | "cp" => {
            let name = if args.is_empty() {
                select_entry(&session.vault)?
            } else {
                args.join(" ")
            };
            commands::copy::run_with_vault(&session.vault, &name, false)
        }
        "search" | "s" | "find" => {
            let query = if args.is_empty() {
                prompt_input("Search query")?
            } else {
                args.join(" ")
            };
            commands::search::run_with_vault(&session.vault, &query)
        }
        "export" => {
            let file = if args.is_empty() {
                prompt_input("Export file path")?
            } else {
                args.join(" ")
            };
            commands::export::run_with_vault(&session.vault, &file)
        }
        "import" => {
            let file = if args.is_empty() {
                prompt_input("Import file path")?
            } else {
                args.join(" ")
            };
            let modified = commands::import::run_with_vault(&mut session.vault, &file)?;
            if modified {
                eprintln!("Saving vault...");
                session.save()?;
            }
            Ok(())
        }
        "passwd" | "password" => {
            let new_password = commands::passwd::prompt_new_password()?;
            eprintln!("Re-encrypting vault with new password...");
            session.change_password(new_password)?;
            print_success("Master password changed successfully.");
            Ok(())
        }
        _ => {
            print_error(&format!(
                "Unknown command: /{}. Type /help for available commands.",
                cmd
            ));
            Ok(())
        }
    }
}

/// Parse a command line into (command, args), handling quoted arguments.
fn parse_command(line: &str) -> (&str, Vec<String>) {
    let line = line.trim();
    if line.is_empty() {
        return ("", vec![]);
    }

    // Split on first space to get the command
    let (cmd, rest) = match line.find(' ') {
        Some(pos) => (&line[..pos], line[pos + 1..].trim()),
        None => (line, ""),
    };

    let args = if rest.is_empty() {
        vec![]
    } else {
        parse_args(rest)
    };

    (cmd, args)
}

/// Parse arguments, respecting quoted strings.
fn parse_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote_char = '"';

    for ch in input.chars() {
        if in_quote {
            if ch == quote_char {
                in_quote = false;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            in_quote = true;
            quote_char = ch;
        } else if ch == ' ' {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

fn print_help() {
    println!();
    println!("  {}", "Available commands:".bold());
    println!();
    println!("    {}   List all entries (filter: privatekey, seedphrase, password)", "/list [filter]".cyan());
    println!("    {}             Add a new entry", "/add".cyan());
    println!("    {}      View entry details", "/view [name|#]".cyan());
    println!("    {}      Edit an existing entry", "/edit [name|#]".cyan());
    println!("    {} Rename an entry", "/rename [old] [new]".cyan());
    println!("    {}    Delete an entry", "/delete [name|#]".cyan());
    println!("    {}      Copy secret to clipboard (auto-clears 10s)", "/copy [name|#]".cyan());
    println!("    {}   Search entries", "/search [query]".cyan());
    println!("    {}    Export encrypted backup", "/export [file]".cyan());
    println!("    {}    Import from encrypted backup", "/import [file]".cyan());
    println!("    {}            Change master password", "/passwd".cyan());
    println!("    {}              Show this help", "/help".cyan());
    println!("    {}              Exit CryptoKeeper", "/quit".cyan());
    println!();
    println!("  {} Commands without arguments show an interactive menu.", "Tip:".dimmed());
    println!("  {} Type {} or press Enter for the command menu.", "".dimmed(), "/".cyan());
}
