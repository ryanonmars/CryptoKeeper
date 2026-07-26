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
use crate::crypto::passwords;
use crate::ui::text_edit;
use crate::vault::model::{Entry, SecretType};

pub struct AddEntryScreen {
    current_field: usize,
    name: String,
    name_cursor: usize,
    secret_type: SecretType,
    custom_secret_type: String,
    custom_secret_type_cursor: usize,
    secret: String,
    secret_cursor: usize,
    secret_confirm: String,
    secret_confirm_cursor: usize,
    generated_password: bool,
    network: String,
    custom_network: String,
    custom_network_cursor: usize,
    use_custom_network: bool,
    username: String,
    username_cursor: usize,
    url: String,
    url_cursor: usize,
    notes: String,
    notes_cursor: usize,
    use_secondary_password: bool,
    secondary_password: String,
    secondary_password_cursor: usize,
    secondary_password_confirm: String,
    secondary_password_confirm_cursor: usize,
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

impl Default for AddEntryScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl AddEntryScreen {
    pub fn new() -> Self {
        Self {
            current_field: 0,
            name: String::new(),
            name_cursor: 0,
            secret_type: SecretType::Password,
            custom_secret_type: String::new(),
            custom_secret_type_cursor: 0,
            secret: String::new(),
            secret_cursor: 0,
            secret_confirm: String::new(),
            secret_confirm_cursor: 0,
            generated_password: false,
            network: "Ethereum".to_string(),
            custom_network: String::new(),
            custom_network_cursor: 0,
            use_custom_network: false,
            username: String::new(),
            username_cursor: 0,
            url: String::new(),
            url_cursor: 0,
            notes: String::new(),
            notes_cursor: 0,
            use_secondary_password: false,
            secondary_password: String::new(),
            secondary_password_cursor: 0,
            secondary_password_confirm: String::new(),
            secondary_password_confirm_cursor: 0,
            show_type_select: false,
            type_selected: 0,
            show_network_select: false,
            network_selected: 0,
            scroll_offset: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> AddEntryAction {
        let movement_modifiers = KeyModifiers::ALT | KeyModifiers::CONTROL;

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
            KeyCode::Char('a') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_cursor_home();
                AddEntryAction::Continue
            }
            KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_cursor_end();
                AddEntryAction::Continue
            }
            KeyCode::Char('b') if modifiers.contains(KeyModifiers::ALT) => {
                self.move_cursor_word_left();
                AddEntryAction::Continue
            }
            KeyCode::Char('f') if modifiers.contains(KeyModifiers::ALT) => {
                self.move_cursor_word_right();
                AddEntryAction::Continue
            }
            KeyCode::Char('w') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.backspace_word();
                AddEntryAction::Continue
            }
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::ALT) => {
                self.delete_word();
                AddEntryAction::Continue
            }
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
                // Network selector (crypto only)
                else if self.is_crypto_type() && self.current_field == self.network_field() {
                    self.show_network_select = true;
                } else if self.has_generate_password_field()
                    && self.current_field == self.generate_password_field()
                {
                    self.generate_password();
                    self.current_field = (self.current_field + 1) % self.field_count();
                }
                // Secondary password toggle
                else if self.current_field == self.secondary_toggle_field() {
                    self.use_secondary_password = !self.use_secondary_password;
                    if !self.use_secondary_password {
                        self.secondary_password.zeroize();
                        self.secondary_password = String::new();
                        self.secondary_password_cursor = 0;
                        self.secondary_password_confirm.zeroize();
                        self.secondary_password_confirm = String::new();
                        self.secondary_password_confirm_cursor = 0;
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
                if modifiers.contains(KeyModifiers::ALT) {
                    return AddEntryAction::Continue;
                }
                self.insert_char(c);
                AddEntryAction::Continue
            }
            KeyCode::Left => {
                if modifiers.intersects(movement_modifiers) {
                    self.move_cursor_word_left();
                } else {
                    self.move_cursor_left();
                }
                AddEntryAction::Continue
            }
            KeyCode::Right => {
                if modifiers.intersects(movement_modifiers) {
                    self.move_cursor_word_right();
                } else {
                    self.move_cursor_right();
                }
                AddEntryAction::Continue
            }
            KeyCode::Home => {
                self.move_cursor_home();
                AddEntryAction::Continue
            }
            KeyCode::End => {
                self.move_cursor_end();
                AddEntryAction::Continue
            }
            KeyCode::Backspace => {
                if modifiers.intersects(movement_modifiers) {
                    self.backspace_word();
                } else {
                    self.backspace_char();
                }
                AddEntryAction::Continue
            }
            KeyCode::Delete => {
                if modifiers.intersects(movement_modifiers) {
                    self.delete_word();
                } else {
                    self.delete_char();
                }
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
                if self.type_selected < 3 {
                    self.type_selected += 1;
                }
            }
            KeyCode::Enter => {
                self.secret_type = match self.type_selected {
                    0 => SecretType::Password,
                    1 => SecretType::SeedPhrase,
                    2 => SecretType::PrivateKey,
                    _ => SecretType::Other(self.custom_secret_type.trim().to_string()),
                };
                if self.is_crypto_type() && self.network.is_empty() && !self.use_custom_network {
                    self.network = "Ethereum".to_string();
                }
                if !self.secret_type.is_other_type() {
                    self.custom_secret_type.clear();
                    self.custom_secret_type_cursor = 0;
                }
                if !self.is_password_type() {
                    self.generated_password = false;
                }
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
                match self.network_selected {
                    0 => {
                        self.network = "Ethereum".to_string();
                        self.use_custom_network = false;
                        self.custom_network.clear();
                        self.custom_network_cursor = 0;
                    }
                    1 => {
                        self.network = "Bitcoin".to_string();
                        self.use_custom_network = false;
                        self.custom_network.clear();
                        self.custom_network_cursor = 0;
                    }
                    2 => {
                        self.network = "Solana".to_string();
                        self.use_custom_network = false;
                        self.custom_network.clear();
                        self.custom_network_cursor = 0;
                    }
                    _ => {
                        self.use_custom_network = true;
                    }
                }
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
        self.notes_field() + 1
    }

    fn insert_char(&mut self, c: char) {
        match self.current_field {
            0 => text_edit::insert_char(&mut self.name, &mut self.name_cursor, c),
            f if self.has_custom_type_field() && f == self.custom_type_field() => {
                text_edit::insert_char(
                    &mut self.custom_secret_type,
                    &mut self.custom_secret_type_cursor,
                    c,
                );
            }
            f if f == self.secret_field() => {
                self.generated_password = false;
                text_edit::insert_char(&mut self.secret, &mut self.secret_cursor, c);
            }
            f if f == self.confirm_field() => {
                self.generated_password = false;
                text_edit::insert_char(
                    &mut self.secret_confirm,
                    &mut self.secret_confirm_cursor,
                    c,
                );
            }
            f if self.has_custom_network_field() && f == self.custom_network_field() => {
                text_edit::insert_char(
                    &mut self.custom_network,
                    &mut self.custom_network_cursor,
                    c,
                );
            }
            f if self.is_password_type() && f == self.username_field() => {
                text_edit::insert_char(&mut self.username, &mut self.username_cursor, c);
            }
            f if self.is_password_type() && f == self.url_field() => {
                text_edit::insert_char(&mut self.url, &mut self.url_cursor, c);
            }
            f if f == self.notes_field() => {
                text_edit::insert_char(&mut self.notes, &mut self.notes_cursor, c);
            }
            f if self.use_secondary_password && f == self.secondary_toggle_field() + 1 => {
                text_edit::insert_char(
                    &mut self.secondary_password,
                    &mut self.secondary_password_cursor,
                    c,
                );
            }
            f if self.use_secondary_password && f == self.secondary_toggle_field() + 2 => {
                text_edit::insert_char(
                    &mut self.secondary_password_confirm,
                    &mut self.secondary_password_confirm_cursor,
                    c,
                );
            }
            _ => {}
        }
    }

    fn backspace_char(&mut self) {
        match self.current_field {
            0 => {
                text_edit::backspace(&mut self.name, &mut self.name_cursor);
            }
            f if self.has_custom_type_field() && f == self.custom_type_field() => {
                text_edit::backspace(
                    &mut self.custom_secret_type,
                    &mut self.custom_secret_type_cursor,
                );
            }
            f if f == self.secret_field() => {
                self.generated_password = false;
                text_edit::backspace(&mut self.secret, &mut self.secret_cursor);
            }
            f if f == self.confirm_field() => {
                self.generated_password = false;
                text_edit::backspace(&mut self.secret_confirm, &mut self.secret_confirm_cursor);
            }
            f if self.has_custom_network_field() && f == self.custom_network_field() => {
                text_edit::backspace(&mut self.custom_network, &mut self.custom_network_cursor);
            }
            f if self.is_password_type() && f == self.username_field() => {
                text_edit::backspace(&mut self.username, &mut self.username_cursor);
            }
            f if self.is_password_type() && f == self.url_field() => {
                text_edit::backspace(&mut self.url, &mut self.url_cursor);
            }
            f if f == self.notes_field() => {
                text_edit::backspace(&mut self.notes, &mut self.notes_cursor);
            }
            f if self.use_secondary_password && f == self.secondary_toggle_field() + 1 => {
                text_edit::backspace(
                    &mut self.secondary_password,
                    &mut self.secondary_password_cursor,
                );
            }
            f if self.use_secondary_password && f == self.secondary_toggle_field() + 2 => {
                text_edit::backspace(
                    &mut self.secondary_password_confirm,
                    &mut self.secondary_password_confirm_cursor,
                );
            }
            _ => {}
        }
    }

    fn delete_char(&mut self) {
        match self.current_field {
            0 => {
                text_edit::delete(&mut self.name, &mut self.name_cursor);
            }
            f if self.has_custom_type_field() && f == self.custom_type_field() => {
                text_edit::delete(
                    &mut self.custom_secret_type,
                    &mut self.custom_secret_type_cursor,
                );
            }
            f if f == self.secret_field() => {
                self.generated_password = false;
                text_edit::delete(&mut self.secret, &mut self.secret_cursor);
            }
            f if f == self.confirm_field() => {
                self.generated_password = false;
                text_edit::delete(&mut self.secret_confirm, &mut self.secret_confirm_cursor);
            }
            f if self.has_custom_network_field() && f == self.custom_network_field() => {
                text_edit::delete(&mut self.custom_network, &mut self.custom_network_cursor);
            }
            f if self.is_password_type() && f == self.username_field() => {
                text_edit::delete(&mut self.username, &mut self.username_cursor);
            }
            f if self.is_password_type() && f == self.url_field() => {
                text_edit::delete(&mut self.url, &mut self.url_cursor);
            }
            f if f == self.notes_field() => {
                text_edit::delete(&mut self.notes, &mut self.notes_cursor);
            }
            f if self.use_secondary_password && f == self.secondary_toggle_field() + 1 => {
                text_edit::delete(
                    &mut self.secondary_password,
                    &mut self.secondary_password_cursor,
                );
            }
            f if self.use_secondary_password && f == self.secondary_toggle_field() + 2 => {
                text_edit::delete(
                    &mut self.secondary_password_confirm,
                    &mut self.secondary_password_confirm_cursor,
                );
            }
            _ => {}
        }
    }

    fn move_cursor_left(&mut self) {
        self.with_active_cursor(|cursor, _| text_edit::move_left(cursor));
    }

    fn move_cursor_right(&mut self) {
        self.with_active_cursor(text_edit::move_right);
    }

    fn move_cursor_home(&mut self) {
        self.with_active_cursor(|cursor, _| text_edit::move_home(cursor));
    }

    fn move_cursor_end(&mut self) {
        self.with_active_cursor(text_edit::move_end);
    }

    fn move_cursor_word_left(&mut self) {
        self.with_active_cursor(text_edit::move_word_left);
    }

    fn move_cursor_word_right(&mut self) {
        self.with_active_cursor(text_edit::move_word_right);
    }

    fn backspace_word(&mut self) {
        self.with_active_string(text_edit::backspace_word);
    }

    fn delete_word(&mut self) {
        self.with_active_string(text_edit::delete_word);
    }

    fn with_active_cursor(&mut self, mut edit: impl FnMut(&mut usize, &str)) {
        match self.current_field {
            0 => edit(&mut self.name_cursor, &self.name),
            f if self.has_custom_type_field() && f == self.custom_type_field() => {
                edit(
                    &mut self.custom_secret_type_cursor,
                    &self.custom_secret_type,
                );
            }
            f if f == self.secret_field() => edit(&mut self.secret_cursor, &self.secret),
            f if f == self.confirm_field() => {
                edit(&mut self.secret_confirm_cursor, &self.secret_confirm);
            }
            f if self.has_custom_network_field() && f == self.custom_network_field() => {
                edit(&mut self.custom_network_cursor, &self.custom_network);
            }
            f if self.is_password_type() && f == self.username_field() => {
                edit(&mut self.username_cursor, &self.username);
            }
            f if self.is_password_type() && f == self.url_field() => {
                edit(&mut self.url_cursor, &self.url);
            }
            f if f == self.notes_field() => edit(&mut self.notes_cursor, &self.notes),
            f if self.use_secondary_password && f == self.secondary_toggle_field() + 1 => {
                edit(
                    &mut self.secondary_password_cursor,
                    &self.secondary_password,
                );
            }
            f if self.use_secondary_password && f == self.secondary_toggle_field() + 2 => {
                edit(
                    &mut self.secondary_password_confirm_cursor,
                    &self.secondary_password_confirm,
                );
            }
            _ => {}
        }
    }

    fn with_active_string(&mut self, mut edit: impl FnMut(&mut String, &mut usize)) {
        match self.current_field {
            0 => edit(&mut self.name, &mut self.name_cursor),
            f if self.has_custom_type_field() && f == self.custom_type_field() => {
                edit(
                    &mut self.custom_secret_type,
                    &mut self.custom_secret_type_cursor,
                );
            }
            f if f == self.secret_field() => {
                self.generated_password = false;
                edit(&mut self.secret, &mut self.secret_cursor);
            }
            f if f == self.confirm_field() => {
                self.generated_password = false;
                edit(&mut self.secret_confirm, &mut self.secret_confirm_cursor);
            }
            f if self.has_custom_network_field() && f == self.custom_network_field() => {
                edit(&mut self.custom_network, &mut self.custom_network_cursor);
            }
            f if self.is_password_type() && f == self.username_field() => {
                edit(&mut self.username, &mut self.username_cursor);
            }
            f if self.is_password_type() && f == self.url_field() => {
                edit(&mut self.url, &mut self.url_cursor);
            }
            f if f == self.notes_field() => {
                edit(&mut self.notes, &mut self.notes_cursor);
            }
            f if self.use_secondary_password && f == self.secondary_toggle_field() + 1 => {
                edit(
                    &mut self.secondary_password,
                    &mut self.secondary_password_cursor,
                );
            }
            f if self.use_secondary_password && f == self.secondary_toggle_field() + 2 => {
                edit(
                    &mut self.secondary_password_confirm,
                    &mut self.secondary_password_confirm_cursor,
                );
            }
            _ => {}
        }
    }

    fn field_count(&self) -> usize {
        let mut base = 5; // name, type, secret, confirm, notes
        if self.has_custom_type_field() {
            base += 1;
        }
        if self.is_crypto_type() {
            base += 1; // network
        }
        if self.has_custom_network_field() {
            base += 1;
        }
        if self.is_password_type() {
            base += 3; // generate, username, url
        }
        base += 1; // toggle
        if self.use_secondary_password {
            base + 2 // secondary password + confirm
        } else {
            base
        }
    }

    fn is_crypto_type(&self) -> bool {
        self.secret_type.is_crypto_type()
    }

    fn is_password_type(&self) -> bool {
        self.secret_type.is_password_type()
    }

    fn has_custom_type_field(&self) -> bool {
        self.secret_type.is_other_type()
    }

    fn has_custom_network_field(&self) -> bool {
        self.is_crypto_type() && self.use_custom_network
    }

    fn custom_type_field(&self) -> usize {
        2
    }

    fn secret_field(&self) -> usize {
        2 + usize::from(self.has_custom_type_field())
    }

    fn confirm_field(&self) -> usize {
        self.secret_field() + 1
    }

    fn network_field(&self) -> usize {
        self.confirm_field() + 1
    }

    fn custom_network_field(&self) -> usize {
        self.network_field() + 1
    }

    fn username_field(&self) -> usize {
        self.confirm_field() + 1 + usize::from(self.has_generate_password_field())
    }

    fn url_field(&self) -> usize {
        self.username_field() + 1
    }

    fn notes_field(&self) -> usize {
        let mut idx = self.confirm_field() + 1;
        if self.is_crypto_type() {
            idx += 1;
            if self.has_custom_network_field() {
                idx += 1;
            }
        } else if self.is_password_type() {
            idx += 3;
        }
        idx
    }

    fn has_generate_password_field(&self) -> bool {
        self.is_password_type()
    }

    fn generate_password_field(&self) -> usize {
        self.confirm_field() + 1
    }

    fn effective_secret_type(&self) -> Option<SecretType> {
        match &self.secret_type {
            SecretType::Other(_) => {
                let label = self.custom_secret_type.trim();
                if label.is_empty() {
                    None
                } else {
                    Some(SecretType::Other(label.to_string()))
                }
            }
            other => Some(other.clone()),
        }
    }

    fn effective_network(&self) -> String {
        if !self.is_crypto_type() {
            return String::new();
        }

        if self.use_custom_network {
            self.custom_network.trim().to_string()
        } else {
            self.network.clone()
        }
    }

    fn generate_password(&mut self) {
        self.secret.zeroize();
        self.secret_confirm.zeroize();
        let generated = passwords::generate_password();
        self.secret = generated.clone();
        self.secret_confirm = generated;
        self.secret_cursor = text_edit::char_count(&self.secret);
        self.secret_confirm_cursor = text_edit::char_count(&self.secret_confirm);
        self.generated_password = true;
    }

    fn try_save(&self) -> AddEntryAction {
        if self.name.is_empty() {
            return AddEntryAction::Continue;
        }

        let Some(secret_type) = self.effective_secret_type() else {
            return AddEntryAction::Continue;
        };

        if self.secret.is_empty() || self.secret != self.secret_confirm {
            return AddEntryAction::Continue;
        }

        if self.use_secondary_password
            && (self.secondary_password.is_empty()
                || self.secondary_password != self.secondary_password_confirm)
        {
            return AddEntryAction::Continue;
        }

        let network = self.effective_network();
        if self.is_crypto_type() && network.is_empty() {
            return AddEntryAction::Continue;
        }

        // Auto-derive public address for crypto types
        let public_address = if self.is_crypto_type() {
            // Bad key formats are still saved without a derived address.
            derive_address(&self.secret, &self.secret_type, &network).unwrap_or_default()
        } else {
            None
        };

        let now = Utc::now();

        // Handle secondary password encryption
        let (
            has_secondary,
            secret_to_store,
            encrypted_secret,
            encrypted_secret_nonce,
            entry_key_wrapped,
            entry_key_nonce,
            entry_key_salt,
        ) = if self.use_secondary_password {
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
            secret_type,
            network,
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
            site_rules: Vec::new(),
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

        AddEntryAction::Save(Box::new(entry))
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(22),
                Constraint::Min(1),
            ])
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
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
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
        lines.push(self.render_field(field_idx, "Entry name", &self.name, self.name_cursor));
        field_idx += 1;

        // Field 1: Secret type
        lines.push(Line::from(""));
        let secret_type_str = self.secret_type.to_string();
        lines.push(self.render_field(field_idx, "Secret type", &secret_type_str, 0));
        field_idx += 1;

        if self.has_custom_type_field() {
            lines.push(Line::from(""));
            lines.push(self.render_field(
                field_idx,
                "Custom type",
                &self.custom_secret_type,
                self.custom_secret_type_cursor,
            ));
            field_idx += 1;
        }

        // Field 2: Secret
        lines.push(Line::from(""));
        let secret_masked = "\u{2022}".repeat(self.secret.len());
        lines.push(self.render_field(field_idx, "Secret", &secret_masked, self.secret_cursor));
        field_idx += 1;

        // Field 3: Confirm secret
        lines.push(Line::from(""));
        let secret_confirm_masked = "\u{2022}".repeat(self.secret_confirm.len());
        lines.push(self.render_field(
            field_idx,
            "Confirm secret",
            &secret_confirm_masked,
            self.secret_confirm_cursor,
        ));
        field_idx += 1;

        if self.is_password_type() {
            lines.push(Line::from(""));
            let generate_value = if self.generated_password {
                "Press Enter to regenerate (24 chars ready)"
            } else {
                "Press Enter to generate"
            };
            lines.push(self.render_field(field_idx, "Generate password", generate_value, 0));
            field_idx += 1;
        }

        if self.is_crypto_type() {
            // Field 4: Network
            lines.push(Line::from(""));
            let network_value = if self.use_custom_network {
                "Other"
            } else {
                &self.network
            };
            lines.push(self.render_field(field_idx, "Network", network_value, 0));
            field_idx += 1;

            if self.use_custom_network {
                lines.push(Line::from(""));
                lines.push(self.render_field(
                    field_idx,
                    "Custom network",
                    &self.custom_network,
                    self.custom_network_cursor,
                ));
                field_idx += 1;
            }
        } else if self.is_password_type() {
            // Field 4: Username
            lines.push(Line::from(""));
            lines.push(self.render_field(
                field_idx,
                "Username (optional)",
                &self.username,
                self.username_cursor,
            ));
            field_idx += 1;

            // Field 5: URL
            lines.push(Line::from(""));
            lines.push(self.render_field(field_idx, "URL (optional)", &self.url, self.url_cursor));
            field_idx += 1;
        }

        // Notes
        lines.push(Line::from(""));
        lines.push(self.render_field(
            field_idx,
            "Notes (optional)",
            &self.notes,
            self.notes_cursor,
        ));
        field_idx += 1;

        // Secondary password toggle
        lines.push(Line::from(""));
        let toggle_value = if self.use_secondary_password {
            "Yes"
        } else {
            "No"
        };
        lines.push(self.render_field(field_idx, "Secondary password", toggle_value, 0));
        field_idx += 1;

        // Secondary password fields (only when toggled on)
        let sp_masked = "\u{2022}".repeat(self.secondary_password.len());
        let sp_confirm_masked = "\u{2022}".repeat(self.secondary_password_confirm.len());
        if self.use_secondary_password {
            lines.push(Line::from(""));
            lines.push(self.render_field(
                field_idx,
                "Secondary pwd",
                &sp_masked,
                self.secondary_password_cursor,
            ));
            field_idx += 1;

            lines.push(Line::from(""));
            lines.push(self.render_field(
                field_idx,
                "Confirm secondary",
                &sp_confirm_masked,
                self.secondary_password_confirm_cursor,
            ));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        let help_text = if self.current_field == 1
            || (self.is_crypto_type() && self.current_field == self.network_field())
        {
            "\u{2191}\u{2193}: Scroll \u{2502} Enter: Select \u{2502} Tab: Next \u{2502} Esc: Cancel"
        } else if self.has_generate_password_field()
            && self.current_field == self.generate_password_field()
        {
            "\u{2191}\u{2193}: Scroll \u{2502} Enter: Generate \u{2502} Tab: Next \u{2502} Ctrl+S: Save \u{2502} Esc: Cancel"
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
        cursor_pos: usize,
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

        let (before_cursor, after_cursor) = text_edit::cursor_segments(value, cursor_pos);

        if is_active {
            Line::from(vec![
                Span::styled(format!("{}: ", label), label_style),
                Span::styled(before_cursor, value_style),
                Span::styled("\u{2588}", Style::default().fg(Color::Cyan)),
                Span::styled(after_cursor, value_style),
            ])
        } else {
            Line::from(vec![
                Span::styled(format!("{}: ", label), label_style),
                Span::styled(value, value_style),
            ])
        }
    }

    fn render_type_select(&self, frame: &mut Frame, area: Rect) {
        let types = ["Password", "Seed Phrase", "Private Key", "Other"];
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
    Save(Box<Entry>),
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_entries_do_not_keep_default_network() {
        let mut screen = AddEntryScreen::new();
        screen.name = "API Secret".to_string();
        screen.secret_type = SecretType::Password;
        screen.secret = "secret".to_string();
        screen.secret_confirm = "secret".to_string();

        let AddEntryAction::Save(entry) = screen.try_save() else {
            panic!("expected save action");
        };

        assert!(entry.network.is_empty());
        assert!(entry.public_address.is_none());
    }

    #[test]
    fn custom_secret_type_uses_typed_value() {
        let mut screen = AddEntryScreen::new();
        screen.name = "Service Secret".to_string();
        screen.secret_type = SecretType::Other(String::new());
        screen.custom_secret_type = "API".to_string();
        screen.secret = "secret".to_string();
        screen.secret_confirm = "secret".to_string();

        let AddEntryAction::Save(entry) = screen.try_save() else {
            panic!("expected save action");
        };

        assert_eq!(entry.secret_type, SecretType::Other("API".to_string()));
        assert!(entry.network.is_empty());
        assert!(entry.public_address.is_none());
    }

    #[test]
    fn custom_network_uses_typed_value() {
        let mut screen = AddEntryScreen::new();
        screen.name = "Custom Entry".to_string();
        screen.secret_type = SecretType::PrivateKey;
        screen.secret = "not-a-real-key".to_string();
        screen.secret_confirm = "not-a-real-key".to_string();
        screen.use_custom_network = true;
        screen.custom_network = "API".to_string();

        let AddEntryAction::Save(entry) = screen.try_save() else {
            panic!("expected save action");
        };

        assert_eq!(entry.network, "API");
    }

    #[test]
    fn generated_password_populates_both_secret_fields() {
        let mut screen = AddEntryScreen::new();
        screen.secret_type = SecretType::Password;

        screen.generate_password();

        assert!(!screen.secret.is_empty());
        assert_eq!(screen.secret, screen.secret_confirm);
        assert!(screen.generated_password);
    }

    #[test]
    fn editing_generated_password_clears_generated_state() {
        let mut screen = AddEntryScreen::new();
        screen.secret_type = SecretType::Password;
        screen.generate_password();
        screen.current_field = screen.secret_field();

        screen.insert_char('x');

        assert!(!screen.generated_password);
        assert_ne!(screen.secret, screen.secret_confirm);
    }

    #[test]
    fn text_can_be_edited_mid_field() {
        let mut screen = AddEntryScreen::new();
        screen.name = "abcd".to_string();
        screen.name_cursor = 2;

        screen.insert_char('X');
        assert_eq!(screen.name, "abXcd");
        assert_eq!(screen.name_cursor, 3);

        screen.backspace_char();
        assert_eq!(screen.name, "abcd");
        assert_eq!(screen.name_cursor, 2);

        screen.delete_char();
        assert_eq!(screen.name, "abd");
        assert_eq!(screen.name_cursor, 2);
    }

    #[test]
    fn alt_word_navigation_and_delete_work() {
        let mut screen = AddEntryScreen::new();
        screen.name = "alpha beta gamma".to_string();
        screen.name_cursor = screen.name.chars().count();

        screen.handle_key(KeyCode::Char('b'), KeyModifiers::ALT);
        assert_eq!(screen.name_cursor, 11);

        screen.handle_key(KeyCode::Char('b'), KeyModifiers::ALT);
        assert_eq!(screen.name_cursor, 6);

        screen.handle_key(KeyCode::Backspace, KeyModifiers::ALT);
        assert_eq!(screen.name, "beta gamma");
        assert_eq!(screen.name_cursor, 0);

        screen.handle_key(KeyCode::Char('f'), KeyModifiers::ALT);
        assert_eq!(screen.name_cursor, 4);
    }
}
