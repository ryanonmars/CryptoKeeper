use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use unicode_width::UnicodeWidthStr;

pub struct MenuBar {
    items: Vec<(&'static str, &'static str)>,
}

fn item_width(key: &str, label: &str) -> usize {
    1 + "[".width() + key.width() + "]".width() + 1 + label.width() + 1
}

impl MenuBar {
    pub fn new() -> Self {
        Self {
            items: vec![
                ("⇧A", "Add"),
                ("⇧V", "View"),
                ("⇧C", "Copy"),
                ("⇧E", "Edit"),
                ("⇧D", "Delete"),
                ("⇧F", "Find"),
                ("⇧X", "Export"),
                ("⇧I", "Import"),
                ("⇧P", "Passwd"),
                ("⇧S", "Settings"),
                ("?", "Help"),
                ("⇧Q", "Quit"),
            ],
        }
    }

    pub fn lines_for_width(&self, width: u16) -> u16 {
        let w = width as usize;
        let mut lines = 1u16;
        let mut current_width = 0usize;
        for (key, label) in &self.items {
            let item_w = item_width(key, label);
            let need = if current_width == 0 { 1 + item_w } else { current_width + item_w };
            if need > w && current_width > 0 {
                lines += 1;
                current_width = 1 + item_w;
            } else {
                current_width = if current_width == 0 { 1 + item_w } else { need };
            }
        }
        lines
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let width = area.width as usize;
        let mut lines: Vec<Line> = Vec::new();
        let mut current_spans: Vec<Span> = Vec::new();
        let mut current_width = 0usize;

        for (key, label) in &self.items {
            let item_w = item_width(key, label);
            if current_width + item_w > width && !current_spans.is_empty() {
                lines.push(Line::from(current_spans));
                current_spans = Vec::new();
                current_width = 0;
            }
            if current_spans.is_empty() {
                current_spans.push(Span::raw(" "));
                current_width += 1;
            }
            current_spans.push(Span::styled(
                format!("[{}]", key),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));
            current_spans.push(Span::raw(format!("{} ", label)));
            current_width += item_w;
        }
        if !current_spans.is_empty() {
            lines.push(Line::from(current_spans));
        }

        let paragraph = Paragraph::new(lines)
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));

        frame.render_widget(paragraph, area);
    }
}
