pub mod borders;
pub mod header;
pub mod theme;
pub mod terminal;

use std::io::IsTerminal;

/// Get the current terminal width, with a fallback of 80.
pub fn get_terminal_width() -> u16 {
    let term = console::Term::stdout();
    let (_, cols) = term.size();
    if cols == 0 { 80 } else { cols }
}

/// Check if stdout is connected to an interactive terminal.
pub fn is_interactive() -> bool {
    std::io::stdout().is_terminal()
}

/// Set up the app theme: clear screen, set window title, print header.
pub fn setup_app_theme(clear: bool) {
    if !is_interactive() {
        return;
    }
    theme::set_title("CryptoKeeper");
    if clear {
        theme::clear_screen();
    }
    header::print_header();
}
