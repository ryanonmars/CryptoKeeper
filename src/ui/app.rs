use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::Frame;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

use crate::config::model::Config;
use crate::error::{CryptoKeeperError, Result};
use crate::ui::terminal::Tui;
use crate::vault::model::{Entry, VaultData};
use crate::vault::storage;

use super::screens::{
    add_entry::AddEntryScreen, confirm::ConfirmScreen, edit_entry::EditEntryScreen,
    input::InputScreen, login::LoginScreen, recovery::RecoveryScreen,
    recovery_setup::RecoverySetupScreen, settings::SettingsScreen,
    view_entry::ViewEntryScreen, view_password::ViewPasswordScreen,
    wizard::{WizardScreen, WizardAction},
};
use super::widgets::dashboard::Dashboard;

pub struct Session {
    pub vault: VaultData,
    password: Zeroizing<String>,
    key: Zeroizing<[u8; 32]>,
    salt: [u8; 32],
}

impl Session {
    pub fn save(&self) -> Result<()> {
        storage::save_vault_with_key(&self.vault, &*self.key, &self.salt)
    }
}

pub struct App {
    config: Config,
    session: Option<Session>,
    view: AppView,
    should_quit: bool,
    clipboard_clear_time: Option<Instant>,
    pending_export_password: Option<String>,
    pending_new_password: Option<String>,
    /// Entry index pending secondary password verification for view
    pending_view_entry_idx: Option<usize>,
    /// Entry index pending secondary password verification for copy
    pending_copy_entry_idx: Option<usize>,
}

pub enum AppView {
    Wizard(WizardScreen),
    Login(LoginScreen),
    Dashboard(Dashboard),
    AddEntry(AddEntryScreen),
    ViewEntry(ViewEntryScreen),
    EditEntry(EditEntryScreen),
    Confirm(ConfirmScreen),
    Settings(SettingsScreen),
    ViewPassword(ViewPasswordScreen),
    Recovery(RecoveryScreen),
    RecoverySetup(RecoverySetupScreen),
    Message { title: String, message: String, is_error: bool },
    Help,
    CopyCountdown { entry_name: String, seconds_left: u8 },
    Search(String),
    Input(InputScreen, InputPurpose),
}

#[derive(Clone)]
pub enum InputPurpose {
    ExportPath,
    ExportPassword,
    ImportPath,
    ImportPassword,
    ChangePassword,
    ConfirmPassword,
}

impl App {
    pub fn new() -> Result<Self> {
        let config = crate::config::load_config()?;

        let view = if !config.first_run_complete && !storage::vault_exists() {
            AppView::Wizard(WizardScreen::new())
        } else if !storage::vault_exists() {
            return Err(CryptoKeeperError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No vault found. Run `cryptokeeper init` to create one.",
            )));
        } else {
            AppView::Login(LoginScreen::new())
        };

        Ok(Self {
            config,
            session: None,
            view,
            should_quit: false,
            clipboard_clear_time: None,
            pending_export_password: None,
            pending_new_password: None,
            pending_view_entry_idx: None,
            pending_copy_entry_idx: None,
        })
    }

    pub fn run(mut self, terminal: &mut Tui) -> Result<()> {
        loop {
            terminal.draw(|frame| self.render(frame))?;

            if self.should_quit {
                break;
            }

            if let Some(clear_time) = self.clipboard_clear_time {
                if Instant::now() >= clear_time {
                    self.clear_clipboard()?;
                    self.clipboard_clear_time = None;
                    self.view = AppView::Dashboard(Dashboard::new(
                        self.session.as_ref().unwrap().vault.metadata(),
                    ));
                }
            }

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key.code, key.modifiers)?;
                }
            } else if let AppView::CopyCountdown { entry_name, seconds_left } = &self.view {
                if let Some(clear_time) = self.clipboard_clear_time {
                    let remaining = clear_time.saturating_duration_since(Instant::now());
                    let new_seconds = remaining.as_secs() as u8;
                    if new_seconds != *seconds_left {
                        self.view = AppView::CopyCountdown {
                            entry_name: entry_name.clone(),
                            seconds_left: new_seconds,
                        };
                    }
                }
            }
        }

        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        match &mut self.view {
            AppView::Wizard(wizard) => wizard.render(frame),
            AppView::Login(login) => login.render(frame),
            AppView::Dashboard(dashboard) => dashboard.render(frame),
            AppView::AddEntry(add_entry) => add_entry.render(frame),
            AppView::ViewEntry(view_entry) => view_entry.render(frame),
            AppView::EditEntry(edit_entry) => edit_entry.render(frame),
            AppView::Confirm(confirm) => confirm.render(frame),
            AppView::Settings(settings) => settings.render(frame),
            AppView::ViewPassword(vp) => vp.render(frame),
            AppView::Recovery(recovery) => recovery.render(frame),
            AppView::RecoverySetup(setup) => setup.render(frame),
            AppView::Message { title, message, is_error } => {
                let title = title.clone();
                let message = message.clone();
                let is_error = *is_error;
                Self::render_message_static(frame, &title, &message, is_error);
            }
            AppView::Help => {
                Self::render_help_static(frame);
            }
            AppView::CopyCountdown { entry_name, seconds_left } => {
                let entry_name = entry_name.clone();
                let seconds_left = *seconds_left;
                Self::render_copy_countdown_static(frame, &entry_name, seconds_left);
            }
            AppView::Search(query) => {
                let query = query.clone();
                Self::render_search_static(frame, &query);
            }
            AppView::Input(input, _) => {
                input.render(frame);
            }
        }
    }

    fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        if matches!(key, KeyCode::Char('c' | 'q')) && modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Ok(());
        }

        match &mut self.view {
            AppView::Wizard(_) => {
                self.handle_wizard_input(key, modifiers)?;
            }
            AppView::Login(login) => {
                // F1 for recovery
                if key == KeyCode::F(1) {
                    self.start_recovery()?;
                    return Ok(());
                }
                if let Some(password) = login.handle_key(key, modifiers) {
                    let password = password.clone();
                    self.unlock_vault(password)?;
                }
            }
            AppView::Dashboard(_) => {
                self.handle_dashboard_input(key, modifiers)?;
            }
            AppView::AddEntry(_) => {
                self.handle_add_entry_input(key, modifiers)?;
            }
            AppView::ViewEntry(_) => {
                self.handle_view_entry_input(key, modifiers)?;
            }
            AppView::EditEntry(_) => {
                self.handle_edit_entry_input(key, modifiers)?;
            }
            AppView::Confirm(_) => {
                self.handle_confirm_input(key, modifiers)?;
            }
            AppView::Settings(_) => {
                self.handle_settings_input(key, modifiers)?;
            }
            AppView::ViewPassword(_) => {
                self.handle_view_password_input(key, modifiers)?;
            }
            AppView::Recovery(_) => {
                self.handle_recovery_input(key, modifiers)?;
            }
            AppView::RecoverySetup(_) => {
                self.handle_recovery_setup_input(key, modifiers)?;
            }
            AppView::Message { .. } => {
                if matches!(key, KeyCode::Enter | KeyCode::Esc) {
                    if self.session.is_none() {
                        self.view = AppView::Login(LoginScreen::new());
                    } else {
                        self.return_to_dashboard();
                    }
                }
            }
            AppView::Help => {
                if matches!(key, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                    self.return_to_dashboard();
                }
            }
            AppView::CopyCountdown { .. } => {
                if key == KeyCode::Esc {
                    self.clear_clipboard()?;
                    self.clipboard_clear_time = None;
                    self.return_to_dashboard();
                }
            }
            AppView::Search(ref mut query) => {
                match key {
                    KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                        query.push(c);
                    }
                    KeyCode::Backspace => {
                        query.pop();
                    }
                    KeyCode::Enter => {
                        if let Some(session) = &self.session {
                            let mut dashboard = Dashboard::new(session.vault.metadata());
                            if let AppView::Search(q) = &self.view {
                                dashboard.set_filter(q.clone());
                            }
                            self.view = AppView::Dashboard(dashboard);
                        }
                    }
                    KeyCode::Esc => {
                        self.return_to_dashboard();
                    }
                    _ => {}
                }
            }
            AppView::Input(_, _) => {
                let (result, purpose) = match &mut self.view {
                    AppView::Input(input, purpose) => {
                        (input.handle_key(key, modifiers), purpose.clone())
                    }
                    _ => return Ok(()),
                };
                if let Some(result) = result {
                    self.handle_input_result(result, purpose)?;
                }
            }
        }

        Ok(())
    }

    // ─── Wizard ──────────────────────────────────────────────────────

    fn handle_wizard_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let action = match &mut self.view {
            AppView::Wizard(wizard) => wizard.handle_key(key, modifiers),
            _ => return Ok(()),
        };

        match action {
            WizardAction::Complete(result) => {
                // Create vault directory
                storage::ensure_vault_dir()?;

                // Create the vault
                let vault = VaultData::new();
                let password = Zeroizing::new(result.password);
                storage::save_vault(&vault, password.as_bytes())?;

                // Set up recovery if chosen
                if let Some((question_index, answer)) = &result.recovery {
                    let (vault_data, key, salt) =
                        storage::unlock_vault_returning_key(password.as_bytes())?;

                    let answer_salt = crate::crypto::kdf::generate_salt();
                    let answer_hash =
                        crate::crypto::recovery::hash_answer(answer, &answer_salt)?;
                    let (blob, nonce, blob_salt) =
                        crate::crypto::recovery::create_recovery_blob(&*key, answer)?;

                    self.config.recovery = Some(crate::config::RecoveryConfig {
                        question_index: *question_index,
                        answer_hash,
                        answer_salt: answer_salt.to_vec(),
                        master_key_blob: blob,
                        master_key_blob_nonce: nonce,
                        master_key_blob_salt: blob_salt,
                    });

                    self.session = Some(Session {
                        vault: vault_data,
                        password: password.clone(),
                        key,
                        salt,
                    });
                } else {
                    let (vault_data, key, salt) =
                        storage::unlock_vault_returning_key(password.as_bytes())?;
                    self.session = Some(Session {
                        vault: vault_data,
                        password: password.clone(),
                        key,
                        salt,
                    });
                }

                // Save config
                self.config.first_run_complete = true;
                crate::config::save_config(&self.config)?;

                self.return_to_dashboard();
            }
            WizardAction::Cancel => {
                self.should_quit = true;
            }
            WizardAction::Continue => {}
        }

        Ok(())
    }

    // ─── Recovery ────────────────────────────────────────────────────

    fn start_recovery(&mut self) -> Result<()> {
        let config = crate::config::load_config()?;
        match config.recovery {
            Some(recovery_config) => {
                self.view = AppView::Recovery(RecoveryScreen::new(recovery_config));
            }
            None => {
                self.show_message(
                    "Recovery Not Available".into(),
                    "No recovery question has been configured.\nSet one up in Settings or during init.".into(),
                    true,
                );
            }
        }
        Ok(())
    }

    fn handle_recovery_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let action = match &mut self.view {
            AppView::Recovery(recovery) => recovery.handle_key(key, modifiers),
            _ => return Ok(()),
        };

        match action {
            super::screens::recovery::RecoveryAction::Complete { master_key, new_password } => {
                // Verify we can decrypt the vault with the recovered key
                let vault_path = storage::vault_path();
                let data = std::fs::read(&vault_path)?;
                match storage::read_vault_with_key(&*master_key, &data) {
                    Ok(vault) => {
                        // Re-encrypt vault with the new password
                        storage::save_vault(&vault, new_password.as_bytes())?;

                        // Re-derive key and salt for the new session
                        let (vault_data, new_key, new_salt) =
                            storage::unlock_vault_returning_key(new_password.as_bytes())?;

                        // Update recovery config with the new master key
                        let mut config = crate::config::load_config()?;
                        if config.recovery.is_some() {
                            // Password changed = recovery must be reset.
                            // The recovery blob is encrypted under the old master key.
                            config.recovery = None;
                            crate::config::save_config(&config)?;
                            self.config = config;
                        }

                        self.session = Some(Session {
                            vault: vault_data,
                            password: new_password,
                            key: new_key,
                            salt: new_salt,
                        });

                        self.show_message(
                            "Recovery Successful".into(),
                            "Master password changed successfully!\n\nNote: Your recovery question has been cleared.\nPlease set up a new one in Settings (Shift+S).".into(),
                            false,
                        );
                    }
                    Err(e) => {
                        self.show_message(
                            "Recovery Error".into(),
                            format!("Failed to decrypt vault with recovered key: {}", e),
                            true,
                        );
                    }
                }
            }
            super::screens::recovery::RecoveryAction::Cancel => {
                self.view = AppView::Login(LoginScreen::new());
            }
            super::screens::recovery::RecoveryAction::Continue => {}
        }
        Ok(())
    }

    // ─── Login ───────────────────────────────────────────────────────

    fn unlock_vault(&mut self, password: Zeroizing<String>) -> Result<()> {
        match storage::unlock_vault_returning_key(password.as_bytes()) {
            Ok((vault, key, salt)) => {
                self.session = Some(Session {
                    vault,
                    password,
                    key,
                    salt,
                });
                self.return_to_dashboard();
                Ok(())
            }
            Err(e) => {
                self.view = AppView::Login(LoginScreen::new());
                self.show_message(
                    "Login Failed".to_string(),
                    format!("Failed to unlock vault: {}\n\nPress Enter to try again.\nPress F1 for password recovery.", e),
                    true,
                );
                Ok(())
            }
        }
    }

    // ─── Dashboard ───────────────────────────────────────────────────

    fn handle_dashboard_input(
        &mut self,
        key: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<()> {
        let (selected_idx, should_handle_key) = match &mut self.view {
            AppView::Dashboard(d) => (d.selected_index(), true),
            _ => return Ok(()),
        };

        // Enter works without modifier
        if modifiers.is_empty() && key == KeyCode::Enter {
            if let Some(idx) = selected_idx {
                if let Some(entry) = self.session.as_ref()
                    .and_then(|s| s.vault.entries.get(idx).cloned())
                {
                    if entry.has_secondary_password {
                        self.pending_view_entry_idx = Some(idx);
                        self.view = AppView::ViewPassword(
                            ViewPasswordScreen::new("Enter Secondary Password"),
                        );
                    } else {
                        self.view = AppView::ViewEntry(ViewEntryScreen::new(entry));
                    }
                }
            }
            return Ok(());
        }

        // ? works without modifier
        if modifiers.is_empty() && key == KeyCode::Char('?') {
            self.view = AppView::Help;
            return Ok(());
        }

        // Shift+key commands
        if modifiers.contains(KeyModifiers::SHIFT) {
            match key {
                KeyCode::Char('Q') => {
                    self.should_quit = true;
                    return Ok(());
                }
                KeyCode::Char('A') => {
                    self.view = AppView::AddEntry(AddEntryScreen::new());
                    return Ok(());
                }
                KeyCode::Char('V') => {
                    if let Some(idx) = selected_idx {
                        if let Some(entry) = self.session.as_ref()
                            .and_then(|s| s.vault.entries.get(idx).cloned())
                        {
                            if entry.has_secondary_password {
                                self.pending_view_entry_idx = Some(idx);
                                self.view = AppView::ViewPassword(
                                    ViewPasswordScreen::new("Enter Secondary Password"),
                                );
                            } else {
                                self.view = AppView::ViewEntry(ViewEntryScreen::new(entry));
                            }
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('C') => {
                    if let Some(idx) = selected_idx {
                        if let Some(entry) = self.session.as_ref()
                            .and_then(|s| s.vault.entries.get(idx).cloned())
                        {
                            if entry.has_secondary_password {
                                self.pending_copy_entry_idx = Some(idx);
                                self.view = AppView::ViewPassword(
                                    ViewPasswordScreen::new("Enter Secondary Password to Copy"),
                                );
                            } else {
                                self.copy_to_clipboard(&entry)?;
                            }
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('E') => {
                    if let Some(idx) = selected_idx {
                        if let Some(entry) = self.session.as_ref()
                            .and_then(|s| s.vault.entries.get(idx).cloned())
                        {
                            self.view = AppView::EditEntry(EditEntryScreen::new(entry));
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('D') => {
                    if let Some(idx) = selected_idx {
                        if let Some(entry) = self.session.as_ref()
                            .and_then(|s| s.vault.entries.get(idx))
                        {
                            self.view = AppView::Confirm(ConfirmScreen::new(
                                "Delete Entry",
                                &format!("Are you sure you want to delete '{}'?", entry.name),
                                ConfirmAction::Delete(entry.name.clone()),
                            ));
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('F') => {
                    self.view = AppView::Search(String::new());
                    return Ok(());
                }
                KeyCode::Char('S') => {
                    self.config = crate::config::load_config()?;
                    self.view = AppView::Settings(SettingsScreen::new(self.config.clone()));
                    return Ok(());
                }
                KeyCode::Char('X') => {
                    let input = InputScreen::new("Export Vault", "Enter directory path:", false);
                    self.view = AppView::Input(input, InputPurpose::ExportPath);
                    return Ok(());
                }
                KeyCode::Char('I') => {
                    let input = InputScreen::new("Import Vault", "Enter backup file path:", false);
                    self.view = AppView::Input(input, InputPurpose::ImportPath);
                    return Ok(());
                }
                KeyCode::Char('P') => {
                    let input = InputScreen::new("Change Password", "Enter new master password:", true);
                    self.view = AppView::Input(input, InputPurpose::ChangePassword);
                    return Ok(());
                }
                _ => {}
            }
        }

        if should_handle_key {
            if let AppView::Dashboard(dashboard) = &mut self.view {
                dashboard.handle_key(key, modifiers);
            }
        }
        Ok(())
    }

    fn get_selected_entry_copy(&self, dashboard: &Dashboard) -> Option<Entry> {
        let session = self.session.as_ref()?;
        let selected_idx = dashboard.selected_index()?;
        session.vault.entries.get(selected_idx).cloned()
    }

    // ─── Settings ────────────────────────────────────────────────────

    fn handle_settings_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let action = match &mut self.view {
            AppView::Settings(settings) => settings.handle_key(key, modifiers),
            _ => return Ok(()),
        };

        match action {
            super::screens::settings::SettingsAction::Save(updated_config) => {
                self.config = updated_config;
                crate::config::save_config(&self.config)?;
                self.return_to_dashboard();
            }
            super::screens::settings::SettingsAction::Cancel => {
                self.return_to_dashboard();
            }
            super::screens::settings::SettingsAction::SetupRecovery => {
                self.view = AppView::RecoverySetup(RecoverySetupScreen::new());
            }
            super::screens::settings::SettingsAction::Continue => {}
        }
        Ok(())
    }

    // ─── Recovery Setup (from Settings) ───────────────────────────────

    fn handle_recovery_setup_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let action = match &mut self.view {
            AppView::RecoverySetup(setup) => setup.handle_key(key, modifiers),
            _ => return Ok(()),
        };

        match action {
            super::screens::recovery_setup::RecoverySetupAction::Complete {
                question_index,
                answer,
            } => {
                if let Some(session) = &self.session {
                    let master_key: &[u8; 32] = &*session.key;

                    let answer_salt = crate::crypto::kdf::generate_salt();
                    let answer_hash =
                        crate::crypto::recovery::hash_answer(&answer, &answer_salt)?;
                    let (blob, nonce, blob_salt) =
                        crate::crypto::recovery::create_recovery_blob(master_key, &answer)?;

                    self.config.recovery = Some(crate::config::RecoveryConfig {
                        question_index,
                        answer_hash,
                        answer_salt: answer_salt.to_vec(),
                        master_key_blob: blob,
                        master_key_blob_nonce: nonce,
                        master_key_blob_salt: blob_salt,
                    });
                    crate::config::save_config(&self.config)?;

                    self.show_success("Recovery question configured successfully!".to_string());
                }
            }
            super::screens::recovery_setup::RecoverySetupAction::Cancel => {
                // Return to settings
                self.config = crate::config::load_config()?;
                self.view = AppView::Settings(SettingsScreen::new(self.config.clone()));
            }
            super::screens::recovery_setup::RecoverySetupAction::Continue => {}
        }
        Ok(())
    }

    // ─── View Password (secondary password gate) ─────────────────────

    fn handle_view_password_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let action = match &mut self.view {
            AppView::ViewPassword(vp) => vp.handle_key(key, modifiers),
            _ => return Ok(()),
        };

        match action {
            super::screens::view_password::ViewPasswordAction::Submit(view_pass) => {
                // Try to unlock the entry's secret
                if let Some(idx) = self.pending_view_entry_idx.take() {
                    if let Some(entry) = self.session.as_ref()
                        .and_then(|s| s.vault.entries.get(idx).cloned())
                    {
                        match self.decrypt_entry_secret(&entry, &view_pass) {
                            Ok(decrypted_secret) => {
                                let mut revealed_entry = entry.clone();
                                revealed_entry.secret = (*decrypted_secret).clone();
                                self.view = AppView::ViewEntry(ViewEntryScreen::new(revealed_entry));
                            }
                            Err(_) => {
                                let mut vp = ViewPasswordScreen::new("Enter Secondary Password");
                                vp.set_error("Incorrect password. Try again.");
                                self.pending_view_entry_idx = Some(idx);
                                self.view = AppView::ViewPassword(vp);
                            }
                        }
                    } else {
                        self.return_to_dashboard();
                    }
                } else if let Some(idx) = self.pending_copy_entry_idx.take() {
                    if let Some(entry) = self.session.as_ref()
                        .and_then(|s| s.vault.entries.get(idx).cloned())
                    {
                        match self.decrypt_entry_secret(&entry, &view_pass) {
                            Ok(decrypted_secret) => {
                                let mut copy_entry = entry.clone();
                                copy_entry.secret = (*decrypted_secret).clone();
                                self.copy_to_clipboard(&copy_entry)?;
                            }
                            Err(_) => {
                                let mut vp = ViewPasswordScreen::new("Enter Secondary Password to Copy");
                                vp.set_error("Incorrect password. Try again.");
                                self.pending_copy_entry_idx = Some(idx);
                                self.view = AppView::ViewPassword(vp);
                            }
                        }
                    } else {
                        self.return_to_dashboard();
                    }
                } else {
                    self.return_to_dashboard();
                }
            }
            super::screens::view_password::ViewPasswordAction::Cancel => {
                self.pending_view_entry_idx = None;
                self.pending_copy_entry_idx = None;
                self.return_to_dashboard();
            }
            super::screens::view_password::ViewPasswordAction::Continue => {}
        }
        Ok(())
    }

    fn decrypt_entry_secret(&self, entry: &Entry, view_password: &str) -> Result<Zeroizing<String>> {
        let wrapped = entry.entry_key_wrapped.as_ref()
            .ok_or(CryptoKeeperError::SecondaryPasswordRequired)?;
        let nonce = entry.entry_key_nonce.as_ref()
            .ok_or(CryptoKeeperError::SecondaryPasswordRequired)?;
        let salt = entry.entry_key_salt.as_ref()
            .ok_or(CryptoKeeperError::SecondaryPasswordRequired)?;
        let ct = entry.encrypted_secret.as_ref()
            .ok_or(CryptoKeeperError::SecondaryPasswordRequired)?;
        let ct_nonce = entry.encrypted_secret_nonce.as_ref()
            .ok_or(CryptoKeeperError::SecondaryPasswordRequired)?;

        let entry_key = crate::crypto::entry_key::unwrap_entry_key(wrapped, nonce, salt, view_password)?;
        crate::crypto::entry_key::decrypt_secret(&entry_key, ct, ct_nonce)
    }

    // ─── Add Entry ───────────────────────────────────────────────────

    fn handle_add_entry_input(
        &mut self,
        key: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<()> {
        let action = match &mut self.view {
            AppView::AddEntry(add_entry) => add_entry.handle_key(key, modifiers),
            _ => return Ok(()),
        };

        match action {
            super::screens::add_entry::AddEntryAction::Save(entry) => {
                if let Some(session) = &mut self.session {
                    let msg = match &entry.public_address {
                        Some(addr) => format!("Entry added! Address: {}", addr),
                        None => "Entry added successfully!".to_string(),
                    };
                    session.vault.entries.push(entry);
                    session.save()?;
                    self.show_success(msg);
                }
            }
            super::screens::add_entry::AddEntryAction::Cancel => {
                self.return_to_dashboard();
            }
            super::screens::add_entry::AddEntryAction::Continue => {}
        }
        Ok(())
    }

    // ─── View Entry ──────────────────────────────────────────────────

    fn handle_view_entry_input(
        &mut self,
        key: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<()> {
        let action = match &mut self.view {
            AppView::ViewEntry(view_entry) => view_entry.handle_key(key, modifiers),
            _ => return Ok(()),
        };

        match action {
            super::screens::view_entry::ViewEntryAction::Close => {
                self.return_to_dashboard();
            }
            super::screens::view_entry::ViewEntryAction::Copy(secret) => {
                use arboard::Clipboard;
                let timeout = self.config.clipboard_timeout_secs;
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(&secret);
                    self.clipboard_clear_time = Some(Instant::now() + Duration::from_secs(timeout));

                    let entry_name = match &self.view {
                        AppView::ViewEntry(v) => v.entry.name.clone(),
                        _ => String::new(),
                    };

                    self.view = AppView::CopyCountdown {
                        entry_name,
                        seconds_left: timeout as u8,
                    };
                }
            }
            super::screens::view_entry::ViewEntryAction::Continue => {}
        }
        Ok(())
    }

    // ─── Edit Entry ──────────────────────────────────────────────────

    fn handle_edit_entry_input(
        &mut self,
        key: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<()> {
        let (action, original_name) = match &mut self.view {
            AppView::EditEntry(edit_entry) => {
                let original = edit_entry.original_name.clone();
                (edit_entry.handle_key(key, modifiers), original)
            },
            _ => return Ok(()),
        };

        match action {
            super::screens::edit_entry::EditEntryAction::Save(updated_entry) => {
                if let Some(session) = &mut self.session {
                    if let Some(entry) = session
                        .vault
                        .entries
                        .iter_mut()
                        .find(|e| e.name == original_name)
                    {
                        *entry = updated_entry;
                    }
                    session.save()?;
                    self.show_success("Entry updated successfully!".to_string());
                }
            }
            super::screens::edit_entry::EditEntryAction::Cancel => {
                self.return_to_dashboard();
            }
            super::screens::edit_entry::EditEntryAction::Continue => {}
        }
        Ok(())
    }

    // ─── Confirm ─────────────────────────────────────────────────────

    fn handle_confirm_input(
        &mut self,
        key: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<()> {
        let (result, action) = match &mut self.view {
            AppView::Confirm(confirm) => {
                (confirm.handle_key(key, modifiers), confirm.action.clone())
            },
            _ => return Ok(()),
        };

        match result {
            Some(true) => {
                match action {
                    ConfirmAction::Delete(entry_name) => {
                        if let Some(session) = &mut self.session {
                            session.vault.remove_entry(&entry_name);
                            session.save()?;
                            self.show_success("Entry deleted successfully!".to_string());
                        }
                    }
                }
            }
            Some(false) => {
                self.return_to_dashboard();
            }
            None => {}
        }
        Ok(())
    }

    // ─── Clipboard ───────────────────────────────────────────────────

    fn copy_to_clipboard(&mut self, entry: &Entry) -> Result<()> {
        use arboard::Clipboard;
        let timeout = self.config.clipboard_timeout_secs;
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(&entry.secret);
            self.clipboard_clear_time = Some(Instant::now() + Duration::from_secs(timeout));
            self.view = AppView::CopyCountdown {
                entry_name: entry.name.clone(),
                seconds_left: timeout as u8,
            };
        }
        Ok(())
    }

    fn clear_clipboard(&self) -> Result<()> {
        use arboard::Clipboard;
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text("");
        }
        Ok(())
    }

    // ─── Navigation ──────────────────────────────────────────────────

    fn return_to_dashboard(&mut self) {
        if let Some(session) = &self.session {
            self.view = AppView::Dashboard(Dashboard::new(session.vault.metadata()));
        }
    }

    fn show_success(&mut self, message: String) {
        self.view = AppView::Message {
            title: "Success".to_string(),
            message,
            is_error: false,
        };
    }

    fn show_message(&mut self, title: String, message: String, is_error: bool) {
        self.view = AppView::Message {
            title,
            message,
            is_error,
        };
    }

    // ─── Static Renderers ────────────────────────────────────────────

    fn render_message_static(frame: &mut Frame, title: &str, message: &str, is_error: bool) {
        use ratatui::{
            layout::{Constraint, Direction, Layout},
            style::{Color, Modifier, Style},
            widgets::{Block, Borders, Paragraph, Wrap},
        };

        let area = frame.area();
        let color = if is_error { Color::Red } else { Color::Green };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(7), Constraint::Min(1)])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", title))
            .title_style(Style::default().fg(color).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(color));

        let paragraph = Paragraph::new(format!("{}\n\nPress Enter or Esc to continue", message))
            .block(block)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::White));

        frame.render_widget(paragraph, chunks[1]);
    }

    fn render_help_static(frame: &mut Frame) {
        use ratatui::{
            layout::{Constraint, Direction, Layout},
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Paragraph, Wrap},
        };

        let area = frame.area();

        let help_text = vec![
            Line::from(vec![Span::styled(
                "Navigation & Entry Selection:",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )]),
            Line::from("  ↑/↓       Navigate entry list"),
            Line::from("  1-9       Quick jump to entry 1-9"),
            Line::from("  Type #    Type number + Enter (e.g. 15 + Enter)"),
            Line::from("  Enter     View selected entry"),
            Line::from("  /         Start filtering entries"),
            Line::from("  Esc       Clear filter or number entry"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Commands:",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )]),
            Line::from("  Shift+A   Add new entry"),
            Line::from("  Shift+V   View selected entry"),
            Line::from("  Shift+C   Copy secret to clipboard"),
            Line::from("  Shift+E   Edit selected entry"),
            Line::from("  Shift+D   Delete selected entry"),
            Line::from("  Shift+F   Find/filter entries"),
            Line::from("  Shift+X   Export vault"),
            Line::from("  Shift+I   Import vault"),
            Line::from("  Shift+P   Change password"),
            Line::from("  Shift+S   Settings"),
            Line::from("  ?         Show this help"),
            Line::from("  Shift+Q   Quit application"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Global Shortcuts:",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )]),
            Line::from("  Ctrl+C    Quit from anywhere"),
            Line::from("  Ctrl+Q    Quit from anywhere"),
            Line::from("  F1        Password recovery (login screen)"),
            Line::from("  Esc       Go back/cancel"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Press Esc or ? to close",
                Style::default().fg(Color::Yellow),
            )]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Keyboard Shortcuts ")
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(help_text)
            .block(block)
            .wrap(Wrap { trim: false });

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(28),
                Constraint::Min(1),
            ])
            .split(area);

        frame.render_widget(paragraph, chunks[1]);
    }

    fn render_copy_countdown_static(frame: &mut Frame, entry_name: &str, seconds_left: u8) {
        use ratatui::{
            layout::{Constraint, Direction, Layout},
            style::{Color, Modifier, Style},
            widgets::{Block, Borders, Paragraph, Wrap},
        };

        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(5), Constraint::Min(1)])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Copied to Clipboard ")
            .title_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Green));

        let message = format!(
            "Secret for '{}' copied to clipboard!\n\nClearing in {} second{}...\n\nPress Esc to clear now",
            entry_name,
            seconds_left,
            if seconds_left == 1 { "" } else { "s" }
        );

        let paragraph = Paragraph::new(message)
            .block(block)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::White));

        frame.render_widget(paragraph, chunks[1]);
    }

    fn render_search_static(frame: &mut Frame, query: &str) {
        use ratatui::{
            layout::{Constraint, Direction, Layout},
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Paragraph},
        };

        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(5), Constraint::Min(1)])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Find Entries ")
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(Color::Cyan));

        let text = vec![
            Line::from("Type to find entries by name or network:"),
            Line::from(""),
            Line::from(vec![
                Span::styled("Find: ", Style::default().fg(Color::Cyan)),
                Span::styled(query, Style::default().fg(Color::Yellow)),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Press Enter to apply filter │ Esc to cancel",
                Style::default().fg(Color::DarkGray),
            )]),
        ];

        let paragraph = Paragraph::new(text).block(block);

        frame.render_widget(paragraph, chunks[1]);
    }

    // ─── Input Result Handler ────────────────────────────────────────

    fn handle_input_result(&mut self, result: super::screens::input::InputResult, purpose: InputPurpose) -> Result<()> {
        use super::screens::input::InputResult;
        use zeroize::Zeroizing;

        match result {
            InputResult::Cancel => {
                self.pending_export_password = None;
                self.pending_new_password = None;
                self.return_to_dashboard();
            }
            InputResult::Submit(value) => {
                match purpose {
                    InputPurpose::ExportPath => {
                        let input = InputScreen::new("Export Vault", "Enter backup password:", true);
                        self.pending_export_password = Some(value);
                        self.view = AppView::Input(input, InputPurpose::ExportPassword);
                    }
                    InputPurpose::ExportPassword => {
                        if let Some(path) = self.pending_export_password.take() {
                            if let Some(session) = &self.session {
                                let password = Zeroizing::new(value);
                                let backup_path = std::path::Path::new(&path).join("backup.ck");
                                match crate::vault::storage::write_backup(&session.vault, password.as_bytes(), &backup_path) {
                                    Ok(_) => {
                                        self.show_success(format!("Vault exported to {}/backup.ck", path));
                                    }
                                    Err(e) => {
                                        self.show_message("Export Error".to_string(), format!("Failed to export: {}", e), true);
                                    }
                                }
                            }
                        }
                    }
                    InputPurpose::ImportPath => {
                        let input = InputScreen::new("Import Vault", "Enter backup password:", true);
                        self.pending_export_password = Some(value);
                        self.view = AppView::Input(input, InputPurpose::ImportPassword);
                    }
                    InputPurpose::ImportPassword => {
                        if let Some(path) = self.pending_export_password.take() {
                            if let Some(session) = &mut self.session {
                                let password = Zeroizing::new(value);
                                match crate::vault::storage::read_backup(password.as_bytes(), std::path::Path::new(&path)) {
                                    Ok(backup) => {
                                        let mut imported = 0;
                                        for entry in backup.entries {
                                            if !session.vault.has_entry(&entry.name) {
                                                session.vault.entries.push(entry);
                                                imported += 1;
                                            }
                                        }
                                        if imported > 0 {
                                            let _ = session.save();
                                        }
                                        self.show_success(format!("Imported {} entries from backup", imported));
                                    }
                                    Err(e) => {
                                        self.show_message("Import Error".to_string(), format!("Failed to import: {}", e), true);
                                    }
                                }
                            }
                        }
                    }
                    InputPurpose::ChangePassword => {
                        let input = InputScreen::new("Change Password", "Confirm new password:", true);
                        self.pending_new_password = Some(value);
                        self.view = AppView::Input(input, InputPurpose::ConfirmPassword);
                    }
                    InputPurpose::ConfirmPassword => {
                        if let Some(new_pass) = self.pending_new_password.take() {
                            if new_pass == value {
                                if let Some(session) = &mut self.session {
                                    let password = Zeroizing::new(new_pass);
                                    match crate::vault::storage::save_vault(&session.vault, password.as_bytes()) {
                                        Ok(_) => {
                                            // Warn about recovery invalidation
                                            let has_recovery = self.config.recovery.is_some();
                                            session.password = password.clone();
                                            if has_recovery {
                                                self.config.recovery = None;
                                                let _ = crate::config::save_config(&self.config);
                                                self.show_message(
                                                    "Password Changed".into(),
                                                    "Master password changed successfully!\n\nNote: Your recovery question has been cleared.\nPlease set up a new one in Settings (Shift+S).".into(),
                                                    false,
                                                );
                                            } else {
                                                self.show_success("Master password changed successfully!".to_string());
                                            }
                                        }
                                        Err(e) => {
                                            self.show_message("Password Change Error".to_string(), format!("Failed to change password: {}", e), true);
                                        }
                                    }
                                }
                            } else {
                                self.show_message("Error".to_string(), "Passwords do not match!".to_string(), true);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub enum ConfirmAction {
    Delete(String),
}
