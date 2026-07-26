use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::vault::model::Entry;
use zeroize::Zeroizing;

pub struct ViewEntryScreen {
    pub entry: Entry,
    revealed_secret: Option<Zeroizing<String>>,
    secret_revealed: bool,
    status_message: Option<(String, bool)>,
}

impl ViewEntryScreen {
    pub fn new(entry: Entry) -> Self {
        let revealed_secret = entry.reveal_secret(None).ok();
        Self {
            entry,
            revealed_secret,
            secret_revealed: false,
            status_message: None,
        }
    }

    pub fn new_with_secret(entry: Entry, secret: Zeroizing<String>) -> Self {
        Self {
            entry,
            revealed_secret: Some(secret),
            secret_revealed: false,
            status_message: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, _modifiers: KeyModifiers) -> ViewEntryAction {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => ViewEntryAction::Close,
            KeyCode::Char('r') => {
                self.secret_revealed = !self.secret_revealed;
                ViewEntryAction::Continue
            }
            KeyCode::Char('c') => {
                if self.secret_revealed {
                    self.revealed_secret
                        .as_ref()
                        .map(|secret| ViewEntryAction::Copy(secret.clone()))
                        .unwrap_or(ViewEntryAction::Continue)
                } else {
                    ViewEntryAction::Continue
                }
            }
            KeyCode::Char('u') => {
                if let Some(url) = self.entry.url.clone() {
                    ViewEntryAction::CopyUrl(url)
                } else {
                    ViewEntryAction::Continue
                }
            }
            KeyCode::Char('o') => self
                .entry
                .url
                .as_deref()
                .filter(|url| crate::links::is_web_url(url))
                .map(|url| ViewEntryAction::OpenUrl(url.to_string()))
                .unwrap_or(ViewEntryAction::Continue),
            _ => ViewEntryAction::Continue,
        }
    }

    pub fn set_status(&mut self, message: String, is_error: bool) {
        self.status_message = Some((message, is_error));
    }

    fn secret_display(&self) -> &str {
        if self.entry.has_secondary_password && self.revealed_secret.is_none() {
            "[Protected - secondary password required]"
        } else if self.secret_revealed {
            self.revealed_secret
                .as_ref()
                .map(|secret| secret.as_str())
                .unwrap_or("[Secret unavailable]")
        } else {
            "••••••••••••••••"
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(20),
                Constraint::Min(1),
            ])
            .split(area);

        let view_area = centered_rect(70, chunks[1]);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " Entry: {} ",
                crate::links::sanitize_terminal_text(&self.entry.name)
            ))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Cyan));

        frame.render_widget(block.clone(), view_area);

        let inner = block.inner(view_area);

        let mut lines = vec![];

        lines.push(Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                self.entry.secret_type.to_string(),
                Style::default().fg(Color::White),
            ),
        ]));

        lines.push(Line::from(""));

        if self.entry.secret_type.is_crypto_type() {
            lines.push(Line::from(vec![
                Span::styled("Network: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    crate::links::sanitize_terminal_text(&self.entry.network),
                    Style::default().fg(Color::White),
                ),
            ]));

            if let Some(ref addr) = self.entry.public_address {
                lines.push(Line::from(vec![
                    Span::styled("Public Address: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        crate::links::sanitize_terminal_text(addr),
                        Style::default().fg(Color::White),
                    ),
                ]));
            }
        } else if self.entry.secret_type.is_password_type() {
            if let Some(ref username) = self.entry.username {
                lines.push(Line::from(vec![
                    Span::styled("Username: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        crate::links::sanitize_terminal_text(username),
                        Style::default().fg(Color::White),
                    ),
                ]));
            }

            if let Some(ref url) = self.entry.url {
                lines.push(Line::from(vec![
                    Span::styled("URL: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        crate::links::sanitize_terminal_text(url),
                        Style::default().fg(Color::White),
                    ),
                ]));
            }
        }

        if !self.entry.notes.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Notes:",
                Style::default().fg(Color::Cyan),
            )]));
            lines.push(Line::from(crate::links::sanitize_terminal_text(
                &self.entry.notes,
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        if let Some((message, is_error)) = &self.status_message {
            lines.push(Line::from(vec![Span::styled(
                message.clone(),
                if *is_error {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                },
            )]));
            lines.push(Line::from(""));
        }

        let secret_display = self.secret_display();

        lines.push(Line::from(vec![
            Span::styled("Secret: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                crate::links::sanitize_terminal_text(secret_display),
                if self.secret_revealed {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        let has_url = self.entry.url.is_some();
        let has_openable_url = self
            .entry
            .url
            .as_deref()
            .is_some_and(crate::links::is_web_url);
        let help_text = if self.secret_revealed && has_openable_url {
            "r: Hide secret │ c: Copy secret │ u: Copy URL │ o: Open URL │ Esc/q: Close"
        } else if self.secret_revealed && has_url {
            "r: Hide secret │ c: Copy secret │ u: Copy URL │ Esc/q: Close"
        } else if self.secret_revealed {
            "r: Hide secret │ c: Copy secret │ Esc/q: Close"
        } else if has_openable_url {
            "r: Reveal secret │ u: Copy URL │ o: Open URL │ Esc/q: Close"
        } else if has_url {
            "r: Reveal secret │ u: Copy URL │ Esc/q: Close"
        } else {
            "r: Reveal secret │ Esc/q: Close"
        };

        lines.push(Line::from(vec![Span::styled(
            help_text,
            Style::default().fg(Color::DarkGray),
        )]));

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
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

pub enum ViewEntryAction {
    Continue,
    Copy(Zeroizing<String>),
    CopyUrl(String),
    OpenUrl(String),
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ratatui::{backend::TestBackend, Terminal};

    fn password_entry_with_url() -> Entry {
        Entry {
            name: "Example".to_string(),
            secret: "secret".to_string(),
            secret_type: crate::vault::model::SecretType::Password,
            network: String::new(),
            public_address: None,
            username: Some("user".to_string()),
            url: Some("https://example.com".to_string()),
            site_rules: Vec::new(),
            notes: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        }
    }

    #[test]
    fn u_key_copies_url_when_present() {
        let mut screen = ViewEntryScreen::new(password_entry_with_url());

        let action = screen.handle_key(KeyCode::Char('u'), KeyModifiers::NONE);

        match action {
            ViewEntryAction::CopyUrl(url) => assert_eq!(url, "https://example.com"),
            _ => panic!("expected CopyUrl action"),
        }
    }

    #[test]
    fn o_key_opens_url_when_present() {
        let mut screen = ViewEntryScreen::new(password_entry_with_url());

        let action = screen.handle_key(KeyCode::Char('o'), KeyModifiers::NONE);

        match action {
            ViewEntryAction::OpenUrl(url) => assert_eq!(url, "https://example.com"),
            _ => panic!("expected OpenUrl action"),
        }
    }

    #[test]
    fn revealed_secret_display_borrows_zeroizing_storage() {
        let mut screen = ViewEntryScreen::new_with_secret(
            password_entry_with_url(),
            Zeroizing::new("revealed-secret".to_string()),
        );
        screen.secret_revealed = true;

        let display = screen.secret_display();
        let backing = screen.revealed_secret.as_ref().unwrap().as_str();

        assert_eq!(display, "revealed-secret");
        assert_eq!(display.as_ptr(), backing.as_ptr());
        assert_eq!(display.len(), backing.len());
    }

    #[test]
    fn tui_view_rendering_exposes_no_entry_derived_controls() {
        let mut password_entry = password_entry_with_url();
        password_entry.name = "na\u{001b}me".to_string();
        password_entry.username = Some("us\u{009b}er".to_string());
        password_entry.url = Some("https://example.com/\u{001b}]0;owned".to_string());
        password_entry.notes = "no\u{0007}tes".to_string();
        let mut password_screen = ViewEntryScreen::new_with_secret(
            password_entry,
            Zeroizing::new("sec\u{001b}]0;owned\u{0085}ret".to_string()),
        );
        password_screen.secret_revealed = true;

        let password_rendered = render_screen(&password_screen);
        assert!(password_rendered.contains("name"));
        assert!(password_rendered.contains("user"));
        assert!(password_rendered.contains("https://example.com/]0;owned"));
        assert!(password_rendered.contains("notes"));
        assert!(password_rendered.contains("sec]0;ownedret"));
        assert!(!contains_terminal_control(&password_rendered));

        let mut crypto_entry = password_entry_with_url();
        crypto_entry.secret_type = crate::vault::model::SecretType::PrivateKey;
        crypto_entry.network = "net\u{001b}work".to_string();
        crypto_entry.public_address = Some("pub\u{009b}lic".to_string());
        let crypto_rendered = render_screen(&ViewEntryScreen::new(crypto_entry));
        assert!(crypto_rendered.contains("network"));
        assert!(crypto_rendered.contains("public"));
        assert!(!contains_terminal_control(&crypto_rendered));
    }

    #[test]
    fn open_url_action_requires_a_structurally_valid_https_url() {
        let mut entry = password_entry_with_url();
        entry.url = Some("https://example.com/$(whoami)".to_string());
        let mut screen = ViewEntryScreen::new(entry);

        assert!(matches!(
            screen.handle_key(KeyCode::Char('o'), KeyModifiers::NONE),
            ViewEntryAction::Continue
        ));
        assert!(!render_screen(&screen).contains("o: Open URL"));
    }

    fn render_screen(screen: &ViewEntryScreen) -> String {
        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| screen.render(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn contains_terminal_control(value: &str) -> bool {
        value
            .chars()
            .any(|ch| matches!(ch as u32, 0x00..=0x1f | 0x7f..=0x9f))
    }
}
