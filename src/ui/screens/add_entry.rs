use chrono::Utc;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use zeroize::Zeroize;

use crate::crypto::derive::derive_address;
use crate::crypto::entry_key;
use crate::vault::model::{Entry, SecretType};

pub struct AddEntryScreen {
    current_field: usize,
    name: String,
    secret_type: SecretType,
    secret: String,
    secret_confirm: String,
    network: String,
    username: String,
    url: String,
    notes: String,
    use_secondary_password: bool,
    secondary_password: String,
    secondary_password_confirm: String,
    show_type_select: bool,
    type_selected: usize,
    show_network_select: bool,
    network_selected: usize,
    scroll_offset: usize,
}

impl Drop for AddEntryScreen {
    fn drop(&mut self) {
        self.secret.zeroize();
        self.secret_confirm.zeroize();
        self.secondary_password.zeroize();
        self.secondary_password_confirm.zeroize();
    }
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
            username: String::new(),
            url: String::new(),
            notes: String::new(),
            use_secondary_password: false,
            secondary_password: String::new(),
            secondary_password_confirm: String::new(),
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
                // Secret type selector
                if self.current_field == 1 {
                    self.show_type_select = true;
                }
                // Network selector (crypto only, field index 4)
                else if self.is_crypto_type() && self.current_field == 4 {
                    self.show_network_select = true;
                }
                // Secondary password toggle
                else if self.current_field == self.secondary_toggle_field() {
                    self.use_secondary_password = !self.use_secondary_password;
                    if !self.use_secondary_password {
                        self.secondary_password.zeroize();
                        self.secondary_password = String::new();
                        self.secondary_password_confirm.zeroize();
                        self.secondary_password_confirm = String::new();
                    }
                }
                // Last field -> save
                else if self.current_field == self.field_count() - 1 {
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

    /// Field index of the secondary password toggle.
    fn secondary_toggle_field(&self) -> usize {
        if self.is_crypto_type() {
            // name(0), type(1), secret(2), confirm(3), network(4), notes(5), toggle(6)
            6
        } else {
            // name(0), type(1), secret(2), confirm(3), username(4), url(5), notes(6), toggle(7)
            7
        }
    }

    fn insert_char(&mut self, c: char) {
        if self.is_crypto_type() {
            match self.current_field {
                0 => self.name.push(c),
                2 => self.secret.push(c),
                3 => self.secret_confirm.push(c),
                // 4 = network selector, no typing
                5 => self.notes.push(c),
                // 6 = toggle, no typing
                f if self.use_secondary_password && f == 7 => {
                    self.secondary_password.push(c);
                }
                f if self.use_secondary_password && f == 8 => {
                    self.secondary_password_confirm.push(c);
                }
                _ => {}
            }
        } else {
            match self.current_field {
                0 => self.name.push(c),
                2 => self.secret.push(c),
                3 => self.secret_confirm.push(c),
                4 => self.username.push(c),
                5 => self.url.push(c),
                6 => self.notes.push(c),
                // 7 = toggle, no typing
                f if self.use_secondary_password && f == 8 => {
                    self.secondary_password.push(c);
                }
                f if self.use_secondary_password && f == 9 => {
                    self.secondary_password_confirm.push(c);
                }
                _ => {}
            }
        }
    }

    fn delete_char(&mut self) {
        if self.is_crypto_type() {
            match self.current_field {
                0 => { self.name.pop(); }
                2 => { self.secret.pop(); }
                3 => { self.secret_confirm.pop(); }
                5 => { self.notes.pop(); }
                f if self.use_secondary_password && f == 7 => {
                    self.secondary_password.pop();
                }
                f if self.use_secondary_password && f == 8 => {
                    self.secondary_password_confirm.pop();
                }
                _ => {}
            }
        } else {
            match self.current_field {
                0 => { self.name.pop(); }
                2 => { self.secret.pop(); }
                3 => { self.secret_confirm.pop(); }
                4 => { self.username.pop(); }
                5 => { self.url.pop(); }
                6 => { self.notes.pop(); }
                f if self.use_secondary_password && f == 8 => {
                    self.secondary_password.pop();
                }
                f if self.use_secondary_password && f == 9 => {
                    self.secondary_password_confirm.pop();
                }
                _ => {}
            }
        }
    }

    fn field_count(&self) -> usize {
        let base = if self.is_crypto_type() {
            7 // name, type, secret, confirm, network, notes, toggle
        } else {
            8 // name, type, secret, confirm, username, url, notes, toggle
        };
        if self.use_secondary_password {
            base + 2 // secondary password + confirm
        } else {
            base
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

        if self.use_secondary_password {
            if self.secondary_password.is_empty()
                || self.secondary_password != self.secondary_password_confirm
            {
                return AddEntryAction::Continue;
            }
        }

        // Auto-derive public address for crypto types
        let public_address = if self.is_crypto_type() {
            match derive_address(&self.secret, &self.secret_type, &self.network) {
                Ok(addr) => addr,
                Err(_) => None, // Bad key format — save with no address
            }
        } else {
            None
        };

        let now = Utc::now();

        // Handle secondary password encryption
        let (has_secondary, secret_to_store, encrypted_secret, encrypted_secret_nonce,
            entry_key_wrapped, entry_key_nonce, entry_key_salt) = if self.use_secondary_password {
            let ek = entry_key::generate_entry_key();
            let (ct, ct_nonce) = match entry_key::encrypt_secret(&ek, &self.secret) {
                Ok(v) => v,
                Err(_) => return AddEntryAction::Continue,
            };
            let (wrapped, wrap_nonce, salt) =
                match entry_key::wrap_entry_key(&ek, &self.secondary_password) {
                    Ok(v) => v,
                    Err(_) => return AddEntryAction::Continue,
                };
            (
                true,
                "[encrypted]".to_string(),
                Some(ct),
                Some(ct_nonce),
                Some(wrapped),
                Some(wrap_nonce),
                Some(salt),
            )
        } else {
            (false, self.secret.clone(), None, None, None, None, None)
        };

        let entry = Entry {
            name: self.name.clone(),
            secret: secret_to_store,
            secret_type: self.secret_type.clone(),
            network: self.network.clone(),
            public_address,
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
            has_secondary_password: has_secondary,
            entry_key_wrapped,
            entry_key_nonce,
            entry_key_salt,
            encrypted_secret,
            encrypted_secret_nonce,
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

        // Ensure current field is visible
        let mut scroll_offset = self.scroll_offset;
        if self.current_field >= scroll_offset + available_height / 2 {
            scroll_offset = self.current_field.saturating_sub(available_height / 2 - 1);
        } else if self.current_field < scroll_offset {
            scroll_offset = self.current_field;
        }

        let mut lines = vec![];
        let mut field_idx = 0;

        // Field 0: Name
        lines.push(self.render_field(field_idx, "Entry name", &self.name, false));
        field_idx += 1;

        // Field 1: Secret type
        lines.push(Line::from(""));
        let secret_type_str = self.secret_type.to_string();
        lines.push(self.render_field(field_idx, "Secret type", &secret_type_str, false));
        field_idx += 1;

        // Field 2: Secret
        lines.push(Line::from(""));
        let secret_masked = "\u{2022}".repeat(self.secret.len());
        lines.push(self.render_field(field_idx, "Secret", &secret_masked, false));
        field_idx += 1;

        // Field 3: Confirm secret
        lines.push(Line::from(""));
        let secret_confirm_masked = "\u{2022}".repeat(self.secret_confirm.len());
        lines.push(self.render_field(field_idx, "Confirm secret", &secret_confirm_masked, false));
        field_idx += 1;

        if self.is_crypto_type() {
            // Field 4: Network
            lines.push(Line::from(""));
            lines.push(self.render_field(field_idx, "Network", &self.network, false));
            field_idx += 1;
        } else {
            // Field 4: Username
            lines.push(Line::from(""));
            lines.push(self.render_field(field_idx, "Username (optional)", &self.username, false));
            field_idx += 1;

            // Field 5: URL
            lines.push(Line::from(""));
            lines.push(self.render_field(field_idx, "URL (optional)", &self.url, false));
            field_idx += 1;
        }

        // Notes
        lines.push(Line::from(""));
        lines.push(self.render_field(field_idx, "Notes (optional)", &self.notes, false));
        field_idx += 1;

        // Secondary password toggle
        lines.push(Line::from(""));
        let toggle_value = if self.use_secondary_password { "Yes" } else { "No" };
        lines.push(self.render_field(field_idx, "Secondary password", toggle_value, false));
        field_idx += 1;

        // Secondary password fields (only when toggled on)
        let sp_masked = "\u{2022}".repeat(self.secondary_password.len());
        let sp_confirm_masked = "\u{2022}".repeat(self.secondary_password_confirm.len());
        if self.use_secondary_password {
            lines.push(Line::from(""));
            lines.push(self.render_field(field_idx, "Secondary pwd", &sp_masked, false));
            field_idx += 1;

            lines.push(Line::from(""));
            lines.push(self.render_field(field_idx, "Confirm secondary", &sp_confirm_masked, false));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        let help_text = if self.current_field == 1 {
            "\u{2191}\u{2193}: Scroll \u{2502} Enter: Select \u{2502} Tab: Next \u{2502} Esc: Cancel"
        } else if self.is_crypto_type() && self.current_field == 4 {
            "\u{2191}\u{2193}: Scroll \u{2502} Enter: Select \u{2502} Tab: Next \u{2502} Esc: Cancel"
        } else if self.current_field == self.secondary_toggle_field() {
            "\u{2191}\u{2193}: Scroll \u{2502} Enter: Toggle \u{2502} Tab: Next \u{2502} Ctrl+S: Save \u{2502} Esc: Cancel"
        } else {
            "\u{2191}\u{2193}: Scroll \u{2502} Tab: Next \u{2502} Shift+Tab: Previous \u{2502} Ctrl+S: Save \u{2502} Esc: Cancel"
        };

        lines.push(Line::from(vec![Span::styled(
            help_text,
            Style::default().fg(Color::DarkGray),
        )]));

        // Skip lines based on scroll offset
        let visible_lines: Vec<Line> = lines
            .into_iter()
            .skip(scroll_offset * 2)
            .take(available_height)
            .collect();

        let paragraph = Paragraph::new(visible_lines);
        frame.render_widget(paragraph, inner);
    }

    fn render_field<'a>(
        &self,
        idx: usize,
        label: &str,
        value: &'a str,
        _multiline: bool,
    ) -> Line<'a> {
        let is_active = self.current_field == idx;
        let label_style = if is_active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let value_style = if is_active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Gray)
        };

        let cursor = if is_active { "\u{2588}" } else { "" };

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
                let prefix = if i == self.type_selected {
                    "\u{25b8} "
                } else {
                    "  "
                };
                let style = if i == self.type_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{}{}", prefix, t)).style(style)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Secret Type (\u{2191}/\u{2193} to navigate, Enter to select) ")
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
                    "\u{25b8} "
                } else {
                    "  "
                };
                let style = if i == self.network_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{}{}", prefix, n)).style(style)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Network (\u{2191}/\u{2193} to navigate, Enter to select) ")
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
