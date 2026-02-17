use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use zeroize::Zeroizing;

use crate::ui::widgets::password_field::{PasswordAction, PasswordField};

pub struct LoginScreen {
    password_field: PasswordField,
}

impl LoginScreen {
    pub fn new() -> Self {
        Self {
            password_field: PasswordField::new("Enter your master password to unlock the vault:"),
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Option<Zeroizing<String>> {
        match self.password_field.handle_key(key, modifiers) {
            PasswordAction::Submit(password) => Some(Zeroizing::new(password)),
            PasswordAction::Cancel => None,
            PasswordAction::Continue => None,
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        self.password_field.render(frame, chunks[0]);

        let hint = Paragraph::new(Line::from(vec![
            Span::styled("F1", Style::default().fg(Color::Cyan)),
            Span::styled(" Forgot password?", Style::default().fg(Color::DarkGray)),
        ]))
        .style(Style::default().bg(Color::Black));
        frame.render_widget(hint, chunks[1]);
    }
}
