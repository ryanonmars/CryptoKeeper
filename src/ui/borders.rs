use colored::{ColoredString, Colorize};
use unicode_width::UnicodeWidthStr;

use super::{get_terminal_width, is_interactive};
use super::theme::dim_border;

/// Measure the display width of a string, ignoring ANSI escape codes.
fn display_width(s: &str) -> usize {
    let stripped = console::strip_ansi_codes(s);
    UnicodeWidthStr::width(stripped.as_ref())
}

/// Pad a (possibly colored) string to exactly `target` display columns.
fn pad_to(s: &str, target: usize) -> String {
    let w = display_width(s);
    if w >= target {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(target - w))
    }
}

/// Truncate a plain string to fit within `max_width` display columns, adding "…" if needed.
pub fn truncate_display(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let w = UnicodeWidthStr::width(s);
    if w <= max_width {
        s.to_string()
    } else if max_width == 1 {
        "…".to_string()
    } else {
        // Take chars until we'd exceed max_width - 1 (room for ellipsis)
        let mut result = String::new();
        let mut current_width = 0;
        let mut buf = [0u8; 4];
        for ch in s.chars() {
            let ch_str: &str = ch.encode_utf8(&mut buf);
            let ch_w = UnicodeWidthStr::width(ch_str);
            if current_width + ch_w > max_width - 1 {
                break;
            }
            result.push(ch);
            current_width += ch_w;
        }
        result.push('…');
        result
    }
}

/// Print content lines wrapped in a bordered box.
///
/// ```text
/// ┌── Title ────────────────────┐
/// │  line 1                     │
/// │  line 2                     │
/// └─────────────────────────────┘
/// ```
pub fn print_box(title: Option<&str>, lines: &[String]) {
    if !is_interactive() {
        // Plain fallback for piped output
        if let Some(t) = title {
            println!("  {}", t);
            println!();
        }
        for line in lines {
            println!("  {}", line);
        }
        println!();
        return;
    }

    let width = get_terminal_width() as usize;
    let inner = width.saturating_sub(4); // 2 for "│ " and " │"

    // Top border
    let top = match title {
        Some(t) => {
            let t_display = format!(" {} ", t);
            let t_len = display_width(&t_display);
            let remaining = inner.saturating_sub(t_len + 1); // +1 for the leading "─"
            format!(
                "{}{}{}{}{}",
                dim_border("┌"),
                dim_border("─"),
                t_display.cyan().bold(),
                dim_border(&"─".repeat(remaining)),
                dim_border("┐")
            )
        }
        None => {
            format!(
                "{}{}{}",
                dim_border("┌"),
                dim_border(&"─".repeat(inner + 2)),
                dim_border("┐")
            )
        }
    };
    println!("{}", top);

    // Content lines
    for line in lines {
        let padded = pad_to(line, inner);
        println!(
            "{} {} {}",
            dim_border("│"),
            padded,
            dim_border("│")
        );
    }

    // Bottom border
    println!(
        "{}{}{}",
        dim_border("└"),
        dim_border(&"─".repeat(inner + 2)),
        dim_border("┘")
    );
}

/// Print a table with headers and rows inside a bordered box.
///
/// `col_styles` provides a style function per column. If fewer styles than columns,
/// the remaining columns use default (no color).
pub fn print_table_box(
    title: Option<&str>,
    headers: &[&str],
    rows: &[Vec<String>],
    col_styles: &[fn(&str) -> ColoredString],
) {
    if !is_interactive() {
        // Plain fallback
        if let Some(t) = title {
            println!("  {}", t);
            println!();
        }
        // Simple indented table
        let col_count = headers.len();
        let mut col_widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                if i < col_count {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }
        // Header
        let header_line: String = headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:<width$}", h, width = col_widths[i] + 2))
            .collect();
        println!("  {}", header_line);
        println!("  {}", "─".repeat(header_line.len()));
        // Rows
        for row in rows {
            let row_line: String = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let w = if i < col_count { col_widths[i] + 2 } else { cell.len() + 2 };
                    format!("{:<width$}", cell, width = w)
                })
                .collect();
            println!("  {}", row_line);
        }
        println!();
        return;
    }

    let width = get_terminal_width() as usize;
    let col_count = headers.len();
    let inner = width.saturating_sub(4); // "│ " + " │"

    // Calculate column widths: give each column its share of space
    let col_widths = compute_col_widths(headers, rows, inner, col_count);

    // Top border
    let top = match title {
        Some(t) => {
            let t_display = format!(" {} ", t);
            let t_len = display_width(&t_display);
            let remaining = (inner + 2).saturating_sub(t_len + 1);
            format!(
                "{}{}{}{}{}",
                dim_border("┌"),
                dim_border("─"),
                t_display.cyan().bold(),
                dim_border(&"─".repeat(remaining)),
                dim_border("┐")
            )
        }
        None => {
            format!(
                "{}{}{}",
                dim_border("┌"),
                dim_border(&"─".repeat(inner + 2)),
                dim_border("┐")
            )
        }
    };
    println!("{}", top);

    // Header row
    let header_cells: Vec<String> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let styled = format!("{}", h.bold());
            pad_to(&styled, col_widths[i])
        })
        .collect();
    let header_line = header_cells.join("");
    let header_padded = pad_to(&header_line, inner);
    println!(
        "{} {} {}",
        dim_border("│"),
        header_padded,
        dim_border("│")
    );

    // Header separator
    println!(
        "{}{}{}",
        dim_border("├"),
        dim_border(&"─".repeat(inner + 2)),
        dim_border("┤")
    );

    // Data rows
    let default_style: fn(&str) -> ColoredString = |s: &str| s.normal();
    for row in rows {
        let row_cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let w = if i < col_widths.len() { col_widths[i] } else { cell.len() };
                let truncated = truncate_display(cell, w.saturating_sub(1)); // leave 1 col gap
                let style_fn = col_styles.get(i).copied().unwrap_or(default_style);
                let styled = format!("{}", style_fn(&truncated));
                pad_to(&styled, w)
            })
            .collect();
        let row_line = row_cells.join("");
        let row_padded = pad_to(&row_line, inner);
        println!(
            "{} {} {}",
            dim_border("│"),
            row_padded,
            dim_border("│")
        );
    }

    // Bottom border
    println!(
        "{}{}{}",
        dim_border("└"),
        dim_border(&"─".repeat(inner + 2)),
        dim_border("┘")
    );
}

/// Compute column widths that fit within `total_width`.
fn compute_col_widths(
    headers: &[&str],
    rows: &[Vec<String>],
    total_width: usize,
    col_count: usize,
) -> Vec<usize> {
    if col_count == 0 {
        return vec![];
    }

    // Measure natural widths (max content + 2 padding)
    let mut natural: Vec<usize> = headers.iter().map(|h| h.len() + 2).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_count {
                natural[i] = natural[i].max(cell.len() + 2);
            }
        }
    }

    let total_natural: usize = natural.iter().sum();

    if total_natural <= total_width {
        // Everything fits — distribute extra space to the last column
        let extra = total_width - total_natural;
        let mut widths = natural;
        widths[col_count - 1] += extra;
        widths
    } else {
        // Need to shrink. Give each column proportional share, minimum 6.
        let mut widths: Vec<usize> = natural
            .iter()
            .map(|&n| {
                let share = (n as f64 / total_natural as f64 * total_width as f64) as usize;
                share.max(6)
            })
            .collect();

        // Adjust to fit exactly
        let sum: usize = widths.iter().sum();
        if sum > total_width {
            // Shrink last column
            widths[col_count - 1] = widths[col_count - 1].saturating_sub(sum - total_width);
        } else if sum < total_width {
            widths[col_count - 1] += total_width - sum;
        }

        widths
    }
}

/// Print a success message with a styled checkmark.
pub fn print_success(msg: &str) {
    if !is_interactive() {
        println!("  OK: {}", msg);
        return;
    }
    println!();
    println!("  {} {}", "✓".green().bold(), msg);
}

/// Print an error message with styling.
pub fn print_error(msg: &str) {
    if !is_interactive() {
        eprintln!("  Error: {}", msg);
        return;
    }
    eprintln!("  {} {}", "Error:".red().bold(), msg);
}

