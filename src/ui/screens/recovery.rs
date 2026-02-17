use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use zeroize::{Zeroize, Zeroizing};

use crate::config::RecoveryConfig;
use crate::config::model::RECOVERY_QUESTIONS;
use crate::crypto::recovery;

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Answer,
    NewPassword,
    ConfirmPassword,
}

pub struct RecoveryScreen {
    step: Step,
    question: String,
    answer: String,
    new_password: String,
    confirm_password: String,
    error_message: Option<String>,
    recovery_config: RecoveryConfig,
    master_key: Option<Zeroizing<[u8; 32]>>,
}

impl Drop for RecoveryScreen {
    fn drop(&mut self) {
        self.answer.zeroize();
        self.new_password.zeroize();
        self.confirm_password.zeroize();
    }
}

pub enum RecoveryAction {
    Continue,
    Cancel,
    /// Recovery complete: (master_key, new_password)
    Complete {
        master_key: Zeroizing<[u8; 32]>,
        new_password: Zeroizing<String>,
    },
}

impl RecoveryScreen {
    pub fn new(recovery_config: RecoveryConfig) -> Self {
        let question = RECOVERY_QUESTIONS
            .get(recovery_config.question_index as usize)
            .unwrap_or(&"Unknown question")
            .to_string();

        Self {
            step: Step::Answer,
            question,
            answer: String::new(),
            new_password: String::new(),
            confirm_password: String::new(),
            error_message: None,
            recovery_config,
            master_key: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> RecoveryAction {
        if key == KeyCode::Esc {
            return RecoveryAction::Cancel;
        }

        self.error_message = None;

        match key {
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                self.current_buffer_mut().push(c);
                RecoveryAction::Continue
            }
            KeyCode::Backspace => {
                self.current_buffer_mut().pop();
                RecoveryAction::Continue
            }
            KeyCode::Enter => self.handle_enter(),
            _ => RecoveryAction::Continue,
        }
    }

    fn current_buffer_mut(&mut self) -> &mut String {
        match self.step {
            Step::Answer => &mut self.answer,
            Step::NewPassword => &mut self.new_password,
            Step::ConfirmPassword => &mut self.confirm_password,
        }
    }

    fn current_buffer(&self) -> &str {
        match self.step {
            Step::Answer => &self.answer,
            Step::NewPassword => &self.new_password,
            Step::ConfirmPassword => &self.confirm_password,
        }
    }

    fn handle_enter(&mut self) -> RecoveryAction {
        if self.current_buffer().is_empty() {
            return RecoveryAction::Continue;
        }

        match self.step {
            Step::Answer => {
                let normalized = recovery::normalize_answer(&self.answer);

                // Verify answer
                match recovery::verify_answer(
                    &normalized,
                    &self.recovery_config.answer_salt,
                    &self.recovery_config.answer_hash,
                ) {
                    Ok(true) => {}
                    Ok(false) => {
                        self.error_message = Some("Incorrect answer. Try again.".to_string());
                        self.answer.zeroize();
                        self.answer = String::new();
                        return RecoveryAction::Continue;
                    }
                    Err(_) => {
                        self.error_message =
                            Some("Verification error. Try again.".to_string());
                        self.answer.zeroize();
                        self.answer = String::new();
                        return RecoveryAction::Continue;
                    }
                }

                // Decrypt master key
                match recovery::decrypt_recovery_blob(
                    &self.recovery_config.master_key_blob,
                    &self.recovery_config.master_key_blob_nonce,
                    &self.recovery_config.master_key_blob_salt,
                    &normalized,
                ) {
                    Ok(key) => {
                        self.master_key = Some(key);
                        self.step = Step::NewPassword;
                        RecoveryAction::Continue
                    }
                    Err(_) => {
                        self.error_message =
                            Some("Failed to recover master key. Try again.".to_string());
                        self.answer.zeroize();
                        self.answer = String::new();
                        RecoveryAction::Continue
                    }
                }
            }
            Step::NewPassword => {
                if self.new_password.len() < 8 {
                    self.error_message =
                        Some("Password must be at least 8 characters.".to_string());
                    return RecoveryAction::Continue;
                }
                self.step = Step::ConfirmPassword;
                RecoveryAction::Continue
            }
            Step::ConfirmPassword => {
                if self.new_password != self.confirm_password {
                    self.error_message = Some("Passwords do not match.".to_string());
                    self.confirm_password.zeroize();
                    self.confirm_password = String::new();
                    self.step = Step::NewPassword;
                    self.new_password.zeroize();
                    self.new_password = String::new();
                    return RecoveryAction::Continue;
                }

                if let Some(master_key) = self.master_key.take() {
                    RecoveryAction::Complete {
                        master_key,
                        new_password: Zeroizing::new(self.new_password.clone()),
                    }
                } else {
                    self.error_message = Some("Internal error. Please restart.".to_string());
                    RecoveryAction::Continue
                }
            }
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(12),
                Constraint::Min(1),
            ])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Password Recovery ")
            .title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Yellow));

        let masked = "\u{2022}".repeat(self.current_buffer().len());

        let mut lines = vec![Line::from("")];

        match self.step {
            Step::Answer => {
                lines.push(Line::from(Span::styled(
                    "Recovery question:",
                    Style::default().fg(Color::White),
                )));
                lines.push(Line::from(Span::styled(
                    format!("  {}", self.question),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("  Your answer: ", Style::default().fg(Color::White)),
                    Span::styled(&masked, Style::default().fg(Color::Yellow)),
                    Span::styled("\u{2588}", Style::default().fg(Color::Cyan)),
                ]));
            }
            Step::NewPassword => {
                lines.push(Line::from(Span::styled(
                    "Recovery successful! Set a new master password.",
                    Style::default().fg(Color::Green),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(
                        "  New password: ",
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(&masked, Style::default().fg(Color::Yellow)),
                    Span::styled("\u{2588}", Style::default().fg(Color::Cyan)),
                ]));
                lines.push(Line::from(Span::styled(
                    "  (minimum 8 characters)",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            Step::ConfirmPassword => {
                lines.push(Line::from(Span::styled(
                    "Recovery successful! Set a new master password.",
                    Style::default().fg(Color::Green),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(
                        "  Confirm password: ",
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(&masked, Style::default().fg(Color::Yellow)),
                    Span::styled("\u{2588}", Style::default().fg(Color::Cyan)),
                ]));
            }
        }

        if let Some(ref error) = self.error_message {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {}", error),
                Style::default().fg(Color::Red),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Enter: Submit | Esc: Cancel",
            Style::default().fg(Color::DarkGray),
        )));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, chunks[1]);
    }
}
