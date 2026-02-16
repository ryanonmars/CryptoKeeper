use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub struct InputScreen {
    title: String,
    prompt: String,
    value: String,
    is_password: bool,
}

impl InputScreen {
    pub fn new(title: &str, prompt: &str, is_password: bool) -> Self {
        Self {
            title: title.to_string(),
            prompt: prompt.to_string(),
            value: String::new(),
            is_password,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Option<InputResult> {
        match key {
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.push(c);
                None
            }
            KeyCode::Backspace => {
                self.value.pop();
                None
            }
            KeyCode::Enter => {
                if !self.value.is_empty() {
                    Some(InputResult::Submit(self.value.clone()))
                } else {
                    None
                }
            }
            KeyCode::Esc => Some(InputResult::Cancel),
            _ => None,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(7), Constraint::Min(1)])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(Color::Cyan));

        let display_value = if self.is_password {
            "•".repeat(self.value.len())
        } else {
            self.value.clone()
        };

        let text = vec![
            Line::from(self.prompt.as_str()),
            Line::from(""),
            Line::from(vec![
                Span::styled(display_value, Style::default().fg(Color::Yellow)),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Enter: Submit │ Esc: Cancel",
                Style::default().fg(Color::DarkGray),
            )]),
        ];

        let paragraph = Paragraph::new(text).block(block);

        frame.render_widget(paragraph, chunks[1]);
    }
}

pub enum InputResult {
    Submit(String),
    Cancel,
}
