use colored::{ColoredString, Colorize};

use std::io::{self, Write};

const CLEAR: &str = "\x1b[2J\x1b[H";

pub fn set_title(title: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(b"\x1b]0;");
    let _ = out.write_all(title.as_bytes());
    let _ = out.write_all(b"\x07");
}

pub fn clear_screen() {
    let mut out = io::stdout();
    let _ = out.write_all(CLEAR.as_bytes());
    let _ = out.flush();
}

pub fn heading(text: &str) -> ColoredString {
    text.bold()
}

pub fn dim_border(ch: &str) -> ColoredString {
    ch.cyan().dimmed()
}
