use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub struct StatusBar {
    vault_name: String,
    entry_count: usize,
    filter_text: String,
    number_buffer: String,
}

impl StatusBar {
    pub fn new(vault_name: &str, entry_count: usize, filter_text: &str, number_buffer: &str) -> Self {
        Self {
            vault_name: vault_name.to_string(),
            entry_count,
            filter_text: filter_text.to_string(),
            number_buffer: number_buffer.to_string(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let filter_display = if self.filter_text.is_empty() {
            String::new()
        } else {
            format!(" │ Filter: {}", self.filter_text)
        };

        let number_display = if self.number_buffer.is_empty() {
            String::new()
        } else {
            format!(" │ Go to: {}█", self.number_buffer)
        };

        let content = format!(
            " {} │ {} entries{}{} │ ? for help ",
            self.vault_name, self.entry_count, filter_display, number_display
        );

        let spans = vec![Span::styled(
            content,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )];

        let paragraph = Paragraph::new(Line::from(spans));
        frame.render_widget(paragraph, area);
    }
}
