use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Frame;
use zeroize::Zeroizing;

use crate::ui::widgets::password_field::{PasswordAction, PasswordField};

pub struct LoginScreen {
    password_field: PasswordField,
}

impl LoginScreen {
    pub fn new() -> Self {
        Self {
            password_field: PasswordField::new("Enter your master password to unlock the vault:"),
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Option<Zeroizing<String>> {
        match self.password_field.handle_key(key, modifiers) {
            PasswordAction::Submit(password) => Some(Zeroizing::new(password)),
            PasswordAction::Cancel => None,
            PasswordAction::Continue => None,
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        self.password_field.render(frame, frame.area());
    }
}
