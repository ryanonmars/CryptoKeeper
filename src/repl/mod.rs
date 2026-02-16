use colored::Colorize;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use dialoguer::{Input, Select};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, ListItem, Paragraph, Row, Table, Wrap},
};
use std::time::Duration;
use zeroize::Zeroizing;

use crate::commands;
use crate::crypto::kdf;
use crate::error::{CryptoKeeperError, Result};
use crate::ui;
use crate::ui::borders::{print_error, print_success};
use crate::ui::header::render_header;
use crate::vault::model::{EntryMeta, SecretType, VaultData};
use crate::vault::storage;

mod input;
use input::{InputResult, PasswordInput};

struct Session {
    vault: VaultData,
    password: Zeroizing<String>,
    key: Zeroizing<[u8; 32]>,
    salt: [u8; 32],
}

impl Session {
    fn save(&self) -> Result<()> {
        storage::save_vault_with_key(&self.vault, &*self.key, &self.salt)
    }

    #[allow(dead_code)]
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

enum ContentView {
    Input {
        buffer: String,
        prompt: String,
    },
    List {
        entries: Vec<(usize, EntryMeta)>,
        filter: Option<String>,
    },
    Message(String),
}

struct AppState {
    content: ContentView,
    show_completions: bool,
    selected_completion: usize,
    commands: Vec<(&'static str, &'static str)>,
}

pub fn run() -> Result<()> {
    if !storage::vault_exists() {
        ui::setup_app_theme(true);
        println!();
        println!(
            "{}",
            "No vault found. Run `cryptokeeper init` to create one.".yellow()
        );
        return Ok(());
    }

    let mut terminal = ui::terminal::init().map_err(CryptoKeeperError::Io)?;
    
    let password = {
        let mut password_input = PasswordInput::new("Master password: ");
        let result = loop {
            terminal.draw(|frame| {
                let area = frame.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(10),
                        Constraint::Length(1),
                        Constraint::Min(0),
                    ])
                    .split(area);

                render_header(frame, chunks[0]);
                password_input.render(frame, chunks[1]);
            }).map_err(CryptoKeeperError::Io)?;

            if event::poll(Duration::from_millis(100)).map_err(CryptoKeeperError::Io)? {
                if let Event::Key(key_event) = event::read().map_err(CryptoKeeperError::Io)? {
                    match password_input.handle_key(key_event.code, key_event.modifiers) {
                        InputResult::Done(password) => break Ok(password),
                        InputResult::Cancelled => break Err(CryptoKeeperError::Cancelled),
                        InputResult::Continue => {}
                    }
                }
            }
        };
        
        ui::terminal::restore().map_err(CryptoKeeperError::Io)?;
        result?
    };

    let password = Zeroizing::new(password);

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

    let mut terminal = ui::terminal::init().map_err(CryptoKeeperError::Io)?;
    
    let mut app_state = AppState {
        content: ContentView::Input {
            buffer: String::new(),
            prompt: "cryptokeeper> ".to_string(),
        },
        show_completions: false,
        selected_completion: 0,
        commands: MENU_COMMANDS.to_vec(),
    };

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            
            let constraints = match &app_state.content {
                ContentView::List { .. } => {
                    vec![
                        Constraint::Length(14),
                        Constraint::Min(10),
                        Constraint::Length(0),
                    ]
                }
                _ => {
                    vec![
                        Constraint::Length(14),
                        Constraint::Length(3),
                        Constraint::Length(if app_state.show_completions { 10 } else { 0 }),
                        Constraint::Min(0),
                    ]
                }
            };
            
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(area);

            render_header(frame, chunks[0]);
            
            if matches!(&app_state.content, ContentView::Input { .. }) {
                render_content(frame, chunks[1], &app_state);
                if app_state.show_completions && chunks[2].height > 0 {
                    render_completions(frame, chunks[2], &app_state);
                }
            } else {
                render_content(frame, chunks[1], &app_state);
            }
        }).map_err(CryptoKeeperError::Io)?;

        if event::poll(Duration::from_millis(100)).map_err(CryptoKeeperError::Io)? {
            if let Event::Key(key_event) = event::read().map_err(CryptoKeeperError::Io)? {
                if matches!(app_state.content, ContentView::List { .. } | ContentView::Message(_)) {
                    if key_event.code == KeyCode::Esc || key_event.code == KeyCode::Enter {
                        app_state.content = ContentView::Input {
                            buffer: String::new(),
                            prompt: "cryptokeeper> ".to_string(),
                        };
                        app_state.show_completions = false;
                        continue;
                    }
                }

                if let ContentView::Input { buffer, .. } = &mut app_state.content {
                    match key_event.code {
                        KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                            ui::terminal::restore().map_err(CryptoKeeperError::Io)?;
                            println!("Goodbye!");
                            break;
                        }
                        KeyCode::Char('d') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                            if buffer.is_empty() {
                                ui::terminal::restore().map_err(CryptoKeeperError::Io)?;
                                println!("Goodbye!");
                                break;
                            }
                        }
                        KeyCode::Char('/') => {
                            buffer.push('/');
                            app_state.show_completions = true;
                            app_state.selected_completion = 0;
                        }
                        KeyCode::Char(c) => {
                            buffer.push(c);
                            if buffer.starts_with('/') {
                                app_state.show_completions = true;
                                app_state.selected_completion = 0;
                            }
                        }
                        KeyCode::Backspace => {
                            buffer.pop();
                            if buffer.starts_with('/') {
                                app_state.show_completions = true;
                            } else {
                                app_state.show_completions = false;
                            }
                        }
                        KeyCode::Down if app_state.show_completions => {
                            let matches = get_matching_commands(&app_state.commands, buffer);
                            if !matches.is_empty() {
                                app_state.selected_completion = (app_state.selected_completion + 1) % matches.len();
                            }
                        }
                        KeyCode::Up if app_state.show_completions => {
                            let matches = get_matching_commands(&app_state.commands, buffer);
                            if !matches.is_empty() {
                                app_state.selected_completion = if app_state.selected_completion == 0 {
                                    matches.len() - 1
                                } else {
                                    app_state.selected_completion - 1
                                };
                            }
                        }
                        KeyCode::Tab if app_state.show_completions => {
                            let matches = get_matching_commands(&app_state.commands, buffer);
                            if !matches.is_empty() && app_state.selected_completion < matches.len() {
                                *buffer = format!("/{}", matches[app_state.selected_completion].0);
                            }
                        }
                        KeyCode::Enter => {
                            if app_state.show_completions {
                                let matches = get_matching_commands(&app_state.commands, buffer);
                                if !matches.is_empty() && app_state.selected_completion < matches.len() {
                                    *buffer = format!("/{}", matches[app_state.selected_completion].0);
                                }
                            }
                            
                            let line = buffer.trim().to_string();
                            app_state.show_completions = false;
                            
                        if line.is_empty() || line == "/" {
                            ui::terminal::exit_raw_mode_temporarily().map_err(CryptoKeeperError::Io)?;
                            match select_command() {
                                Ok(cmd) => {
                                    if !dispatch_in_tui(&mut session, &cmd, &mut app_state) {
                                        dispatch_external(&mut session, &cmd);
                                        app_state.content = ContentView::Input {
                                            buffer: String::new(),
                                            prompt: "cryptokeeper> ".to_string(),
                                        };
                                    }
                                }
                                Err(_) => {}
                            }
                            ui::terminal::reenter_raw_mode().map_err(CryptoKeeperError::Io)?;
                            terminal.clear().map_err(CryptoKeeperError::Io)?;
                        } else if !dispatch_in_tui(&mut session, &line, &mut app_state) {
                            ui::terminal::exit_raw_mode_temporarily().map_err(CryptoKeeperError::Io)?;
                            dispatch_external(&mut session, &line);
                            app_state.content = ContentView::Input {
                                buffer: String::new(),
                                prompt: "cryptokeeper> ".to_string(),
                            };
                            ui::terminal::reenter_raw_mode().map_err(CryptoKeeperError::Io)?;
                            terminal.clear().map_err(CryptoKeeperError::Io)?;
                        }
                        }
                        KeyCode::Esc if app_state.show_completions => {
                            app_state.show_completions = false;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

fn render_content(frame: &mut ratatui::Frame, area: Rect, app_state: &AppState) {
    match &app_state.content {
        ContentView::Input { buffer, prompt } => {
            let input_text = format!("{}{}", prompt, buffer);
            
            let paragraph = Paragraph::new(input_text)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title(" Console ")
                    .border_style(Style::default().fg(Color::Cyan)));
            frame.render_widget(paragraph, area);
        }
        ContentView::Message(msg) => {
            let paragraph = Paragraph::new(msg.as_str())
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title(" Output ")
                    .border_style(Style::default().fg(Color::Cyan)))
                .wrap(Wrap { trim: false });
            frame.render_widget(paragraph, area);
        }
        ContentView::List { entries, filter } => {
            let title = match filter {
                Some(f) => format!(" {} ({} entries) [ESC to close] ", f, entries.len()),
                None => format!(" Vault ({} entries) [ESC to close] ", entries.len()),
            };

            let header_cells = ["#", "NAME", "TYPE", "NETWORK"]
                .iter()
                .map(|h| Cell::from(*h).style(Style::default().add_modifier(Modifier::BOLD)));
            let header = Row::new(header_cells).height(1);

            let rows: Vec<Row> = entries.iter().map(|(i, entry)| {
                let type_str = match entry.secret_type {
                    SecretType::PrivateKey => "Private Key",
                    SecretType::SeedPhrase => "Seed Phrase",
                    SecretType::Password => "Password",
                };
                let type_color = match entry.secret_type {
                    SecretType::PrivateKey => Color::Yellow,
                    SecretType::SeedPhrase => Color::Magenta,
                    SecretType::Password => Color::Green,
                };

                let network = if entry.network.is_empty() { "-" } else { &entry.network };

                Row::new(vec![
                    Cell::from(format!("{}", i + 1)).style(Style::default().fg(Color::DarkGray)),
                    Cell::from(entry.name.as_str()).style(Style::default().fg(Color::Cyan)),
                    Cell::from(type_str).style(Style::default().fg(type_color)),
                    Cell::from(network),
                ])
            }).collect();

            let widths = [
                Constraint::Length(5),
                Constraint::Percentage(50),
                Constraint::Length(15),
                Constraint::Percentage(35),
            ];

            let table = Table::new(rows, widths)
                .header(header)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .title_style(Style::default().fg(Color::Cyan))
                        .border_style(Style::default().fg(Color::Cyan))
                );

            frame.render_widget(table, area);
        }
    }
}

fn render_completions(frame: &mut ratatui::Frame, area: Rect, app_state: &AppState) {
    if let ContentView::Input { buffer, .. } = &app_state.content {
        let matches = get_matching_commands(&app_state.commands, buffer);
        if matches.is_empty() {
            return;
        }

        let visible_height = area.height.saturating_sub(2).max(1) as usize;
        let selected = app_state.selected_completion;
        
        let scroll_offset = if selected >= visible_height {
            selected - visible_height + 1
        } else {
            0
        };

        let visible_matches: Vec<_> = matches
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .collect();

        let items: Vec<ListItem> = visible_matches
            .iter()
            .map(|(i, (cmd, desc))| {
                let prefix = if *i == selected {
                    "▸ "
                } else {
                    "  "
                };
                let content = format!("{}/{:<10} {}", prefix, cmd, desc);
                let style = if *i == selected {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                };
                ListItem::new(content).style(style)
            })
            .collect();

        let title = if matches.len() > visible_height {
            format!(" Commands ({}/{}) ", selected + 1, matches.len())
        } else {
            " Commands ".to_string()
        };

        let list = ratatui::widgets::List::new(items)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::DarkGray)));
        
        frame.render_widget(list, area);
    }
}

fn get_matching_commands<'a>(commands: &'a [(&'a str, &'a str)], input: &str) -> Vec<(&'a str, &'a str)> {
    if !input.starts_with('/') {
        return vec![];
    }

    let prefix = &input[1..];
    commands
        .iter()
        .filter(|(cmd, _)| cmd.starts_with(prefix))
        .copied()
        .collect()
}

fn dispatch_in_tui(session: &mut Session, line: &str, app_state: &mut AppState) -> bool {
    let line = if line.starts_with('/') { &line[1..] } else { line };
    let (cmd, args) = parse_command(line);

    match cmd {
        "list" | "ls" | "l" => {
            let filter = args.first().map(|s| s.to_string());
            let meta = session.vault.metadata();
            
            if meta.is_empty() {
                app_state.content = ContentView::Message("No entries in vault. Use /add to create one.".to_string());
                return true;
            }

            let type_filter = filter.as_ref().and_then(|f| match f.to_lowercase().as_str() {
                "privatekey" | "private-key" | "private_key" => Some(SecretType::PrivateKey),
                "seedphrase" | "seed-phrase" | "seed_phrase" => Some(SecretType::SeedPhrase),
                "password" | "passwords" => Some(SecretType::Password),
                _ => None,
            });

            let filtered: Vec<(usize, EntryMeta)> = meta
                .iter()
                .enumerate()
                .filter(|(_, e)| type_filter.as_ref().map_or(true, |ft| e.secret_type == *ft))
                .map(|(i, e)| (i, e.clone()))
                .collect();

            if filtered.is_empty() {
                app_state.content = ContentView::Message("No entries match the filter.".to_string());
            } else {
                app_state.content = ContentView::List { entries: filtered, filter };
            }
            true
        }
        "help" | "h" | "?" => {
            let help_lines = vec![
                "Available commands:",
                "",
                "  /list [filter]    List entries (filter: privatekey, seedphrase, password)",
                "  /add              Add new entry",
                "  /view [name|#]    View entry details",
                "  /edit [name|#]    Edit entry",
                "  /delete [name|#]  Delete entry",
                "  /copy [name|#]    Copy to clipboard",
                "  /search [query]   Search entries",
                "  /help             Show this help",
                "  /quit             Exit",
                "",
                "Press ENTER or ESC to return to input",
            ];
            app_state.content = ContentView::Message(help_lines.join("\n"));
            true
        }
        "quit" | "exit" | "q" => {
            std::process::exit(0);
        }
        _ => false,
    }
}

fn dispatch_external(session: &mut Session, line: &str) {
    let line = if line.starts_with('/') { &line[1..] } else { line };
    let (cmd, args) = parse_command(line);

    let result = match cmd {
        "add" | "a" => {
            commands::add::run_with_vault(&mut session.vault)
                .and_then(|_| { eprintln!("Saving vault..."); session.save() })
        }
        "view" | "v" => {
            let name = if args.is_empty() {
                select_entry(&session.vault).ok()
            } else {
                Some(args.join(" "))
            };
            name.map(|n| commands::view::run_with_vault(&session.vault, &n)).unwrap_or(Ok(()))
        }
        "edit" | "e" => {
            let name = if args.is_empty() {
                select_entry(&session.vault).ok()
            } else {
                Some(args.join(" "))
            };
            name.and_then(|n| {
                commands::edit::run_with_vault(&mut session.vault, &n).ok()?;
                eprintln!("Saving vault...");
                session.save().ok()
            });
            Ok(())
        }
        "delete" | "del" | "rm" => {
            let name = if args.is_empty() {
                select_entry(&session.vault).ok()
            } else {
                Some(args.join(" "))
            };
            name.and_then(|n| {
                commands::delete::run_with_vault(&mut session.vault, &n).ok()?;
                eprintln!("Saving vault...");
                session.save().ok()
            });
            Ok(())
        }
        "copy" | "cp" => {
            let name = if args.is_empty() {
                select_entry(&session.vault).ok()
            } else {
                Some(args.join(" "))
            };
            name.map(|n| commands::copy::run_with_vault(&session.vault, &n, false)).unwrap_or(Ok(()))
        }
        "search" | "s" | "find" => {
            let query = if args.is_empty() {
                prompt_input("Search query").ok()
            } else {
                Some(args.join(" "))
            };
            query.map(|q| commands::search::run_with_vault(&session.vault, &q)).unwrap_or(Ok(()))
        }
        _ => {
            print_error(&format!("Unknown command: /{}", cmd));
            Ok(())
        }
    };

    if let Err(e) = result {
        if !matches!(e, CryptoKeeperError::Cancelled) {
            print_error(&e.to_string());
        }
    }
}

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

fn select_entry(vault: &VaultData) -> Result<String> {
    if vault.entries.is_empty() {
        print_error("No entries in vault. Use /add to create one.");
        return Err(CryptoKeeperError::Cancelled);
    }

    let mut items: Vec<String> = vault
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| format!("{}. {}", i + 1, e.name))
        .collect();
    
    items.push("Exit".to_string());

    let idx = Select::new()
        .with_prompt("Select an entry")
        .items(&items)
        .default(0)
        .interact_opt()
        .map_err(|e| CryptoKeeperError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    match idx {
        Some(i) if i >= vault.entries.len() => Err(CryptoKeeperError::Cancelled),
        Some(i) => Ok((i + 1).to_string()),
        None => Err(CryptoKeeperError::Cancelled),
    }
}

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

fn parse_command(line: &str) -> (&str, Vec<String>) {
    let line = line.trim();
    if line.is_empty() {
        return ("", vec![]);
    }

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
