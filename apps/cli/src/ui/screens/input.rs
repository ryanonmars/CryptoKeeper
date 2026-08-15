use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::borrow::Cow;
use zeroize::Zeroizing;

pub struct InputScreen {
    title: String,
    prompt: String,
    value: Zeroizing<String>,
    is_password: bool,
}

impl InputScreen {
    pub fn new(title: &str, prompt: &str, is_password: bool) -> Self {
        Self::new_with_value(title, prompt, is_password, "")
    }

    pub fn new_with_value(title: &str, prompt: &str, is_password: bool, value: &str) -> Self {
        Self {
            title: title.to_string(),
            prompt: prompt.to_string(),
            value: Zeroizing::new(value.to_string()),
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
                    Some(InputResult::Submit(std::mem::take(&mut self.value)))
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
            .constraints([
                Constraint::Min(1),
                Constraint::Length(7),
                Constraint::Min(1),
            ])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Cyan));

        let display_value: Cow<'_, str> = if self.is_password {
            Cow::Owned("•".repeat(self.value.chars().count()))
        } else {
            Cow::Borrowed(self.value.as_str())
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
    Submit(Zeroizing<String>),
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    #[test]
    fn password_submit_transfers_zeroizing_input_without_leaving_a_copy() {
        let mut screen = InputScreen::new("Backup", "Password:", true);
        for character in "backup-secret".chars() {
            screen.handle_key(KeyCode::Char(character), KeyModifiers::NONE);
        }

        let Some(InputResult::Submit(value)) =
            screen.handle_key(KeyCode::Enter, KeyModifiers::NONE)
        else {
            panic!("password was not submitted");
        };
        let _: Zeroizing<String> = value;
        assert!(screen.value.is_empty());
    }

    #[test]
    fn prefilled_input_allows_the_default_to_be_replaced() {
        let mut screen =
            InputScreen::new_with_value("Export Vault", "Backup name:", false, "backup");
        for _ in 0.."backup".len() {
            screen.handle_key(KeyCode::Backspace, KeyModifiers::NONE);
        }
        for character in "family-wallet".chars() {
            screen.handle_key(KeyCode::Char(character), KeyModifiers::NONE);
        }

        let Some(InputResult::Submit(value)) =
            screen.handle_key(KeyCode::Enter, KeyModifiers::NONE)
        else {
            panic!("prefilled value was not submitted");
        };

        assert_eq!(value.as_str(), "family-wallet");
        assert!(screen.value.is_empty());
    }
}
