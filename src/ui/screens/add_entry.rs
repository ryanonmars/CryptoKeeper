use chrono::Utc;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::vault::model::{Entry, SecretType};

pub struct AddEntryScreen {
    current_field: usize,
    name: String,
    secret_type: SecretType,
    secret: String,
    secret_confirm: String,
    network: String,
    public_address: String,
    username: String,
    url: String,
    notes: String,
    show_type_select: bool,
    type_selected: usize,
    show_network_select: bool,
    network_selected: usize,
    scroll_offset: usize,
}

impl AddEntryScreen {
    pub fn new() -> Self {
        Self {
            current_field: 0,
            name: String::new(),
            secret_type: SecretType::PrivateKey,
            secret: String::new(),
            secret_confirm: String::new(),
            network: "Ethereum".to_string(),
            public_address: String::new(),
            username: String::new(),
            url: String::new(),
            notes: String::new(),
            show_type_select: false,
            type_selected: 0,
            show_network_select: false,
            network_selected: 0,
            scroll_offset: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> AddEntryAction {
        if key == KeyCode::Esc {
            return AddEntryAction::Cancel;
        }

        if modifiers.contains(KeyModifiers::CONTROL) && key == KeyCode::Char('s') {
            return self.try_save();
        }

        if self.show_type_select {
            return self.handle_type_select(key);
        }

        if self.show_network_select {
            return self.handle_network_select(key);
        }

        match key {
            KeyCode::Tab => {
                self.current_field = (self.current_field + 1) % self.field_count();
                AddEntryAction::Continue
            }
            KeyCode::BackTab => {
                if self.current_field == 0 {
                    self.current_field = self.field_count() - 1;
                } else {
                    self.current_field -= 1;
                }
                AddEntryAction::Continue
            }
            KeyCode::Up => {
                if self.current_field > 0 {
                    self.current_field -= 1;
                }
                AddEntryAction::Continue
            }
            KeyCode::Down => {
                self.current_field = (self.current_field + 1) % self.field_count();
                AddEntryAction::Continue
            }
            KeyCode::Enter => {
                if self.current_field == 1 {
                    self.show_type_select = true;
                } else if self.is_crypto_type() && self.current_field == 4 {
                    self.show_network_select = true;
                } else if self.current_field == self.field_count() - 1 {
                    return self.try_save();
                } else {
                    self.current_field = (self.current_field + 1) % self.field_count();
                }
                AddEntryAction::Continue
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_char(c);
                AddEntryAction::Continue
            }
            KeyCode::Backspace => {
                self.delete_char();
                AddEntryAction::Continue
            }
            _ => AddEntryAction::Continue,
        }
    }

    fn handle_type_select(&mut self, key: KeyCode) -> AddEntryAction {
        match key {
            KeyCode::Up => {
                if self.type_selected > 0 {
                    self.type_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.type_selected < 2 {
                    self.type_selected += 1;
                }
            }
            KeyCode::Enter => {
                self.secret_type = match self.type_selected {
                    0 => SecretType::PrivateKey,
                    1 => SecretType::SeedPhrase,
                    _ => SecretType::Password,
                };
                self.show_type_select = false;
                self.current_field += 1;
            }
            KeyCode::Esc => {
                self.show_type_select = false;
            }
            _ => {}
        }
        AddEntryAction::Continue
    }

    fn handle_network_select(&mut self, key: KeyCode) -> AddEntryAction {
        match key {
            KeyCode::Up => {
                if self.network_selected > 0 {
                    self.network_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.network_selected < 3 {
                    self.network_selected += 1;
                }
            }
            KeyCode::Enter => {
                self.network = match self.network_selected {
                    0 => "Ethereum",
                    1 => "Bitcoin",
                    2 => "Solana",
                    _ => "Other",
                }
                .to_string();
                self.show_network_select = false;
                self.current_field += 1;
            }
            KeyCode::Esc => {
                self.show_network_select = false;
            }
            _ => {}
        }
        AddEntryAction::Continue
    }

    fn insert_char(&mut self, c: char) {
        match self.current_field {
            0 => self.name.push(c),
            2 => self.secret.push(c),
            3 => self.secret_confirm.push(c),
            5 => self.public_address.push(c),
            6 => self.username.push(c),
            7 => self.url.push(c),
            8 => self.notes.push(c),
            _ => {}
        }
    }

    fn delete_char(&mut self) {
        match self.current_field {
            0 => {
                self.name.pop();
            }
            2 => {
                self.secret.pop();
            }
            3 => {
                self.secret_confirm.pop();
            }
            5 => {
                self.public_address.pop();
            }
            6 => {
                self.username.pop();
            }
            7 => {
                self.url.pop();
            }
            8 => {
                self.notes.pop();
            }
            _ => {}
        }
    }

    fn field_count(&self) -> usize {
        if self.is_crypto_type() {
            7  // name, type, secret, confirm, network, address, notes
        } else {
            7  // name, type, secret, confirm, username, url, notes
        }
    }

    fn is_crypto_type(&self) -> bool {
        !matches!(self.secret_type, SecretType::Password)
    }

    fn try_save(&self) -> AddEntryAction {
        if self.name.is_empty() {
            return AddEntryAction::Continue;
        }

        if self.secret.is_empty() || self.secret != self.secret_confirm {
            return AddEntryAction::Continue;
        }

        let now = Utc::now();
        let entry = Entry {
            name: self.name.clone(),
            secret: self.secret.clone(),
            secret_type: self.secret_type.clone(),
            network: self.network.clone(),
            public_address: if self.public_address.is_empty() {
                None
            } else {
                Some(self.public_address.clone())
            },
            username: if self.username.is_empty() {
                None
            } else {
                Some(self.username.clone())
            },
            url: if self.url.is_empty() {
                None
            } else {
                Some(self.url.clone())
            },
            notes: self.notes.clone(),
            created_at: now,
            updated_at: now,
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        };

        AddEntryAction::Save(entry)
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(22), Constraint::Min(1)])
            .split(area);

        let form_area = centered_rect(80, chunks[1]);

        if self.show_type_select {
            self.render_type_select(frame, form_area);
            return;
        }

        if self.show_network_select {
            self.render_network_select(frame, form_area);
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Add New Entry ")
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(Color::Cyan));

        frame.render_widget(block.clone(), form_area);

        let inner = block.inner(form_area);
        
        // Calculate visible area and scroll offset
        let available_height = inner.height as usize;
        let total_fields = self.field_count();
        
        // Ensure current field is visible
        let mut scroll_offset = self.scroll_offset;
        if self.current_field >= scroll_offset + available_height / 2 {
            scroll_offset = self.current_field.saturating_sub(available_height / 2 - 1);
        } else if self.current_field < scroll_offset {
            scroll_offset = self.current_field;
        }

        let mut lines = vec![];
        let mut field_idx = 0;

        lines.push(self.render_field(field_idx, "Entry name", &self.name, false));
        field_idx += 1;

        lines.push(Line::from(""));
        let secret_type_str = self.secret_type.to_string();
        lines.push(self.render_field(
            field_idx,
            "Secret type",
            &secret_type_str,
            false,
        ));
        field_idx += 1;

        lines.push(Line::from(""));
        let secret_masked = "•".repeat(self.secret.len());
        lines.push(self.render_field(field_idx, "Secret", &secret_masked, false));
        field_idx += 1;

        lines.push(Line::from(""));
        let secret_confirm_masked = "•".repeat(self.secret_confirm.len());
        lines.push(self.render_field(
            field_idx,
            "Confirm secret",
            &secret_confirm_masked,
            false,
        ));
        field_idx += 1;

        if self.is_crypto_type() {
            lines.push(Line::from(""));
            lines.push(self.render_field(field_idx, "Network", &self.network, false));
            field_idx += 1;

            lines.push(Line::from(""));
            lines.push(self.render_field(
                field_idx,
                "Public address (optional)",
                &self.public_address,
                false,
            ));
            field_idx += 1;
        } else {
            lines.push(Line::from(""));
            lines.push(self.render_field(
                field_idx,
                "Username (optional)",
                &self.username,
                false,
            ));
            field_idx += 1;

            lines.push(Line::from(""));
            lines.push(self.render_field(field_idx, "URL (optional)", &self.url, false));
            field_idx += 1;
        }

        lines.push(Line::from(""));
        lines.push(self.render_field(field_idx, "Notes (optional)", &self.notes, false));

        lines.push(Line::from(""));
        lines.push(Line::from(""));
        
        let help_text = if self.current_field == 1 {
            "↑↓: Scroll │ Enter: Select │ Tab: Next │ Esc: Cancel"
        } else if self.is_crypto_type() && self.current_field == 4 {
            "↑↓: Scroll │ Enter: Select │ Tab: Next │ Esc: Cancel"
        } else {
            "↑↓: Scroll │ Tab: Next │ Shift+Tab: Previous │ Ctrl+S: Save │ Esc: Cancel"
        };
        
        lines.push(Line::from(vec![Span::styled(
            help_text,
            Style::default().fg(Color::DarkGray),
        )]));

        // Skip lines based on scroll offset
        let visible_lines: Vec<Line> = lines.into_iter().skip(scroll_offset * 2).take(available_height).collect();

        let paragraph = Paragraph::new(visible_lines);
        frame.render_widget(paragraph, inner);
    }

    fn render_field<'a>(&self, idx: usize, label: &str, value: &'a str, _multiline: bool) -> Line<'a> {
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

    fn render_type_select(&self, frame: &mut Frame, area: Rect) {
        let types = ["Private Key", "Seed Phrase", "Password"];
        let items: Vec<ListItem> = types
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let prefix = if i == self.type_selected { "▸ " } else { "  " };
                let style = if i == self.type_selected {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{}{}", prefix, t)).style(style)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Secret Type (↑/↓ to navigate, Enter to select) ")
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(list, area);
    }

    fn render_network_select(&self, frame: &mut Frame, area: Rect) {
        let networks = ["Ethereum", "Bitcoin", "Solana", "Other"];
        let items: Vec<ListItem> = networks
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let prefix = if i == self.network_selected {
                    "▸ "
                } else {
                    "  "
                };
                let style = if i == self.network_selected {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{}{}", prefix, n)).style(style)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Network (↑/↓ to navigate, Enter to select) ")
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(list, area);
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

pub enum AddEntryAction {
    Continue,
    Save(Entry),
    Cancel,
}
