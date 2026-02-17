use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use zeroize::Zeroize;

use crate::config::model::RECOVERY_QUESTIONS;
use crate::crypto::recovery;

#[derive(Clone, Copy, PartialEq)]
enum Step {
    SelectQuestion,
    EnterAnswer,
    ConfirmAnswer,
}

pub struct RecoverySetupScreen {
    step: Step,
    question_index: usize,
    answer: String,
    confirm_answer: String,
    error_message: Option<String>,
}

impl Drop for RecoverySetupScreen {
    fn drop(&mut self) {
        self.answer.zeroize();
        self.confirm_answer.zeroize();
    }
}

pub enum RecoverySetupAction {
    Continue,
    Cancel,
    /// Setup complete: (question_index, normalized_answer)
    Complete { question_index: u8, answer: String },
}

impl RecoverySetupScreen {
    pub fn new() -> Self {
        Self {
            step: Step::SelectQuestion,
            question_index: 0,
            answer: String::new(),
            confirm_answer: String::new(),
            error_message: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> RecoverySetupAction {
        if key == KeyCode::Esc {
            match self.step {
                Step::SelectQuestion => return RecoverySetupAction::Cancel,
                Step::EnterAnswer => {
                    self.answer.zeroize();
                    self.answer = String::new();
                    self.step = Step::SelectQuestion;
                    self.error_message = None;
                    return RecoverySetupAction::Continue;
                }
                Step::ConfirmAnswer => {
                    self.confirm_answer.zeroize();
                    self.confirm_answer = String::new();
                    self.step = Step::EnterAnswer;
                    self.error_message = None;
                    return RecoverySetupAction::Continue;
                }
            }
        }

        self.error_message = None;

        match self.step {
            Step::SelectQuestion => match key {
                KeyCode::Up => {
                    if self.question_index > 0 {
                        self.question_index -= 1;
                    }
                    RecoverySetupAction::Continue
                }
                KeyCode::Down => {
                    if self.question_index < RECOVERY_QUESTIONS.len() - 1 {
                        self.question_index += 1;
                    }
                    RecoverySetupAction::Continue
                }
                KeyCode::Enter => {
                    self.step = Step::EnterAnswer;
                    RecoverySetupAction::Continue
                }
                _ => RecoverySetupAction::Continue,
            },
            Step::EnterAnswer => match key {
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    self.answer.push(c);
                    RecoverySetupAction::Continue
                }
                KeyCode::Backspace => {
                    self.answer.pop();
                    RecoverySetupAction::Continue
                }
                KeyCode::Enter => {
                    let trimmed = self.answer.trim();
                    if trimmed.len() < 3 {
                        self.error_message =
                            Some("Answer must be at least 3 characters.".to_string());
                        RecoverySetupAction::Continue
                    } else {
                        self.step = Step::ConfirmAnswer;
                        RecoverySetupAction::Continue
                    }
                }
                _ => RecoverySetupAction::Continue,
            },
            Step::ConfirmAnswer => match key {
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    self.confirm_answer.push(c);
                    RecoverySetupAction::Continue
                }
                KeyCode::Backspace => {
                    self.confirm_answer.pop();
                    RecoverySetupAction::Continue
                }
                KeyCode::Enter => {
                    let a = recovery::normalize_answer(&self.answer);
                    let b = recovery::normalize_answer(&self.confirm_answer);
                    if a != b {
                        self.error_message = Some("Answers do not match.".to_string());
                        self.confirm_answer.zeroize();
                        self.confirm_answer = String::new();
                        RecoverySetupAction::Continue
                    } else {
                        RecoverySetupAction::Complete {
                            question_index: self.question_index as u8,
                            answer: a,
                        }
                    }
                }
                _ => RecoverySetupAction::Continue,
            },
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
            .title(" Set Up Recovery Question ")
            .title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Yellow));

        let inner_area = chunks[1];
        let centered = centered_rect(90, inner_area);

        match self.step {
            Step::SelectQuestion => {
                let mut lines = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "Select a recovery question:",
                        Style::default().fg(Color::White),
                    )),
                    Line::from(""),
                ];

                for (i, question) in RECOVERY_QUESTIONS.iter().enumerate() {
                    let style = if i == self.question_index {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let prefix = if i == self.question_index {
                        " \u{25b8} "
                    } else {
                        "   "
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{}{}", prefix, question),
                        style,
                    )));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  \u{2191}/\u{2193}: Navigate | Enter: Select | Esc: Cancel",
                    Style::default().fg(Color::DarkGray),
                )));

                let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, centered);
            }
            Step::EnterAnswer => {
                let question = RECOVERY_QUESTIONS[self.question_index];
                let masked = "\u{2022}".repeat(self.answer.len());

                let mut lines = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        question,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("  Your answer: ", Style::default().fg(Color::White)),
                        Span::styled(masked, Style::default().fg(Color::Yellow)),
                        Span::styled("\u{2588}", Style::default().fg(Color::Cyan)),
                    ]),
                    Line::from(Span::styled(
                        "  (minimum 3 characters)",
                        Style::default().fg(Color::DarkGray),
                    )),
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
                    "  Enter: Submit | Esc: Back",
                    Style::default().fg(Color::DarkGray),
                )));

                let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, centered);
            }
            Step::ConfirmAnswer => {
                let masked = "\u{2022}".repeat(self.confirm_answer.len());

                let mut lines = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "Re-enter your answer to confirm:",
                        Style::default().fg(Color::White),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("  Confirm: ", Style::default().fg(Color::White)),
                        Span::styled(masked, Style::default().fg(Color::Yellow)),
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
                    "  Enter: Submit | Esc: Back",
                    Style::default().fg(Color::DarkGray),
                )));

                let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, centered);
            }
        }
    }
}

fn centered_rect(percent: u16, r: Rect) -> Rect {
    let width = r.width * percent / 100;
    let x = r.x + (r.width - width) / 2;
    Rect {
        x,
        y: r.y,
        width,
        height: r.height,
    }
}
