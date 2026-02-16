use std::io::{self, Write};

use colored::Colorize;

use super::get_terminal_width;

const CURSOR: &str = "▌ ";

pub fn read_stylized_line() -> io::Result<String> {
    let width = get_terminal_width() as usize;
    let line = "_".repeat(width);
    println!();
    println!("{}", line.cyan());
    print!("{}", CURSOR);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    println!("{}", line.cyan());
    Ok(input)
}
