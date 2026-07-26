use crossterm::event::{KeyCode, KeyModifiers};
use rand::seq::SliceRandom;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use zeroize::{Zeroize, Zeroizing};

enum SetupStep {
    Display,
    Confirm,
}

pub struct RecoverySetupScreen {
    phrase: Zeroizing<String>,
    words: Vec<Zeroizing<String>>,
    positions: [usize; 3],
    confirmation_index: usize,
    input: Zeroizing<String>,
    step: SetupStep,
    error: Option<String>,
}

pub enum RecoverySetupAction {
    Continue,
    Cancel,
    Confirmed(Zeroizing<String>),
}

impl RecoverySetupScreen {
    pub fn new(phrase: Zeroizing<String>) -> Self {
        let mut positions: Vec<usize> = (0..24).collect();
        positions.shuffle(&mut rand::thread_rng());
        Self::with_confirmation_positions(phrase, [positions[0], positions[1], positions[2]])
    }

    fn with_confirmation_positions(phrase: Zeroizing<String>, positions: [usize; 3]) -> Self {
        let words = phrase
            .split_whitespace()
            .map(|word| Zeroizing::new(word.to_string()))
            .collect();
        Self {
            phrase,
            words,
            positions,
            confirmation_index: 0,
            input: Zeroizing::new(String::new()),
            step: SetupStep::Display,
            error: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> RecoverySetupAction {
        if key == KeyCode::Esc {
            return RecoverySetupAction::Cancel;
        }

        match self.step {
            SetupStep::Display => {
                if key == KeyCode::Enter {
                    self.step = SetupStep::Confirm;
                }
            }
            SetupStep::Confirm => match key {
                KeyCode::Char(character) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    self.input.push(character);
                    self.error = None;
                }
                KeyCode::Backspace => {
                    self.input.pop();
                    self.error = None;
                }
                KeyCode::Enter => {
                    let position = self.positions[self.confirmation_index];
                    if self.input.trim() != self.words[position].as_str() {
                        self.input.zeroize();
                        self.error = Some(format!(
                            "That word is incorrect. Enter word #{} again.",
                            position + 1
                        ));
                        return RecoverySetupAction::Continue;
                    }

                    self.input.zeroize();
                    self.confirmation_index += 1;
                    if self.confirmation_index == self.positions.len() {
                        self.words.zeroize();
                        return RecoverySetupAction::Confirmed(std::mem::take(&mut self.phrase));
                    }
                }
                _ => {}
            },
        }
        RecoverySetupAction::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Recovery Phrase Setup ")
            .title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Yellow));

        let lines = match self.step {
            SetupStep::Display => vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Write down this recovery phrase. It will only be shown once.",
                    Style::default().fg(Color::White),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    self.phrase.as_str(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Press Enter when you have saved it, or Esc to cancel.",
                    Style::default().fg(Color::DarkGray),
                )),
            ],
            SetupStep::Confirm => {
                let position = self.positions[self.confirmation_index];
                let mut lines = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        format!(
                            "Confirm word #{} ({}/3):",
                            position + 1,
                            self.confirmation_index + 1
                        ),
                        Style::default().fg(Color::White),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::raw(self.input.as_str()),
                        Span::raw("█"),
                    ]),
                ];
                if let Some(error) = &self.error {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        error.as_str(),
                        Style::default().fg(Color::Red),
                    )));
                }
                lines
            }
        };

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(paragraph, centered_rect(80, frame.area()));
    }
}

impl Drop for RecoverySetupScreen {
    fn drop(&mut self) {
        self.input.zeroize();
        self.words.zeroize();
    }
}

fn centered_rect(percent: u16, area: Rect) -> Rect {
    let width = area.width * percent / 100;
    let x = area.x + (area.width - width) / 2;
    Rect {
        x,
        y: area.y,
        width,
        height: area.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_confirmation_rejects_wrong_words() {
        let phrase = crate::crypto::recovery::generate_recovery_phrase().unwrap();
        let words: Vec<_> = phrase.split_whitespace().map(str::to_string).collect();
        let mut screen = RecoverySetupScreen::with_confirmation_positions(
            Zeroizing::new(phrase.to_string()),
            [0, 5, 23],
        );

        assert!(matches!(
            screen.handle_key(KeyCode::Enter, KeyModifiers::NONE),
            RecoverySetupAction::Continue
        ));
        for character in "wrong".chars() {
            screen.handle_key(KeyCode::Char(character), KeyModifiers::NONE);
        }
        assert!(matches!(
            screen.handle_key(KeyCode::Enter, KeyModifiers::NONE),
            RecoverySetupAction::Continue
        ));

        for position in [0, 5, 23] {
            for character in words[position].chars() {
                screen.handle_key(KeyCode::Char(character), KeyModifiers::NONE);
            }
            let action = screen.handle_key(KeyCode::Enter, KeyModifiers::NONE);
            if position != 23 {
                assert!(matches!(action, RecoverySetupAction::Continue));
            } else {
                assert!(matches!(action, RecoverySetupAction::Confirmed(_)));
            }
        }
    }

    #[test]
    fn confirmation_render_borrows_the_zeroizing_input() {
        let source = include_str!("recovery_setup.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();

        assert!(!production.contains("Line::from(format!(\"  {}█\""));
        assert!(production.contains("Span::raw(self.input.as_str())"));
    }
}
