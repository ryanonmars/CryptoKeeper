use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub enum NukeAction {
    Continue,
    Cancel,
    Confirm,
}

pub struct NukeScreen {
    pub input: String,
    pub error_message: Option<String>,
}

impl Default for NukeScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl NukeScreen {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            error_message: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> NukeAction {
        if key == KeyCode::Esc {
            self.error_message = None;
            return NukeAction::Cancel;
        }

        self.error_message = None;

        match key {
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.push(c);
                NukeAction::Continue
            }
            KeyCode::Backspace => {
                self.input.pop();
                NukeAction::Continue
            }
            KeyCode::Enter => {
                if self.input == "DELETE" {
                    NukeAction::Confirm
                } else {
                    self.error_message = Some("Type DELETE (all caps) to confirm.".to_string());
                    NukeAction::Continue
                }
            }
            _ => NukeAction::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(14),
                Constraint::Min(1),
            ])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" \u{26a0}  DELETE VAULT ")
            .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(Color::Red));

        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  WARNING: This will permanently delete your vault.",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "  ALL stored keys and seed phrases will be lost forever.",
                Style::default().fg(Color::Red),
            )),
            Line::from(Span::styled(
                "  This action CANNOT be undone.",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Type ", Style::default().fg(Color::White)),
                Span::styled(
                    "DELETE",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to confirm: ", Style::default().fg(Color::White)),
                Span::styled(&self.input, Style::default().fg(Color::Yellow)),
                Span::styled("\u{2588}", Style::default().fg(Color::Cyan)),
            ]),
        ];

        if let Some(ref error) = self.error_message {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {}", error),
                Style::default().fg(Color::Red),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Enter: Confirm deletion | Esc: Cancel",
            Style::default().fg(Color::DarkGray),
        )));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, chunks[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn no_mod() -> KeyModifiers {
        KeyModifiers::empty()
    }

    #[test]
    fn esc_returns_cancel() {
        let mut screen = NukeScreen::new();
        let action = screen.handle_key(KeyCode::Esc, no_mod());
        assert!(matches!(action, NukeAction::Cancel));
    }

    #[test]
    fn enter_with_wrong_word_returns_continue_with_error() {
        let mut screen = NukeScreen::new();
        for c in "DELET".chars() {
            screen.handle_key(KeyCode::Char(c), no_mod());
        }
        let action = screen.handle_key(KeyCode::Enter, no_mod());
        assert!(matches!(action, NukeAction::Continue));
        assert!(screen.error_message.is_some());
    }

    #[test]
    fn enter_with_correct_word_returns_confirm() {
        let mut screen = NukeScreen::new();
        for c in "DELETE".chars() {
            screen.handle_key(KeyCode::Char(c), no_mod());
        }
        let action = screen.handle_key(KeyCode::Enter, no_mod());
        assert!(matches!(action, NukeAction::Confirm));
    }

    #[test]
    fn lowercase_delete_does_not_confirm() {
        let mut screen = NukeScreen::new();
        for c in "delete".chars() {
            screen.handle_key(KeyCode::Char(c), no_mod());
        }
        let action = screen.handle_key(KeyCode::Enter, no_mod());
        assert!(matches!(action, NukeAction::Continue));
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut screen = NukeScreen::new();
        screen.handle_key(KeyCode::Char('D'), no_mod());
        screen.handle_key(KeyCode::Char('E'), no_mod());
        screen.handle_key(KeyCode::Backspace, no_mod());
        assert_eq!(screen.input, "D");
    }
}
