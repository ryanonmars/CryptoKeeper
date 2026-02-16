use chrono::Utc;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::vault::model::Entry;

pub struct EditEntryScreen {
    pub original_name: String,
    entry: Entry,
    current_field: usize,
}

impl EditEntryScreen {
    pub fn new(entry: Entry) -> Self {
        let original_name = entry.name.clone();
        Self {
            original_name,
            entry,
            current_field: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> EditEntryAction {
        if key == KeyCode::Esc {
            return EditEntryAction::Cancel;
        }

        if modifiers.contains(KeyModifiers::CONTROL) && key == KeyCode::Char('s') {
            return self.try_save();
        }

        match key {
            KeyCode::Tab => {
                self.current_field = (self.current_field + 1) % self.field_count();
                EditEntryAction::Continue
            }
            KeyCode::BackTab => {
                if self.current_field == 0 {
                    self.current_field = self.field_count() - 1;
                } else {
                    self.current_field -= 1;
                }
                EditEntryAction::Continue
            }
            KeyCode::Up => {
                if self.current_field > 0 {
                    self.current_field -= 1;
                }
                EditEntryAction::Continue
            }
            KeyCode::Down => {
                self.current_field = (self.current_field + 1) % self.field_count();
                EditEntryAction::Continue
            }
            KeyCode::Enter => {
                if self.current_field == self.field_count() - 1 {
                    return self.try_save();
                } else {
                    self.current_field = (self.current_field + 1) % self.field_count();
                }
                EditEntryAction::Continue
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_char(c);
                EditEntryAction::Continue
            }
            KeyCode::Backspace => {
                self.delete_char();
                EditEntryAction::Continue
            }
            _ => EditEntryAction::Continue,
        }
    }

    fn insert_char(&mut self, c: char) {
        match self.current_field {
            0 => self.entry.name.push(c),
            1 => {
                if self.is_password_type() {
                    if let Some(ref mut username) = self.entry.username {
                        username.push(c);
                    } else {
                        self.entry.username = Some(c.to_string());
                    }
                } else {
                    if let Some(ref mut addr) = self.entry.public_address {
                        addr.push(c);
                    } else {
                        self.entry.public_address = Some(c.to_string());
                    }
                }
            }
            2 => {
                if self.is_password_type() {
                    if let Some(ref mut url) = self.entry.url {
                        url.push(c);
                    } else {
                        self.entry.url = Some(c.to_string());
                    }
                } else {
                    self.entry.notes.push(c);
                }
            }
            3 => {
                if self.is_password_type() {
                    self.entry.notes.push(c);
                }
            }
            _ => {}
        }
    }

    fn delete_char(&mut self) {
        match self.current_field {
            0 => {
                self.entry.name.pop();
            }
            1 => {
                if self.is_password_type() {
                    if let Some(ref mut username) = self.entry.username {
                        username.pop();
                    }
                } else {
                    if let Some(ref mut addr) = self.entry.public_address {
                        addr.pop();
                    }
                }
            }
            2 => {
                if self.is_password_type() {
                    if let Some(ref mut url) = self.entry.url {
                        url.pop();
                    }
                } else {
                    self.entry.notes.pop();
                }
            }
            3 => {
                if self.is_password_type() {
                    self.entry.notes.pop();
                }
            }
            _ => {}
        }
    }

    fn field_count(&self) -> usize {
        if self.is_password_type() {
            4
        } else {
            3
        }
    }

    fn is_password_type(&self) -> bool {
        matches!(
            self.entry.secret_type,
            crate::vault::model::SecretType::Password
        )
    }

    fn try_save(&mut self) -> EditEntryAction {
        if self.entry.name.is_empty() {
            return EditEntryAction::Continue;
        }

        self.entry.updated_at = Utc::now();
        EditEntryAction::Save(self.entry.clone())
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(18), Constraint::Min(1)])
            .split(area);

        let form_area = centered_rect(70, chunks[1]);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Edit Entry ")
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(Color::Cyan));

        frame.render_widget(block.clone(), form_area);

        let inner = block.inner(form_area);
        let mut field_idx = 0;

        let mut lines = vec![];

        lines.push(self.render_field(field_idx, "Entry name", &self.entry.name));
        field_idx += 1;

        let is_password = matches!(
            self.entry.secret_type,
            crate::vault::model::SecretType::Password
        );

        if !is_password {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Network: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    self.entry.network.clone(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));

            lines.push(Line::from(""));
            let addr_value = self
                .entry
                .public_address
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("");
            lines.push(self.render_field(
                field_idx,
                "Public address (optional)",
                addr_value,
            ));
            field_idx += 1;
        } else {
            lines.push(Line::from(""));
            let username_value = self
                .entry
                .username
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("");
            lines.push(self.render_field(field_idx, "Username (optional)", username_value));
            field_idx += 1;

            lines.push(Line::from(""));
            let url_value = self.entry.url.as_ref().map(|s| s.as_str()).unwrap_or("");
            lines.push(self.render_field(field_idx, "URL (optional)", url_value));
            field_idx += 1;
        }

        lines.push(Line::from(""));
        lines.push(self.render_field(field_idx, "Notes (optional)", &self.entry.notes));

        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "Type: ",
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("{} (cannot be changed)", self.entry.secret_type),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Tab: Next field │ Shift+Tab: Previous │ Enter: Save │ Esc: Cancel",
            Style::default().fg(Color::DarkGray),
        )]));

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    fn render_field<'a>(&self, idx: usize, label: &str, value: &'a str) -> Line<'a> {
        let is_active = self.current_field == idx;
        let label_style = if is_active {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let value_style = if is_active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Gray)
        };

        let cursor = if is_active { "█" } else { "" };

        Line::from(vec![
            Span::styled(format!("{}: ", label), label_style),
            Span::styled(value, value_style),
            Span::styled(cursor, Style::default().fg(Color::Cyan)),
        ])
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

pub enum EditEntryAction {
    Continue,
    Save(Entry),
    Cancel,
}
