use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use zeroize::Zeroizing;

pub enum ViewPasswordAction {
    Continue,
    Submit(Zeroizing<String>),
    Cancel,
}

pub struct ViewPasswordScreen {
    buffer: String,
    title: String,
    error_message: Option<String>,
}

impl ViewPasswordScreen {
    pub fn new(title: &str) -> Self {
        Self {
            buffer: String::new(),
            title: title.to_string(),
            error_message: None,
        }
    }

    pub fn set_error(&mut self, msg: &str) {
        self.error_message = Some(msg.to_string());
        self.buffer.clear();
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> ViewPasswordAction {
        if key == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            return ViewPasswordAction::Cancel;
        }

        self.error_message = None;

        match key {
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                self.buffer.push(c);
                ViewPasswordAction::Continue
            }
            KeyCode::Backspace => {
                self.buffer.pop();
                ViewPasswordAction::Continue
            }
            KeyCode::Enter => {
                if self.buffer.is_empty() {
                    ViewPasswordAction::Continue
                } else {
                    ViewPasswordAction::Submit(Zeroizing::new(self.buffer.clone()))
                }
            }
            KeyCode::Esc => ViewPasswordAction::Cancel,
            _ => ViewPasswordAction::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(8),
                Constraint::Min(1),
            ])
            .split(area);

        let masked = "*".repeat(self.buffer.len());

        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Enter the secondary password for this entry:",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(masked, Style::default().fg(Color::Yellow)),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ]),
        ];

        if let Some(ref error) = self.error_message {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {}", error),
                Style::default().fg(Color::Red),
            )));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Yellow));

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, chunks[1]);
    }
}

impl Drop for ViewPasswordScreen {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.buffer.zeroize();
    }
}
