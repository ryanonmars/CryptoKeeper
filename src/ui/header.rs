use colored::Colorize;
use unicode_width::UnicodeWidthStr;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::get_terminal_width;
use super::theme::dim_border;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Measure display width of a string, ignoring ANSI escape codes.
fn display_width(s: &str) -> usize {
    let stripped = console::strip_ansi_codes(s);
    UnicodeWidthStr::width(stripped.as_ref())
}

/// Print the application header, scaled to terminal width.
pub fn print_header() {
    let width = get_terminal_width() as usize;

    if width >= 70 {
        print_wide_header(width);
    } else if width >= 50 {
        print_medium_header(width);
    } else {
        print_narrow_header(width);
    }
    println!();
}

fn print_wide_header(width: usize) {
    let inner = width.saturating_sub(4); // "│ " + " │"

    // Block letter art for CRYPTO
    let crypto_lines = [
        " ██████╗██████╗ ██╗   ██╗██████╗ ████████╗ ██████╗ ",
        "██╔════╝██╔══██╗╚██╗ ██╔╝██╔══██╗╚══██╔══╝██╔═══██╗",
        "██║     ██████╔╝ ╚████╔╝ ██████╔╝   ██║   ██║   ██║",
        "██║     ██╔══██╗  ╚██╔╝  ██╔═══╝    ██║   ██║   ██║",
        "╚██████╗██║  ██║   ██║   ██║        ██║   ╚██████╔╝",
        " ╚═════╝╚═╝  ╚═╝   ╚═╝   ╚═╝        ╚═╝    ╚═════╝ ",
    ];

    // Block letter art for KEEPER (same style)
    let keeper_lines = [
        "██╗  ██╗███████╗███████╗██████╗ ███████╗██████╗ ",
        "██║ ██╔╝██╔════╝██╔════╝██╔══██╗██╔════╝██╔══██╗",
        "█████╔╝ █████╗  █████╗  ██████╔╝█████╗  ██████╔╝",
        "██╔═██╗ ██╔══╝  ██╔══╝  ██╔═══╝ ██╔══╝  ██╔══██╗",
        "██║  ██╗███████╗███████╗██║     ███████╗██║  ██║",
        "╚═╝  ╚═╝╚══════╝╚══════╝╚═╝     ╚══════╝╚═╝  ╚═╝",
    ];

    let version_line = format!("v{}", VERSION);
    let tagline = "Encrypted vault for crypto keys & seed phrases";

    // Top border
    let title_embed = format!(" CryptoKeeper ");
    let title_dw = display_width(&title_embed);
    let remaining = (inner + 2).saturating_sub(title_dw + 1);
    println!(
        "{}{}{}{}{}",
        dim_border("┌"),
        dim_border("─"),
        title_embed.cyan().bold(),
        dim_border(&"─".repeat(remaining)),
        dim_border("┐")
    );

    // Empty line
    print_padded_line("", inner);

    // CRYPTO art lines (centered, bold cyan)
    for line in &crypto_lines {
        print_centered_art(line, inner);
    }

    // KEEPER art lines (centered, bold cyan)
    for line in &keeper_lines {
        print_centered_art(line, inner);
    }

    // Empty line
    print_padded_line("", inner);

    // Version + tagline (centered, dimmed)
    let info = format!("{} — {}", version_line, tagline);
    print_centered_line(&format!("{}", info.dimmed()), &info, inner);

    // Empty line
    print_padded_line("", inner);

    // Bottom border
    println!(
        "{}{}{}",
        dim_border("└"),
        dim_border(&"─".repeat(inner + 2)),
        dim_border("┘")
    );
}

fn print_medium_header(width: usize) {
    let inner = width.saturating_sub(4);

    let title = "CRYPTOKEEPER";
    let version_line = format!("v{}", VERSION);
    let tagline = "Encrypted vault for crypto keys";

    // Top border
    let remaining = (inner + 2).saturating_sub(1);
    println!(
        "{}{}{}",
        dim_border("┌"),
        dim_border(&"─".repeat(remaining)),
        dim_border("┐")
    );

    // Empty line
    print_padded_line("", inner);

    // Title (centered, bold cyan)
    print_centered_line(&format!("{}", title.bold().cyan()), title, inner);

    // Empty line
    print_padded_line("", inner);

    // Version + tagline
    let info = format!("{} — {}", version_line, tagline);
    print_centered_line(&format!("{}", info.dimmed()), &info, inner);

    // Empty line
    print_padded_line("", inner);

    // Bottom border
    println!(
        "{}{}{}",
        dim_border("└"),
        dim_border(&"─".repeat(inner + 2)),
        dim_border("┘")
    );
}

fn print_narrow_header(width: usize) {
    let text = format!("CRYPTOKEEPER v{}", VERSION);
    let text_dw = display_width(&text);
    let side = width.saturating_sub(text_dw + 2) / 2;
    let right_side = width.saturating_sub(text_dw + 2 + side);
    println!(
        "{}{}{}{}{}",
        dim_border(&"─".repeat(side)),
        " ",
        text.cyan().bold(),
        " ",
        dim_border(&"─".repeat(right_side))
    );
}

/// Print a centered line (no ANSI codes in input) within bordered row.
fn print_centered_art(line: &str, inner: usize) {
    let art_width = display_width(line);
    let left_pad = inner.saturating_sub(art_width) / 2;
    let right_pad = inner.saturating_sub(art_width + left_pad);
    println!(
        "{} {}{}{} {}",
        dim_border("│"),
        " ".repeat(left_pad),
        line.bold().cyan(),
        " ".repeat(right_pad),
        dim_border("│")
    );
}

fn print_padded_line(content: &str, inner: usize) {
    let content_width = display_width(content);
    let padding = inner.saturating_sub(content_width);
    println!(
        "{} {}{} {}",
        dim_border("│"),
        content,
        " ".repeat(padding),
        dim_border("│")
    );
}

fn print_centered_line(styled: &str, raw: &str, inner: usize) {
    let raw_width = display_width(raw);
    let left_pad = inner.saturating_sub(raw_width) / 2;
    let right_pad = inner.saturating_sub(raw_width + left_pad);
    println!(
        "{} {}{}{} {}",
        dim_border("│"),
        " ".repeat(left_pad),
        styled,
        " ".repeat(right_pad),
        dim_border("│")
    );
}

pub fn render_header(frame: &mut Frame, area: Rect) {
    let width = area.width as usize;
    
    let content = if width >= 70 {
        build_wide_header()
    } else if width >= 50 {
        build_medium_header()
    } else {
        build_narrow_header()
    };
    
    frame.render_widget(content, area);
}

fn build_wide_header() -> Paragraph<'static> {
    let crypto_lines = [
        " ██████╗██████╗ ██╗   ██╗██████╗ ████████╗ ██████╗ ",
        "██╔════╝██╔══██╗╚██╗ ██╔╝██╔══██╗╚══██╔══╝██╔═══██╗",
        "██║     ██████╔╝ ╚████╔╝ ██████╔╝   ██║   ██║   ██║",
        "██║     ██╔══██╗  ╚██╔╝  ██╔═══╝    ██║   ██║   ██║",
        "╚██████╗██║  ██║   ██║   ██║        ██║   ╚██████╔╝",
        " ╚═════╝╚═╝  ╚═╝   ╚═╝   ╚═╝        ╚═╝    ╚═════╝ ",
    ];

    let keeper_lines = [
        "██╗  ██╗███████╗███████╗██████╗ ███████╗██████╗ ",
        "██║ ██╔╝██╔════╝██╔════╝██╔══██╗██╔════╝██╔══██╗",
        "█████╔╝ █████╗  █████╗  ██████╔╝█████╗  ██████╔╝",
        "██╔═██╗ ██╔══╝  ██╔══╝  ██╔═══╝ ██╔══╝  ██╔══██╗",
        "██║  ██╗███████╗███████╗██║     ███████╗██║  ██║",
        "╚═╝  ╚═╝╚══════╝╚══════╝╚═╝     ╚══════╝╚═╝  ╚═╝",
    ];

    let version_line = format!("v{}", VERSION);
    let tagline = "Encrypted vault for crypto keys & seed phrases";
    let info = format!("{} — {}", version_line, tagline);

    let art_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default()
        .fg(Color::DarkGray);

    let mut lines = vec![Line::from("")];
    
    for line in &crypto_lines {
        lines.push(Line::from(Span::styled(*line, art_style)));
    }
    
    for line in &keeper_lines {
        lines.push(Line::from(Span::styled(*line, art_style)));
    }
    
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(info, dim_style)));
    lines.push(Line::from(""));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" CryptoKeeper ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::DarkGray));

    Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center)
}

fn build_medium_header() -> Paragraph<'static> {
    let title = "CRYPTOKEEPER";
    let version_line = format!("v{}", VERSION);
    let tagline = "Encrypted vault for crypto keys";
    let info = format!("{} — {}", version_line, tagline);

    let title_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default()
        .fg(Color::DarkGray);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(title, title_style)),
        Line::from(""),
        Line::from(Span::styled(info, dim_style)),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center)
}

fn build_narrow_header() -> Paragraph<'static> {
    let text = format!("CRYPTOKEEPER v{}", VERSION);
    
    let style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let lines = vec![Line::from(Span::styled(text, style))];

    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray));

    Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center)
}
