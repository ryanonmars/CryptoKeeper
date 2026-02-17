use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub struct MenuBar {
    items: Vec<(char, &'static str)>,
}

impl MenuBar {
    pub fn new() -> Self {
        Self {
            items: vec![
                ('A', "Add"),
                ('V', "View"),
                ('C', "Copy"),
                ('E', "Edit"),
                ('D', "Delete"),
                ('S', "Search"),
                ('X', "Export"),
                ('I', "Import"),
                ('P', "Passwd"),
                ('?', "Help"),
                ('Q', "Quit"),
            ],
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let mut spans = Vec::new();
        spans.push(Span::raw(" "));

        for (i, (key, label)) in self.items.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                format!("[{}]", key),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(format!("{} ", label)));
        }

        let paragraph = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));

        frame.render_widget(paragraph, area);
    }
}
