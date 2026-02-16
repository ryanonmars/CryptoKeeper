use colored::{ColoredString, Colorize};

use std::io::{self, Write};

pub fn set_title(title: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(b"\x1b]0;");
    let _ = out.write_all(title.as_bytes());
    let _ = out.write_all(b"\x07");
}

pub fn clear_screen() {
    use crossterm::{execute, terminal::{Clear, ClearType}, cursor::MoveTo};
    let _ = execute!(
        io::stdout(),
        Clear(ClearType::All),
        MoveTo(0, 0)
    );
}

pub fn heading(text: &str) -> ColoredString {
    text.bold()
}

pub fn dim_border(ch: &str) -> ColoredString {
    ch.cyan().dimmed()
}
