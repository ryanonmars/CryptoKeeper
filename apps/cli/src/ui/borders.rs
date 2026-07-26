use colored::{ColoredString, Colorize};
use unicode_width::UnicodeWidthStr;

use super::theme::dim_border;
use super::{get_terminal_width, is_interactive};

/// Measure the display width of a string, ignoring ANSI escape codes.
fn display_width(s: &str) -> usize {
    let stripped = strip_terminal_formatting(s);
    UnicodeWidthStr::width(stripped.as_str())
}

fn strip_terminal_formatting(s: &str) -> String {
    let ansi_stripped = console::strip_ansi_codes(s);
    strip_osc_hyperlinks(ansi_stripped.as_ref())
}

fn strip_osc_hyperlinks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b']' {
            i += 2;
            while i < bytes.len() {
                if bytes[i] == 0x07 {
                    i += 1;
                    break;
                }
                if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }

        if let Some(ch) = s[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    out
}

fn sanitize_raw_box_text(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;

    while index < chars.len() {
        match chars[index] {
            '\u{001b}' if chars.get(index + 1) == Some(&'[') => {
                index += 2;
                while index < chars.len() {
                    let ch = chars[index];
                    index += 1;
                    if ('@'..='~').contains(&ch) {
                        break;
                    }
                }
            }
            '\u{009b}' => {
                index += 1;
                while index < chars.len() {
                    let ch = chars[index];
                    index += 1;
                    if ('@'..='~').contains(&ch) {
                        break;
                    }
                }
            }
            '\u{001b}' if chars.get(index + 1) == Some(&']') => {
                index += 2;
                skip_osc_sequence(&chars, &mut index);
            }
            '\u{009d}' => {
                index += 1;
                skip_osc_sequence(&chars, &mut index);
            }
            '\u{001b}' => {
                index = (index + 2).min(chars.len());
            }
            ch if matches!(ch as u32, 0x00..=0x1f | 0x7f..=0x9f) => {
                index += 1;
            }
            ch => {
                output.push(ch);
                index += 1;
            }
        }
    }

    output
}

fn skip_osc_sequence(chars: &[char], index: &mut usize) {
    while *index < chars.len() {
        match chars[*index] {
            '\u{0007}' | '\u{009c}' => {
                *index += 1;
                break;
            }
            '\u{001b}' if chars.get(*index + 1) == Some(&'\\') => {
                *index += 2;
                break;
            }
            _ => *index += 1,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TrustedBoxLine {
    rendered: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum BoxTextStyle {
    Bold,
    Cyan,
    Dimmed,
}

impl TrustedBoxLine {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push_text(&mut self, text: &str) -> &mut Self {
        self.rendered.push_str(&sanitize_raw_box_text(text));
        self
    }

    pub(crate) fn push_styled(&mut self, text: &str, style: BoxTextStyle) -> &mut Self {
        let safe = sanitize_raw_box_text(text);
        self.rendered.push_str(&apply_box_text_style(&safe, style));
        self
    }

    pub(crate) fn push_styled_padded(
        &mut self,
        text: &str,
        width: usize,
        style: BoxTextStyle,
    ) -> &mut Self {
        let safe = sanitize_raw_box_text(text);
        let display_width = UnicodeWidthStr::width(safe.as_str());
        self.rendered.push_str(&apply_box_text_style(&safe, style));
        self.rendered
            .push_str(&" ".repeat(width.saturating_sub(display_width)));
        self
    }

    pub(crate) fn push_hyperlink(&mut self, label: &str, target: &str) -> &mut Self {
        let safe_label = sanitize_raw_box_text(label);
        self.rendered
            .push_str(&crate::links::format_terminal_hyperlink(
                &safe_label,
                target,
            ));
        self
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.rendered
    }
}

fn apply_box_text_style(text: &str, style: BoxTextStyle) -> String {
    match style {
        BoxTextStyle::Bold => format!("{}", text.bold()),
        BoxTextStyle::Cyan => format!("{}", text.cyan()),
        BoxTextStyle::Dimmed => format!("{}", text.dimmed()),
    }
}

fn sanitized_table_content(
    title: Option<&str>,
    headers: &[&str],
    rows: &[Vec<String>],
) -> (Option<String>, Vec<String>, Vec<Vec<String>>) {
    (
        title.map(crate::links::sanitize_terminal_text),
        headers
            .iter()
            .map(|header| crate::links::sanitize_terminal_text(header))
            .collect(),
        rows.iter()
            .map(|row| {
                row.iter()
                    .map(|cell| crate::links::sanitize_terminal_text(cell))
                    .collect()
            })
            .collect(),
    )
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
    let sanitized = crate::links::sanitize_terminal_text(s);
    let s = sanitized.as_str();
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
    let mode = current_box_output_mode();
    for line in render_raw_box(title, lines, mode, get_terminal_width() as usize) {
        println!("{}", line);
    }
}

pub(crate) fn print_trusted_box(title: Option<&str>, lines: &[TrustedBoxLine]) {
    let mode = current_box_output_mode();
    for line in render_trusted_box(title, lines, mode, get_terminal_width() as usize) {
        println!("{}", line);
    }
}

#[derive(Clone, Copy, Debug)]
enum BoxOutputMode {
    Plain,
    Interactive,
}

fn current_box_output_mode() -> BoxOutputMode {
    if is_interactive() {
        BoxOutputMode::Interactive
    } else {
        BoxOutputMode::Plain
    }
}

fn render_raw_box(
    title: Option<&str>,
    lines: &[String],
    mode: BoxOutputMode,
    width: usize,
) -> Vec<String> {
    let safe_lines = lines
        .iter()
        .map(|line| sanitize_raw_box_text(line))
        .collect();
    render_box_frame(title, safe_lines, mode, width)
}

fn render_trusted_box(
    title: Option<&str>,
    lines: &[TrustedBoxLine],
    mode: BoxOutputMode,
    width: usize,
) -> Vec<String> {
    let safe_lines = lines
        .iter()
        .map(|line| match mode {
            BoxOutputMode::Plain => sanitize_raw_box_text(&line.rendered),
            BoxOutputMode::Interactive => line.rendered.clone(),
        })
        .collect();
    render_box_frame(title, safe_lines, mode, width)
}

fn render_box_frame(
    title: Option<&str>,
    lines: Vec<String>,
    mode: BoxOutputMode,
    width: usize,
) -> Vec<String> {
    let safe_title = title.map(sanitize_raw_box_text);

    if matches!(mode, BoxOutputMode::Plain) {
        let mut rendered = Vec::with_capacity(lines.len() + 3);
        if let Some(title) = safe_title {
            rendered.push(format!("  {}", title));
            rendered.push(String::new());
        }
        rendered.extend(lines.into_iter().map(|line| format!("  {}", line)));
        rendered.push(String::new());
        return rendered;
    }

    let inner = width.saturating_sub(4);
    let top = match safe_title.as_deref() {
        Some(title) => {
            let title_display = format!(" {} ", title);
            let title_len = display_width(&title_display);
            let remaining = inner.saturating_sub(title_len + 1);
            format!(
                "{}{}{}{}{}",
                dim_border("┌"),
                dim_border("─"),
                title_display.cyan().bold(),
                dim_border(&"─".repeat(remaining)),
                dim_border("┐")
            )
        }
        None => format!(
            "{}{}{}",
            dim_border("┌"),
            dim_border(&"─".repeat(inner + 2)),
            dim_border("┐")
        ),
    };

    let mut rendered = Vec::with_capacity(lines.len() + 2);
    rendered.push(top);
    rendered.extend(lines.into_iter().map(|line| {
        let padded = pad_to(&line, inner);
        format!("{} {} {}", dim_border("│"), padded, dim_border("│"))
    }));
    rendered.push(format!(
        "{}{}{}",
        dim_border("└"),
        dim_border(&"─".repeat(inner + 2)),
        dim_border("┘")
    ));
    rendered
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
    let (safe_title, safe_headers, safe_rows) = sanitized_table_content(title, headers, rows);

    if !is_interactive() {
        // Plain fallback
        if let Some(t) = safe_title.as_deref() {
            println!("  {}", t);
            println!();
        }
        // Simple indented table
        let col_count = safe_headers.len();
        let mut col_widths: Vec<usize> = safe_headers
            .iter()
            .map(|header| UnicodeWidthStr::width(header.as_str()))
            .collect();
        for row in &safe_rows {
            for (i, cell) in row.iter().enumerate() {
                if i < col_count {
                    col_widths[i] = col_widths[i].max(UnicodeWidthStr::width(cell.as_str()));
                }
            }
        }
        // Header
        let header_line: String = safe_headers
            .iter()
            .enumerate()
            .map(|(i, header)| pad_to(header, col_widths[i] + 2))
            .collect();
        println!("  {}", header_line);
        println!("  {}", "─".repeat(display_width(&header_line)));
        // Rows
        for row in &safe_rows {
            let row_line: String = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let w = if i < col_count {
                        col_widths[i] + 2
                    } else {
                        UnicodeWidthStr::width(cell.as_str()) + 2
                    };
                    pad_to(cell, w)
                })
                .collect();
            println!("  {}", row_line);
        }
        println!();
        return;
    }

    let width = get_terminal_width() as usize;
    let col_count = safe_headers.len();
    let inner = width.saturating_sub(4); // "│ " + " │"

    // Calculate column widths: give each column its share of space
    let col_widths = compute_col_widths(&safe_headers, &safe_rows, inner, col_count);

    // Top border
    let top = match safe_title.as_deref() {
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
    let header_cells: Vec<String> = safe_headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let styled = format!("{}", h.bold());
            pad_to(&styled, col_widths[i])
        })
        .collect();
    let header_line = header_cells.join("");
    let header_padded = pad_to(&header_line, inner);
    println!("{} {} {}", dim_border("│"), header_padded, dim_border("│"));

    // Header separator
    println!(
        "{}{}{}",
        dim_border("├"),
        dim_border(&"─".repeat(inner + 2)),
        dim_border("┤")
    );

    // Data rows
    let default_style: fn(&str) -> ColoredString = |s: &str| s.normal();
    for row in &safe_rows {
        let row_cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let w = if i < col_widths.len() {
                    col_widths[i]
                } else {
                    UnicodeWidthStr::width(cell.as_str())
                };
                let truncated = truncate_display(cell, w.saturating_sub(1)); // leave 1 col gap
                let style_fn = col_styles.get(i).copied().unwrap_or(default_style);
                let styled = format!("{}", style_fn(&truncated));
                pad_to(&styled, w)
            })
            .collect();
        let row_line = row_cells.join("");
        let row_padded = pad_to(&row_line, inner);
        println!("{} {} {}", dim_border("│"), row_padded, dim_border("│"));
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
    headers: &[String],
    rows: &[Vec<String>],
    total_width: usize,
    col_count: usize,
) -> Vec<usize> {
    if col_count == 0 {
        return vec![];
    }

    // Measure natural widths (max content + 2 padding)
    let mut natural: Vec<usize> = headers
        .iter()
        .map(|header| UnicodeWidthStr::width(header.as_str()) + 2)
        .collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_count {
                natural[i] = natural[i].max(UnicodeWidthStr::width(cell.as_str()) + 2);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_table_border_plain_and_interactive_paths_sanitize_raw_cells_and_titles() {
        let rows = vec![vec![
            "cell\u{001b}]0;owned\u{0007}".to_string(),
            "c1\u{009b}31m".to_string(),
        ]];

        let (title, headers, rows) =
            sanitized_table_content(Some("ti\u{001b}tle"), &["he\u{0000}ader", "other"], &rows);

        assert_eq!(title.as_deref(), Some("title"));
        assert_eq!(headers, ["header", "other"]);
        assert_eq!(
            rows,
            [vec!["cell]0;owned".to_string(), "c131m".to_string()]]
        );
        assert!(headers
            .iter()
            .chain(rows.iter().flatten())
            .all(|value| !contains_disallowed_control(value)));
    }

    #[test]
    fn raw_box_content_removes_sgr_osc_csi_and_c1_sequences() {
        let rendered = sanitize_raw_box_text(
            "\u{001b}[31mred\u{001b}[0m\
             \u{001b}]8;;https://attacker.invalid\u{001b}\\visible\u{001b}]8;;\u{001b}\\\
             \u{001b}]0;owned\u{0007}\u{001b}[2J\u{009b}31m",
        );

        assert_eq!(rendered, "redvisible");
        assert!(!contains_disallowed_control(&rendered));
    }

    #[test]
    fn raw_box_rendering_is_control_free_in_plain_and_interactive_modes() {
        let title = "Ti\u{001b}[31mtle\u{001b}[0m\u{001b}]0;owned\u{0007}";
        let lines = vec!["\u{001b}[38;5;123mred\u{001b}[0m\
             \u{001b}]8;;https://attacker.invalid\u{001b}\\visible\u{001b}]8;;\u{001b}\\\
             \u{001b}[2Jend\u{009b}31m"
            .to_string()];

        for mode in [BoxOutputMode::Plain, BoxOutputMode::Interactive] {
            let rendered = render_raw_box(title.into(), &lines, mode, 80);

            assert!(rendered
                .iter()
                .all(|line| !contains_disallowed_control(line)));
            assert!(rendered.iter().any(|line| line.contains("Title")));
            assert!(rendered.iter().any(|line| line.contains("redvisibleend")));
            assert!(rendered.iter().all(|line| {
                !line.contains("38;5;123")
                    && !line.contains("attacker.invalid")
                    && !line.contains("]8;;")
                    && !line.contains("]0;owned")
                    && !line.contains("[2J")
            }));
        }
    }

    #[test]
    fn trusted_box_keeps_generated_hyperlink_exact_only_when_interactive() {
        let mut line = TrustedBoxLine::new();
        line.push_hyperlink("visible", "https://example.com/a?x=1&y=2");
        let expected =
            "\u{001b}]8;;https://example.com/a?x=1&y=2\u{001b}\\visible\u{001b}]8;;\u{001b}\\";
        assert_eq!(display_width(expected), 7);

        let interactive = render_trusted_box(
            Some("Trusted"),
            &[line.clone()],
            BoxOutputMode::Interactive,
            80,
        );
        assert!(interactive.iter().any(|line| line.contains(expected)));

        let plain = render_trusted_box(Some("Trusted"), &[line], BoxOutputMode::Plain, 80);
        assert!(plain.iter().any(|line| line.contains("visible")));
        assert!(plain.iter().all(|line| !contains_disallowed_control(line)));
        assert!(plain.iter().all(|line| !line.contains("]8;;")));
        assert!(plain
            .iter()
            .all(|line| !line.contains("https://example.com/a?x=1&y=2")));
    }

    #[test]
    fn trusted_box_builder_sanitizes_every_runtime_fragment_before_styling() {
        let mut line = TrustedBoxLine::new();
        line.push_text("\u{001b}]0;owned\u{0007}plain");
        line.push_styled("\u{001b}[31mattacker-red\u{001b}[0m", BoxTextStyle::Cyan);
        line.push_hyperlink(
            "\u{001b}]8;;https://attacker.invalid\u{001b}\\label\u{001b}]8;;\u{001b}\\",
            "https://example.com",
        );

        let rendered = render_trusted_box(None, &[line], BoxOutputMode::Interactive, 80).join("\n");

        assert!(!rendered.contains("]0;owned"));
        assert!(!rendered.contains("\u{001b}[31m"));
        assert!(!rendered.contains("attacker.invalid"));
        assert!(rendered.contains("plain"));
        assert!(rendered.contains("attacker-red"));
        assert!(rendered.contains("label"));
    }

    fn contains_disallowed_control(value: &str) -> bool {
        value
            .chars()
            .any(|ch| matches!(ch as u32, 0x00..=0x1f | 0x7f..=0x9f))
    }
}
