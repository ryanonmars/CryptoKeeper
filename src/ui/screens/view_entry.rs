use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::vault::model::Entry;

pub struct ViewEntryScreen {
    pub entry: Entry,
    secret_revealed: bool,
}

impl ViewEntryScreen {
    pub fn new(entry: Entry) -> Self {
        Self {
            entry,
            secret_revealed: false,
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
                    ViewEntryAction::Copy(self.entry.secret.clone())
                } else {
                    ViewEntryAction::Continue
                }
            }
            _ => ViewEntryAction::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(20), Constraint::Min(1)])
            .split(area);

        let view_area = centered_rect(70, chunks[1]);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Entry: {} ", self.entry.name))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
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

        if !matches!(self.entry.secret_type, crate::vault::model::SecretType::Password) {
            lines.push(Line::from(vec![
                Span::styled("Network: ", Style::default().fg(Color::Cyan)),
                Span::styled(self.entry.network.clone(), Style::default().fg(Color::White)),
            ]));

            if let Some(ref addr) = self.entry.public_address {
                lines.push(Line::from(vec![
                    Span::styled("Public Address: ", Style::default().fg(Color::Cyan)),
                    Span::styled(addr.clone(), Style::default().fg(Color::White)),
                ]));
            }
        } else {
            if let Some(ref username) = self.entry.username {
                lines.push(Line::from(vec![
                    Span::styled("Username: ", Style::default().fg(Color::Cyan)),
                    Span::styled(username.clone(), Style::default().fg(Color::White)),
                ]));
            }

            if let Some(ref url) = self.entry.url {
                lines.push(Line::from(vec![
                    Span::styled("URL: ", Style::default().fg(Color::Cyan)),
                    Span::styled(url.clone(), Style::default().fg(Color::White)),
                ]));
            }
        }

        if !self.entry.notes.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Notes:",
                Style::default().fg(Color::Cyan),
            )]));
            lines.push(Line::from(self.entry.notes.clone()));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        let secret_display = if self.secret_revealed {
            self.entry.secret.clone()
        } else {
            "••••••••••••••••".to_string()
        };

        lines.push(Line::from(vec![
            Span::styled("Secret: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                secret_display,
                if self.secret_revealed {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        let help_text = if self.secret_revealed {
            "r: Hide secret │ c: Copy to clipboard │ Esc/q: Close"
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
    Copy(String),
    Close,
}
