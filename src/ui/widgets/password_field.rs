use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub struct PasswordField {
    buffer: String,
    prompt: String,
}

impl PasswordField {
    pub fn new(prompt: &str) -> Self {
        Self {
            buffer: String::new(),
            prompt: prompt.to_string(),
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> PasswordAction {
        match key {
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                PasswordAction::Cancel
            }
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                if self.buffer.is_empty() {
                    PasswordAction::Cancel
                } else {
                    PasswordAction::Continue
                }
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                self.buffer.push(c);
                PasswordAction::Continue
            }
            KeyCode::Backspace => {
                self.buffer.pop();
                PasswordAction::Continue
            }
            KeyCode::Enter => {
                if self.buffer.is_empty() {
                    PasswordAction::Continue
                } else {
                    PasswordAction::Submit(self.buffer.clone())
                }
            }
            KeyCode::Esc => PasswordAction::Cancel,
            _ => PasswordAction::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(7), Constraint::Min(1)])
            .split(area);

        let masked = "*".repeat(self.buffer.len());
        
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                self.prompt.as_str(),
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(masked, Style::default().fg(Color::Yellow)),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Enter Master Password ")
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(text).block(block);

        frame.render_widget(paragraph, chunks[1]);
    }
}

pub enum PasswordAction {
    Continue,
    Submit(String),
    Cancel,
}
