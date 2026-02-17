use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::config::model::RECOVERY_QUESTIONS;

#[derive(Clone)]
enum WizardStep {
    Welcome,
    SetPassword,
    ConfirmPassword,
    RecoveryChoice,
    RecoveryQuestion,
    RecoveryAnswer,
    RecoveryConfirmAnswer,
    Complete,
}

pub struct WizardResult {
    pub password: String,
    pub recovery: Option<(u8, String)>, // (question_index, answer)
}

pub struct WizardScreen {
    step: WizardStep,
    password: String,
    confirm_password: String,
    recovery_choice: bool,
    recovery_question_index: u8,
    recovery_answer: String,
    recovery_confirm_answer: String,
    error_message: Option<String>,
}

pub enum WizardAction {
    Continue,
    Complete(WizardResult),
    Cancel,
}

impl WizardScreen {
    pub fn new() -> Self {
        Self {
            step: WizardStep::Welcome,
            password: String::new(),
            confirm_password: String::new(),
            recovery_choice: true,
            recovery_question_index: 0,
            recovery_answer: String::new(),
            recovery_confirm_answer: String::new(),
            error_message: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> WizardAction {
        if key == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            return WizardAction::Cancel;
        }

        self.error_message = None;

        match self.step.clone() {
            WizardStep::Welcome => match key {
                KeyCode::Enter => {
                    self.step = WizardStep::SetPassword;
                    WizardAction::Continue
                }
                KeyCode::Esc => WizardAction::Cancel,
                _ => WizardAction::Continue,
            },

            WizardStep::SetPassword => match key {
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    self.password.push(c);
                    WizardAction::Continue
                }
                KeyCode::Backspace => {
                    self.password.pop();
                    WizardAction::Continue
                }
                KeyCode::Enter => {
                    if self.password.is_empty() {
                        self.error_message = Some("Password cannot be empty.".into());
                        WizardAction::Continue
                    } else {
                        self.step = WizardStep::ConfirmPassword;
                        WizardAction::Continue
                    }
                }
                KeyCode::Esc => {
                    self.password.clear();
                    self.step = WizardStep::Welcome;
                    WizardAction::Continue
                }
                _ => WizardAction::Continue,
            },

            WizardStep::ConfirmPassword => match key {
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    self.confirm_password.push(c);
                    WizardAction::Continue
                }
                KeyCode::Backspace => {
                    self.confirm_password.pop();
                    WizardAction::Continue
                }
                KeyCode::Enter => {
                    if self.confirm_password != self.password {
                        self.error_message = Some("Passwords do not match.".into());
                        self.confirm_password.clear();
                        WizardAction::Continue
                    } else {
                        self.step = WizardStep::RecoveryChoice;
                        WizardAction::Continue
                    }
                }
                KeyCode::Esc => {
                    self.confirm_password.clear();
                    self.step = WizardStep::SetPassword;
                    WizardAction::Continue
                }
                _ => WizardAction::Continue,
            },

            WizardStep::RecoveryChoice => match key {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.recovery_choice = true;
                    self.step = WizardStep::RecoveryQuestion;
                    WizardAction::Continue
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.recovery_choice = false;
                    self.step = WizardStep::Complete;
                    WizardAction::Continue
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                    self.recovery_choice = !self.recovery_choice;
                    WizardAction::Continue
                }
                KeyCode::Enter => {
                    if self.recovery_choice {
                        self.step = WizardStep::RecoveryQuestion;
                    } else {
                        self.step = WizardStep::Complete;
                    }
                    WizardAction::Continue
                }
                KeyCode::Esc => {
                    self.confirm_password.clear();
                    self.step = WizardStep::ConfirmPassword;
                    WizardAction::Continue
                }
                _ => WizardAction::Continue,
            },

            WizardStep::RecoveryQuestion => match key {
                KeyCode::Up => {
                    if self.recovery_question_index > 0 {
                        self.recovery_question_index -= 1;
                    }
                    WizardAction::Continue
                }
                KeyCode::Down => {
                    if (self.recovery_question_index as usize) < RECOVERY_QUESTIONS.len() - 1 {
                        self.recovery_question_index += 1;
                    }
                    WizardAction::Continue
                }
                KeyCode::Enter => {
                    self.step = WizardStep::RecoveryAnswer;
                    WizardAction::Continue
                }
                KeyCode::Esc => {
                    self.step = WizardStep::RecoveryChoice;
                    WizardAction::Continue
                }
                _ => WizardAction::Continue,
            },

            WizardStep::RecoveryAnswer => match key {
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    self.recovery_answer.push(c);
                    WizardAction::Continue
                }
                KeyCode::Backspace => {
                    self.recovery_answer.pop();
                    WizardAction::Continue
                }
                KeyCode::Enter => {
                    let trimmed = self.recovery_answer.trim();
                    if trimmed.len() < 3 {
                        self.error_message =
                            Some("Answer must be at least 3 characters.".into());
                        WizardAction::Continue
                    } else {
                        self.step = WizardStep::RecoveryConfirmAnswer;
                        WizardAction::Continue
                    }
                }
                KeyCode::Esc => {
                    self.recovery_answer.clear();
                    self.step = WizardStep::RecoveryQuestion;
                    WizardAction::Continue
                }
                _ => WizardAction::Continue,
            },

            WizardStep::RecoveryConfirmAnswer => match key {
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    self.recovery_confirm_answer.push(c);
                    WizardAction::Continue
                }
                KeyCode::Backspace => {
                    self.recovery_confirm_answer.pop();
                    WizardAction::Continue
                }
                KeyCode::Enter => {
                    let a = crate::crypto::recovery::normalize_answer(&self.recovery_answer);
                    let b =
                        crate::crypto::recovery::normalize_answer(&self.recovery_confirm_answer);
                    if a != b {
                        self.error_message = Some("Answers do not match.".into());
                        self.recovery_confirm_answer.clear();
                        WizardAction::Continue
                    } else {
                        self.step = WizardStep::Complete;
                        WizardAction::Continue
                    }
                }
                KeyCode::Esc => {
                    self.recovery_confirm_answer.clear();
                    self.step = WizardStep::RecoveryAnswer;
                    WizardAction::Continue
                }
                _ => WizardAction::Continue,
            },

            WizardStep::Complete => match key {
                KeyCode::Enter => {
                    let recovery = if self.recovery_choice {
                        Some((
                            self.recovery_question_index,
                            crate::crypto::recovery::normalize_answer(&self.recovery_answer),
                        ))
                    } else {
                        None
                    };
                    WizardAction::Complete(WizardResult {
                        password: self.password.clone(),
                        recovery,
                    })
                }
                KeyCode::Esc => {
                    if self.recovery_choice {
                        self.step = WizardStep::RecoveryConfirmAnswer;
                    } else {
                        self.step = WizardStep::RecoveryChoice;
                    }
                    WizardAction::Continue
                }
                _ => WizardAction::Continue,
            },
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        // Progress indicator
        let step_num = match self.step {
            WizardStep::Welcome => 1,
            WizardStep::SetPassword => 2,
            WizardStep::ConfirmPassword => 3,
            WizardStep::RecoveryChoice => 4,
            WizardStep::RecoveryQuestion => 5,
            WizardStep::RecoveryAnswer => 6,
            WizardStep::RecoveryConfirmAnswer => 7,
            WizardStep::Complete => 8,
        };
        let total = if self.recovery_choice { 8 } else { 5 };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);

        // Progress bar at top
        let progress = format!("Step {} of {}", step_num.min(total), total);
        let progress_para = Paragraph::new(progress)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(progress_para, chunks[0]);

        // Main content
        match &self.step {
            WizardStep::Welcome => self.render_welcome(frame, chunks[1]),
            WizardStep::SetPassword => {
                self.render_password_step(frame, chunks[1], "Set Master Password", &self.password)
            }
            WizardStep::ConfirmPassword => self.render_password_step(
                frame,
                chunks[1],
                "Confirm Master Password",
                &self.confirm_password,
            ),
            WizardStep::RecoveryChoice => self.render_recovery_choice(frame, chunks[1]),
            WizardStep::RecoveryQuestion => self.render_recovery_question(frame, chunks[1]),
            WizardStep::RecoveryAnswer => self.render_text_step(
                frame,
                chunks[1],
                "Recovery Answer",
                &RECOVERY_QUESTIONS[self.recovery_question_index as usize],
                &self.recovery_answer,
                false,
            ),
            WizardStep::RecoveryConfirmAnswer => self.render_text_step(
                frame,
                chunks[1],
                "Confirm Recovery Answer",
                "Re-enter your answer to confirm:",
                &self.recovery_confirm_answer,
                false,
            ),
            WizardStep::Complete => self.render_complete(frame, chunks[1]),
        }

        // Error message at bottom
        if let Some(ref error) = self.error_message {
            let error_para = Paragraph::new(error.as_str())
                .style(Style::default().fg(Color::Red))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(error_para, chunks[2]);
        } else {
            let hint = match self.step {
                WizardStep::Welcome => "Press Enter to begin  |  Esc to quit",
                WizardStep::Complete => "Press Enter to create vault  |  Esc to go back",
                _ => "Enter to continue  |  Esc to go back",
            };
            let hint_para = Paragraph::new(hint)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(hint_para, chunks[2]);
        }
    }

    fn render_welcome(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Welcome to CryptoKeeper!",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("CryptoKeeper is an encrypted vault for your"),
            Line::from("cryptocurrency private keys and seed phrases."),
            Line::from(""),
            Line::from("This wizard will help you:"),
            Line::from(""),
            Line::from(Span::styled(
                "  1. Set your master password",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(Span::styled(
                "  2. Optionally set up password recovery",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from("Your vault will be encrypted with XChaCha20-Poly1305"),
            Line::from("and your password will be processed with Argon2id."),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" CryptoKeeper Setup ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .alignment(ratatui::layout::Alignment::Center);

        let centered = center_vertical(area, 16);
        frame.render_widget(paragraph, centered);
    }

    fn render_password_step(&self, frame: &mut Frame, area: Rect, title: &str, buffer: &str) {
        let masked = "*".repeat(buffer.len());
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Enter your password:",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(masked, Style::default().fg(Color::Yellow)),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", title))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(text).block(block);
        let centered = center_vertical(area, 7);
        frame.render_widget(paragraph, centered);
    }

    fn render_recovery_choice(&self, frame: &mut Frame, area: Rect) {
        let yes_style = if self.recovery_choice {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };

        let no_style = if !self.recovery_choice {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red)
        };

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Set up a recovery question?",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("If you forget your master password, a recovery"),
            Line::from("question can help you regain access to your vault."),
            Line::from(""),
            Line::from(vec![
                Span::raw("         "),
                Span::styled(" Yes ", yes_style),
                Span::raw("   "),
                Span::styled(" No ", no_style),
            ]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Password Recovery ")
            .title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Yellow));

        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false });
        let centered = center_vertical(area, 10);
        frame.render_widget(paragraph, centered);
    }

    fn render_recovery_question(&self, frame: &mut Frame, area: Rect) {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Select a recovery question:",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
        ];

        for (i, question) in RECOVERY_QUESTIONS.iter().enumerate() {
            let style = if i == self.recovery_question_index as usize {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if i == self.recovery_question_index as usize {
                " > "
            } else {
                "   "
            };
            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, question),
                style,
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Use ↑/↓ to select, Enter to confirm",
            Style::default().fg(Color::DarkGray),
        )));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Recovery Question ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(lines).block(block);
        let centered = center_vertical(area, 10);
        frame.render_widget(paragraph, centered);
    }

    fn render_text_step(
        &self,
        frame: &mut Frame,
        area: Rect,
        title: &str,
        prompt: &str,
        buffer: &str,
        _masked: bool,
    ) {
        let display = buffer.to_string();
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(prompt, Style::default().fg(Color::White))),
            Line::from(""),
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(display, Style::default().fg(Color::Yellow)),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", title))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(text).block(block);
        let centered = center_vertical(area, 7);
        frame.render_widget(paragraph, centered);
    }

    fn render_complete(&self, frame: &mut Frame, area: Rect) {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Setup Complete!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Your vault will be created with these settings:"),
            Line::from(""),
            Line::from(Span::styled(
                "  Master password: set",
                Style::default().fg(Color::Yellow),
            )),
        ];

        if self.recovery_choice {
            let q = RECOVERY_QUESTIONS[self.recovery_question_index as usize];
            lines.push(Line::from(Span::styled(
                format!("  Recovery question: {}", q),
                Style::default().fg(Color::Yellow),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "  Recovery question: not set",
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press Enter to create your vault",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Ready ")
            .title_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Green));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .alignment(ratatui::layout::Alignment::Center);
        let centered = center_vertical(area, 12);
        frame.render_widget(paragraph, centered);
    }
}

fn center_vertical(area: Rect, height: u16) -> Rect {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(height),
            Constraint::Min(1),
        ])
        .split(area);
    chunks[1]
}
