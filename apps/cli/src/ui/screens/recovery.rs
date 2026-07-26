use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::borrow::Cow;
use zeroize::{Zeroize, Zeroizing};

use crate::config::{model::RecoveryConfigV2, RecoveryConfig};

pub const LEGACY_RECOVERY_UNSUPPORTED: &str = "Legacy security-question recovery is no longer \
supported. Unlock with your master password, then configure a new recovery phrase.";

enum RecoveryStep {
    UnsupportedLegacy,
    Phrase,
    NewPassword,
    ConfirmPassword,
}

pub struct RecoveryScreen {
    config: Option<RecoveryConfigV2>,
    step: RecoveryStep,
    phrase: Zeroizing<String>,
    new_password: Zeroizing<String>,
    input: Zeroizing<String>,
    phrase_revealed: bool,
    error: Option<String>,
}

pub enum RecoveryAction {
    Continue,
    Cancel,
    UnsupportedLegacy,
    Recover {
        config: RecoveryConfigV2,
        phrase: Zeroizing<String>,
        new_password: Zeroizing<String>,
    },
    DeleteVault,
}

impl RecoveryScreen {
    pub fn new(recovery_config: RecoveryConfig) -> Self {
        let (config, step) = match recovery_config {
            RecoveryConfig::Legacy(_) => (None, RecoveryStep::UnsupportedLegacy),
            RecoveryConfig::V2(config) => (Some(config), RecoveryStep::Phrase),
        };
        Self {
            config,
            step,
            phrase: Zeroizing::new(String::new()),
            new_password: Zeroizing::new(String::new()),
            input: Zeroizing::new(String::new()),
            phrase_revealed: false,
            error: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> RecoveryAction {
        if key == KeyCode::F(2) {
            return RecoveryAction::DeleteVault;
        }
        if key == KeyCode::Esc {
            return RecoveryAction::Cancel;
        }
        if matches!(self.step, RecoveryStep::UnsupportedLegacy) {
            return if key == KeyCode::Enter {
                RecoveryAction::UnsupportedLegacy
            } else {
                RecoveryAction::Continue
            };
        }
        if key == KeyCode::F(3) && matches!(self.step, RecoveryStep::Phrase) {
            self.phrase_revealed = !self.phrase_revealed;
            return RecoveryAction::Continue;
        }

        match key {
            KeyCode::Char(character) if !modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.push(character);
                self.error = None;
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.error = None;
            }
            KeyCode::Enter => match self.step {
                RecoveryStep::Phrase => {
                    if self.input.trim().is_empty() {
                        self.error = Some("Enter your 24-word recovery phrase.".into());
                    } else {
                        self.phrase = std::mem::take(&mut self.input);
                        self.phrase_revealed = false;
                        self.step = RecoveryStep::NewPassword;
                    }
                }
                RecoveryStep::NewPassword => {
                    if self.input.is_empty() {
                        self.error = Some("Password cannot be empty.".into());
                    } else {
                        self.new_password = std::mem::take(&mut self.input);
                        self.step = RecoveryStep::ConfirmPassword;
                    }
                }
                RecoveryStep::ConfirmPassword => {
                    if *self.input != *self.new_password {
                        self.input.zeroize();
                        self.error = Some("Passwords do not match.".into());
                    } else {
                        self.input.zeroize();
                        return RecoveryAction::Recover {
                            config: self.config.clone().expect("V2 recovery config is present"),
                            phrase: std::mem::take(&mut self.phrase),
                            new_password: std::mem::take(&mut self.new_password),
                        };
                    }
                }
                RecoveryStep::UnsupportedLegacy => {}
            },
            _ => {}
        }
        RecoveryAction::Continue
    }

    pub fn retry_after_failure(&mut self, error: String) {
        self.phrase.zeroize();
        self.new_password.zeroize();
        self.input.zeroize();
        self.step = RecoveryStep::Phrase;
        self.phrase_revealed = false;
        self.error = Some(error);
    }

    pub fn render(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(12),
                Constraint::Min(1),
            ])
            .split(frame.area());
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Password Recovery ")
            .title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Yellow));

        let prompt = match self.step {
            RecoveryStep::UnsupportedLegacy => LEGACY_RECOVERY_UNSUPPORTED,
            RecoveryStep::Phrase => "Enter your 24-word recovery phrase:",
            RecoveryStep::NewPassword => "Choose a new master password:",
            RecoveryStep::ConfirmPassword => "Confirm your new master password:",
        };
        let input_line = match self.step {
            RecoveryStep::Phrase => {
                let phrase: Cow<'_, str> = if self.phrase_revealed {
                    Cow::Borrowed(self.input.as_str())
                } else {
                    Cow::Owned("•".repeat(self.input.chars().count()))
                };
                Line::from(vec![Span::raw("  "), Span::raw(phrase), Span::raw("█")])
            }
            RecoveryStep::NewPassword | RecoveryStep::ConfirmPassword => Line::from(vec![
                Span::raw("  "),
                Span::raw("*".repeat(self.input.chars().count())),
                Span::raw("█"),
            ]),
            RecoveryStep::UnsupportedLegacy => Line::from(""),
        };
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(prompt, Style::default().fg(Color::White))),
            Line::from(""),
            input_line,
        ];
        if let Some(error) = &self.error {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                error.as_str(),
                Style::default().fg(Color::Red),
            )));
        }
        lines.push(Line::from(""));
        if matches!(self.step, RecoveryStep::Phrase) {
            lines.push(Line::from(vec![
                Span::styled("F3", Style::default().fg(Color::Cyan)),
                Span::styled(
                    if self.phrase_revealed {
                        ": Hide phrase  "
                    } else {
                        ": Reveal phrase  "
                    },
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::styled(": Cancel  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "F2",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                ": Delete vault & start over",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, chunks[1]);
    }
}

impl Drop for RecoveryScreen {
    fn drop(&mut self) {
        self.phrase.zeroize();
        self.new_password.zeroize();
        self.input.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::format::VaultId;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn legacy_recovery_returns_approved_unsupported_message() {
        let legacy: crate::config::Config = serde_json::from_str(
            r#"{"recovery":{"question_index":1,"answer_hash":[1],"answer_salt":[2],
            "master_key_blob":[3],"master_key_blob_nonce":[4],"master_key_blob_salt":[5]}}"#,
        )
        .unwrap();
        let mut screen = RecoveryScreen::new(legacy.recovery.unwrap());

        assert!(matches!(
            screen.handle_key(KeyCode::Enter, KeyModifiers::NONE),
            RecoveryAction::UnsupportedLegacy
        ));
        assert_eq!(
            LEGACY_RECOVERY_UNSUPPORTED,
            "Legacy security-question recovery is no longer supported. Unlock with your master password, then configure a new recovery phrase."
        );
    }

    #[test]
    fn v2_recovery_collects_phrase_and_new_password() {
        let config = RecoveryConfig::V2(RecoveryConfigV2 {
            version: 2,
            vault_id: VaultId([0x81; 16]),
            salt: vec![0x82; 32],
            nonce: vec![0x83; 24],
            wrapped_dek: vec![0x84; 48],
        });
        let mut screen = RecoveryScreen::new(config);
        for value in ["phrase words", "new-password", "new-password"] {
            for character in value.chars() {
                screen.handle_key(KeyCode::Char(character), KeyModifiers::NONE);
            }
            let action = screen.handle_key(KeyCode::Enter, KeyModifiers::NONE);
            if value == "new-password" && matches!(action, RecoveryAction::Recover { .. }) {
                assert!(
                    screen.config.is_some(),
                    "the recovery config must remain available for a failed attempt"
                );
                return;
            }
        }
        panic!("recovery submission was not produced");
    }

    #[test]
    fn wrong_phrase_resets_recovery_screen_for_retry() {
        let vault_id = VaultId([0x91; 16]);
        let dek = [0x92; 32];
        let phrase = crate::crypto::recovery::generate_recovery_phrase().unwrap();
        let wrong_phrase = crate::crypto::recovery::generate_recovery_phrase().unwrap();
        let config =
            crate::crypto::recovery::create_recovery_config(vault_id, &dek, &phrase).unwrap();
        let mut screen = RecoveryScreen::new(RecoveryConfig::V2(config.clone()));
        for value in [wrong_phrase.as_str(), "new-password", "new-password"] {
            for character in value.chars() {
                screen.handle_key(KeyCode::Char(character), KeyModifiers::NONE);
            }
            let action = screen.handle_key(KeyCode::Enter, KeyModifiers::NONE);
            if let RecoveryAction::Recover {
                phrase,
                new_password,
                ..
            } = action
            {
                let error =
                    crate::crypto::recovery::recover_dek(&config, vault_id, &phrase).unwrap_err();
                drop(phrase);
                drop(new_password);
                screen.retry_after_failure(error.to_string());
                assert!(matches!(screen.step, RecoveryStep::Phrase));
                assert!(screen.config.is_some());
                assert!(screen.phrase.is_empty());
                assert!(screen.new_password.is_empty());
                assert!(screen.input.is_empty());
                assert!(screen.error.is_some());
                return;
            }
        }
        panic!("recovery submission was not produced");
    }

    fn rendered_text(screen: &mut RecoveryScreen) -> String {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| screen.render(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn recovery_phrase_is_masked_until_explicitly_revealed() {
        let config = RecoveryConfig::V2(RecoveryConfigV2 {
            version: 2,
            vault_id: VaultId([0x71; 16]),
            salt: vec![0x72; 32],
            nonce: vec![0x73; 24],
            wrapped_dek: vec![0x74; 48],
        });
        let mut screen = RecoveryScreen::new(config);
        for character in "alpha beta gamma".chars() {
            screen.handle_key(KeyCode::Char(character), KeyModifiers::NONE);
        }

        let masked = rendered_text(&mut screen);
        assert!(!masked.contains("alpha beta gamma"));

        screen.handle_key(KeyCode::F(3), KeyModifiers::NONE);
        let revealed = rendered_text(&mut screen);
        assert!(revealed.contains("alpha beta gamma"));
    }
}
