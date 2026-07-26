use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

use crate::config::model::Config;
use crate::error::{Result, TermKeyError};
use crate::ui::terminal::Tui;
use crate::update::{self, UpdateStatus};
use crate::vault::model::Entry;
use crate::vault::session::VaultSession;
use crate::vault::storage;

use super::screens::{
    add_entry::AddEntryScreen,
    confirm::ConfirmScreen,
    edit_entry::EditEntryScreen,
    input::InputScreen,
    login::LoginScreen,
    nuke::NukeScreen,
    recovery::RecoveryScreen,
    recovery_setup::RecoverySetupScreen,
    settings::SettingsScreen,
    view_entry::ViewEntryScreen,
    view_password::ViewPasswordScreen,
    wizard::{WizardAction, WizardScreen},
};
use super::widgets::dashboard::Dashboard;

fn add_entry_to_vault(vault: &mut crate::vault::model::VaultData, entry: Entry) -> Result<()> {
    vault.push_entry(entry)
}

fn replace_entry_in_vault(
    vault: &mut crate::vault::model::VaultData,
    original_name: &str,
    entry: Entry,
    secondary_password: Option<&str>,
) -> Result<()> {
    vault.replace_entry_authorized(original_name, entry, secondary_password)
}

fn delete_entry_from_vault(
    vault: &mut crate::vault::model::VaultData,
    name: &str,
    secondary_password: Option<&str>,
) -> Result<()> {
    vault.remove_entry_authorized(name, secondary_password)?;
    Ok(())
}

enum SecondaryRevealError {
    Retry,
    Fatal(String),
}

fn classify_secondary_reveal_error(error: TermKeyError) -> SecondaryRevealError {
    match error {
        TermKeyError::SecondaryPasswordWrong => SecondaryRevealError::Retry,
        other => SecondaryRevealError::Fatal(other.to_string()),
    }
}

pub struct App {
    config: Config,
    session: Option<VaultSession>,
    view: AppView,
    should_quit: bool,
    clipboard_clear_time: Option<Instant>,
    pending_export_password: Option<String>,
    pending_new_password: Option<Zeroizing<String>>,
    /// Entry index pending secondary password verification for view
    pending_view_entry_idx: Option<usize>,
    /// Entry index pending secondary password verification for copy
    pending_copy_entry_idx: Option<usize>,
    /// Entry name pending secondary password verification for delete
    pending_delete_entry_name: Option<String>,
    update_check_rx: Option<Receiver<UpdateStatus>>,
    update_status: UpdateStatus,
}

pub enum AppView {
    Wizard(WizardScreen),
    Login(LoginScreen),
    Dashboard(Dashboard),
    AddEntry(AddEntryScreen),
    ViewEntry(ViewEntryScreen),
    EditEntry(Box<EditEntryScreen>),
    Confirm(ConfirmScreen),
    Settings(SettingsScreen),
    ViewPassword(ViewPasswordScreen),
    Recovery(RecoveryScreen),
    RecoverySetup(RecoverySetupScreen),
    NoRecovery,
    Nuke(NukeScreen),
    Message {
        title: String,
        message: String,
        is_error: bool,
    },
    Help,
    CopyCountdown {
        entry_name: String,
        seconds_left: u64,
    },
    Search(String),
    Input(InputScreen, InputPurpose),
}

#[derive(Clone)]
pub enum InputPurpose {
    ExportPath,
    ExportPassword,
    ConfirmExportPassword,
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
            return Err(TermKeyError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No vault found. Run `termkey init` to create one.",
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
            pending_delete_entry_name: None,
            update_check_rx: Some(update::spawn_update_check()),
            update_status: UpdateStatus::Unknown,
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
                    self.clipboard_clear_time = None;
                    self.view = AppView::Dashboard(Dashboard::new(
                        self.session.as_ref().unwrap().vault.metadata(),
                    ));
                }
            }

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Release {
                        self.handle_key(key.code, key.modifiers)?;
                    }
                }
            } else if let AppView::CopyCountdown {
                entry_name,
                seconds_left,
            } = &self.view
            {
                if let Some(clear_time) = self.clipboard_clear_time {
                    let remaining = clear_time.saturating_duration_since(Instant::now());
                    let new_seconds = remaining.as_secs();
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
        self.poll_update_status();
        let update_status = self.update_status.clone();

        match &mut self.view {
            AppView::Wizard(wizard) => wizard.render(frame),
            AppView::Login(login) => login.render(frame),
            AppView::Dashboard(dashboard) => dashboard.render(frame, &update_status),
            AppView::AddEntry(add_entry) => add_entry.render(frame),
            AppView::ViewEntry(view_entry) => view_entry.render(frame),
            AppView::EditEntry(edit_entry) => edit_entry.render(frame),
            AppView::Confirm(confirm) => confirm.render(frame),
            AppView::Settings(settings) => settings.render(frame),
            AppView::ViewPassword(vp) => vp.render(frame),
            AppView::Recovery(recovery) => recovery.render(frame),
            AppView::RecoverySetup(setup) => setup.render(frame),
            AppView::NoRecovery => {
                Self::render_no_recovery_static(frame);
            }
            AppView::Nuke(nuke) => {
                nuke.render(frame);
            }
            AppView::Message {
                title,
                message,
                is_error,
            } => {
                let title = title.clone();
                let message = message.clone();
                let is_error = *is_error;
                Self::render_message_static(frame, &title, &message, is_error);
            }
            AppView::Help => {
                Self::render_help_static(frame);
            }
            AppView::CopyCountdown {
                entry_name,
                seconds_left,
            } => {
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

    fn poll_update_status(&mut self) {
        let Some(rx) = &self.update_check_rx else {
            return;
        };

        match rx.try_recv() {
            Ok(status) => {
                self.update_status = status;
                self.update_check_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.update_check_rx = None;
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
            AppView::NoRecovery => {
                if key == KeyCode::F(2) {
                    self.view = AppView::Nuke(NukeScreen::new());
                } else if matches!(key, KeyCode::Esc | KeyCode::Enter) {
                    self.view = AppView::Login(LoginScreen::new());
                }
            }
            AppView::Nuke(_) => {
                self.handle_nuke_input(key, modifiers)?;
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
                    self.clipboard_clear_time = None;
                    self.return_to_dashboard();
                }
            }
            AppView::Search(ref mut query) => match key {
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
            },
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
                storage::ensure_vault_dir()?;
                self.session = Some(VaultSession::create(
                    crate::vault::model::VaultData::new(),
                    result.password,
                    storage::vault_path(),
                )?);

                self.config = crate::config::update_config(|config| {
                    config.first_run_complete = true;
                    config.vault_path = storage::vault_path().display().to_string();
                    Ok(config.clone())
                })?;

                if result.setup_recovery {
                    let phrase = crate::crypto::recovery::generate_recovery_phrase()?;
                    self.view = AppView::RecoverySetup(RecoverySetupScreen::new(phrase));
                } else {
                    self.return_to_dashboard();
                }
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
                self.view = AppView::NoRecovery;
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
            super::screens::recovery::RecoveryAction::UnsupportedLegacy => {
                self.show_message(
                    "Recovery unavailable".to_string(),
                    super::screens::recovery::LEGACY_RECOVERY_UNSUPPORTED.to_string(),
                    true,
                );
            }
            super::screens::recovery::RecoveryAction::Recover {
                config,
                phrase,
                new_password,
            } => {
                let result =
                    crate::crypto::recovery::recover_dek(&config, config.vault_id, &phrase)
                        .and_then(|dek| {
                            VaultSession::recover(
                                dek,
                                config.vault_id,
                                new_password,
                                storage::vault_path(),
                            )
                        });
                match result {
                    Ok(session) => {
                        self.session = Some(session);
                        self.config = crate::config::load_config()?;
                        self.show_success(
                            "Vault recovered. Your recovery phrase remains active.".to_string(),
                        );
                    }
                    Err(error) => {
                        if let AppView::Recovery(screen) = &mut self.view {
                            screen.retry_after_failure(error.to_string());
                        }
                    }
                }
            }
            super::screens::recovery::RecoveryAction::Cancel => {
                self.view = AppView::Login(LoginScreen::new());
            }
            super::screens::recovery::RecoveryAction::DeleteVault => {
                self.view = AppView::Nuke(NukeScreen::new());
            }
            super::screens::recovery::RecoveryAction::Continue => {}
        }
        Ok(())
    }

    fn handle_nuke_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let action = match &mut self.view {
            AppView::Nuke(nuke) => nuke.handle_key(key, modifiers),
            _ => return Ok(()),
        };

        match action {
            super::screens::nuke::NukeAction::Cancel => {
                self.view = AppView::Login(LoginScreen::new());
            }
            super::screens::nuke::NukeAction::Confirm => {
                if let Err(e) = storage::delete_vault() {
                    self.view = AppView::Login(LoginScreen::new());
                    self.show_message(
                        "Delete Failed".to_string(),
                        format!(
                            "Failed to delete vault: {}\n\nYour vault has not been modified.",
                            e
                        ),
                        true,
                    );
                    return Ok(());
                }
                let _ = crate::config::delete_config(); // best-effort; vault is already gone
                self.config = crate::config::model::Config::default();
                self.session = None;
                self.view = AppView::Wizard(WizardScreen::new());
            }
            super::screens::nuke::NukeAction::Continue => {}
        }
        Ok(())
    }

    // ─── Login ───────────────────────────────────────────────────────

    fn unlock_vault(&mut self, password: Zeroizing<String>) -> Result<()> {
        match VaultSession::open(password, storage::vault_path()) {
            Ok(outcome) => {
                let mut recovery_notice = outcome.recovery_notice;
                self.session = Some(outcome.session);
                match crate::config::load_config() {
                    Ok(config) => self.config = config,
                    Err(error) if recovery_notice.is_none() => {
                        recovery_notice = Some(format!(
                            "Vault unlocked, but configuration could not be loaded: {error}"
                        ));
                    }
                    Err(_) => {}
                }
                self.return_to_dashboard();
                if let Some(notice) = recovery_notice {
                    self.show_message("Recovery setup required".to_string(), notice, false);
                }
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

    fn handle_dashboard_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let (selected_idx, should_handle_key) = match &mut self.view {
            AppView::Dashboard(d) => (d.selected_index(), true),
            _ => return Ok(()),
        };

        // Enter works without modifier
        if modifiers.is_empty() && key == KeyCode::Enter {
            if let Some(idx) = selected_idx {
                if let Some(entry) = self
                    .session
                    .as_ref()
                    .and_then(|s| s.vault.entries.get(idx).cloned())
                {
                    if entry.has_secondary_password {
                        self.pending_view_entry_idx = Some(idx);
                        self.view = AppView::ViewPassword(ViewPasswordScreen::new(
                            "Enter Secondary Password",
                        ));
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
                        if let Some(entry) = self
                            .session
                            .as_ref()
                            .and_then(|s| s.vault.entries.get(idx).cloned())
                        {
                            if entry.has_secondary_password {
                                self.pending_view_entry_idx = Some(idx);
                                self.view = AppView::ViewPassword(ViewPasswordScreen::new(
                                    "Enter Secondary Password",
                                ));
                            } else {
                                self.view = AppView::ViewEntry(ViewEntryScreen::new(entry));
                            }
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('C') => {
                    if let Some(idx) = selected_idx {
                        if let Some(entry) = self
                            .session
                            .as_ref()
                            .and_then(|s| s.vault.entries.get(idx).cloned())
                        {
                            if entry.has_secondary_password {
                                self.pending_copy_entry_idx = Some(idx);
                                self.view = AppView::ViewPassword(ViewPasswordScreen::new(
                                    "Enter Secondary Password to Copy",
                                ));
                            } else {
                                let secret = entry.reveal_secret(None)?;
                                self.copy_to_clipboard(&entry.name, secret)?;
                            }
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('E') => {
                    if let Some(idx) = selected_idx {
                        if let Some(entry) = self
                            .session
                            .as_ref()
                            .and_then(|s| s.vault.entries.get(idx).cloned())
                        {
                            self.view = AppView::EditEntry(Box::new(EditEntryScreen::new(entry)));
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('D') => {
                    if let Some(idx) = selected_idx {
                        if let Some(entry) =
                            self.session.as_ref().and_then(|s| s.vault.entries.get(idx))
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
                    let input =
                        InputScreen::new("Change Password", "Enter new master password:", true);
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

    // ─── Settings ────────────────────────────────────────────────────

    fn handle_settings_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let action = match &mut self.view {
            AppView::Settings(settings) => settings.handle_key(key, modifiers),
            _ => return Ok(()),
        };

        match action {
            super::screens::settings::SettingsAction::SaveClipboardTimeout(timeout) => {
                self.config = crate::config::update_config(|config| {
                    config.clipboard_timeout_secs = timeout;
                    Ok(config.clone())
                })?;
                self.return_to_dashboard();
            }
            super::screens::settings::SettingsAction::Cancel => {
                self.return_to_dashboard();
            }
            super::screens::settings::SettingsAction::SetupRecovery => {
                let phrase = crate::crypto::recovery::generate_recovery_phrase()?;
                self.view = AppView::RecoverySetup(RecoverySetupScreen::new(phrase));
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
            super::screens::recovery_setup::RecoverySetupAction::Confirmed(phrase) => {
                let session = self.session.as_ref().ok_or(TermKeyError::VaultNotFound)?;
                let recovery = crate::crypto::recovery::create_recovery_config(
                    session.vault_id(),
                    session.dek(),
                    &phrase,
                )?;
                self.config = crate::config::update_config(|config| {
                    config.recovery = Some(crate::config::RecoveryConfig::V2(recovery));
                    Ok(config.clone())
                })?;
                self.show_success("Recovery phrase configured successfully.".to_string());
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
                    if let Some(entry) = self
                        .session
                        .as_ref()
                        .and_then(|s| s.vault.entries.get(idx).cloned())
                    {
                        match entry.reveal_secret(Some(&view_pass)) {
                            Ok(decrypted_secret) => {
                                self.view = AppView::ViewEntry(ViewEntryScreen::new_with_secret(
                                    entry,
                                    decrypted_secret,
                                ));
                            }
                            Err(error) => match classify_secondary_reveal_error(error) {
                                SecondaryRevealError::Retry => {
                                    let mut vp =
                                        ViewPasswordScreen::new("Enter Secondary Password");
                                    vp.set_error("Incorrect password. Try again.");
                                    self.pending_view_entry_idx = Some(idx);
                                    self.view = AppView::ViewPassword(vp);
                                }
                                SecondaryRevealError::Fatal(message) => {
                                    self.show_message(
                                        "Entry Error".to_string(),
                                        format!("Unable to reveal secret: {message}"),
                                        true,
                                    );
                                }
                            },
                        }
                    } else {
                        self.return_to_dashboard();
                    }
                } else if let Some(idx) = self.pending_copy_entry_idx.take() {
                    if let Some(entry) = self
                        .session
                        .as_ref()
                        .and_then(|s| s.vault.entries.get(idx).cloned())
                    {
                        match entry.reveal_secret(Some(&view_pass)) {
                            Ok(decrypted_secret) => {
                                self.copy_to_clipboard(&entry.name, decrypted_secret)?;
                            }
                            Err(error) => match classify_secondary_reveal_error(error) {
                                SecondaryRevealError::Retry => {
                                    let mut vp =
                                        ViewPasswordScreen::new("Enter Secondary Password to Copy");
                                    vp.set_error("Incorrect password. Try again.");
                                    self.pending_copy_entry_idx = Some(idx);
                                    self.view = AppView::ViewPassword(vp);
                                }
                                SecondaryRevealError::Fatal(message) => {
                                    self.show_message(
                                        "Entry Error".to_string(),
                                        format!("Unable to copy secret: {message}"),
                                        true,
                                    );
                                }
                            },
                        }
                    } else {
                        self.return_to_dashboard();
                    }
                } else if let Some(entry_name) = self.pending_delete_entry_name.take() {
                    let delete_result = if let Some(session) = &mut self.session {
                        let snapshot = session.vault.clone();
                        match delete_entry_from_vault(
                            &mut session.vault,
                            &entry_name,
                            Some(view_pass.as_str()),
                        ) {
                            Ok(()) => {
                                if let Err(error) = session.save() {
                                    session.vault = snapshot;
                                    Err(error)
                                } else {
                                    Ok(())
                                }
                            }
                            Err(error) => Err(error),
                        }
                    } else {
                        Err(TermKeyError::VaultNotFound)
                    };

                    match delete_result {
                        Ok(()) => {
                            self.show_success("Entry deleted successfully!".to_string());
                        }
                        Err(error) => match classify_secondary_reveal_error(error) {
                            SecondaryRevealError::Retry => {
                                let mut vp =
                                    ViewPasswordScreen::new("Enter Secondary Password to Delete");
                                vp.set_error("Incorrect password. Try again.");
                                self.pending_delete_entry_name = Some(entry_name);
                                self.view = AppView::ViewPassword(vp);
                            }
                            SecondaryRevealError::Fatal(message) => {
                                self.show_message(
                                    "Delete Error".to_string(),
                                    format!("Unable to delete entry: {message}"),
                                    true,
                                );
                            }
                        },
                    }
                } else {
                    self.return_to_dashboard();
                }
            }
            super::screens::view_password::ViewPasswordAction::Cancel => {
                self.pending_view_entry_idx = None;
                self.pending_copy_entry_idx = None;
                self.pending_delete_entry_name = None;
                self.return_to_dashboard();
            }
            super::screens::view_password::ViewPasswordAction::Continue => {}
        }
        Ok(())
    }

    // ─── Add Entry ───────────────────────────────────────────────────

    fn handle_add_entry_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
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
                    let snapshot = session.vault.clone();
                    add_entry_to_vault(&mut session.vault, *entry)?;
                    if let Err(error) = session.save() {
                        session.vault = snapshot;
                        return Err(error);
                    }
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

    fn handle_view_entry_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let action = match &mut self.view {
            AppView::ViewEntry(view_entry) => view_entry.handle_key(key, modifiers),
            _ => return Ok(()),
        };

        match action {
            super::screens::view_entry::ViewEntryAction::Close => {
                self.return_to_dashboard();
            }
            super::screens::view_entry::ViewEntryAction::Copy(secret) => {
                let entry_name = match &self.view {
                    AppView::ViewEntry(v) => v.entry.name.clone(),
                    _ => String::new(),
                };
                self.copy_to_clipboard(&entry_name, secret)?;
            }
            super::screens::view_entry::ViewEntryAction::CopyUrl(url) => {
                let status = match self.copy_text_to_clipboard(&url) {
                    Ok(()) => ("URL copied to clipboard.".to_string(), false),
                    Err(e) => (format!("Failed to copy URL: {}", e), true),
                };
                if let AppView::ViewEntry(view_entry) = &mut self.view {
                    view_entry.set_status(status.0, status.1);
                }
            }
            super::screens::view_entry::ViewEntryAction::OpenUrl(url) => {
                let status = match crate::links::open_url(&url) {
                    Ok(()) => ("Opened URL in your browser.".to_string(), false),
                    Err(e) => (format!("Failed to open URL: {}", e), true),
                };
                if let AppView::ViewEntry(view_entry) = &mut self.view {
                    view_entry.set_status(status.0, status.1);
                }
            }
            super::screens::view_entry::ViewEntryAction::Continue => {}
        }
        Ok(())
    }

    // ─── Edit Entry ──────────────────────────────────────────────────

    fn handle_edit_entry_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let (action, original_name) = match &mut self.view {
            AppView::EditEntry(edit_entry) => {
                let original = edit_entry.original_name.clone();
                (edit_entry.handle_key(key, modifiers), original)
            }
            _ => return Ok(()),
        };

        match action {
            super::screens::edit_entry::EditEntryAction::Save {
                entry: updated_entry,
                secondary_password,
            } => {
                if let Some(session) = &mut self.session {
                    let snapshot = session.vault.clone();
                    replace_entry_in_vault(
                        &mut session.vault,
                        &original_name,
                        *updated_entry,
                        secondary_password
                            .as_ref()
                            .map(|password| password.as_str()),
                    )?;
                    if let Err(error) = session.save() {
                        session.vault = snapshot;
                        return Err(error);
                    }
                    self.show_success("Entry updated successfully!".to_string());
                }
            }
            super::screens::edit_entry::EditEntryAction::Cancel => {
                self.return_to_dashboard();
            }
            super::screens::edit_entry::EditEntryAction::Error(error) => {
                self.show_message(
                    "Edit Error".to_string(),
                    format!("Unable to update entry: {error}"),
                    true,
                );
            }
            super::screens::edit_entry::EditEntryAction::Continue => {}
        }
        Ok(())
    }

    // ─── Confirm ─────────────────────────────────────────────────────

    fn handle_confirm_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let (result, action) = match &mut self.view {
            AppView::Confirm(confirm) => {
                (confirm.handle_key(key, modifiers), confirm.action.clone())
            }
            _ => return Ok(()),
        };

        match result {
            Some(true) => match action {
                ConfirmAction::Delete(entry_name) => {
                    let requires_secondary_password = self
                        .session
                        .as_ref()
                        .and_then(|session| session.vault.find_entry(&entry_name))
                        .map(|entry| entry.has_secondary_password)
                        .ok_or_else(|| TermKeyError::EntryNotFound(entry_name.clone()))?;
                    if requires_secondary_password {
                        self.pending_delete_entry_name = Some(entry_name);
                        self.view = AppView::ViewPassword(ViewPasswordScreen::new(
                            "Enter Secondary Password to Delete",
                        ));
                    } else if let Some(session) = &mut self.session {
                        let snapshot = session.vault.clone();
                        delete_entry_from_vault(&mut session.vault, &entry_name, None)?;
                        if let Err(error) = session.save() {
                            session.vault = snapshot;
                            return Err(error);
                        }
                        self.show_success("Entry deleted successfully!".to_string());
                    }
                }
            },
            Some(false) => {
                self.return_to_dashboard();
            }
            None => {}
        }
        Ok(())
    }

    // ─── Clipboard ───────────────────────────────────────────────────

    fn copy_to_clipboard(&mut self, entry_name: &str, secret: Zeroizing<String>) -> Result<()> {
        let timeout = self.config.clipboard_timeout_secs;
        let cleanup = crate::clipboard::copy_and_clear(secret, Duration::from_secs(timeout))?;
        drop(cleanup);
        self.clipboard_clear_time = Some(Instant::now() + Duration::from_secs(timeout));
        self.view = AppView::CopyCountdown {
            entry_name: entry_name.to_string(),
            seconds_left: timeout,
        };
        Ok(())
    }

    fn copy_text_to_clipboard(&self, text: &str) -> Result<()> {
        use arboard::Clipboard;

        let mut clipboard = Clipboard::new().map_err(|e| {
            TermKeyError::Clipboard(format!("failed to access system clipboard: {}", e))
        })?;
        clipboard
            .set_text(text.to_string())
            .map_err(|e| TermKeyError::Clipboard(format!("failed to write clipboard: {}", e)))
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

    fn render_no_recovery_static(frame: &mut Frame) {
        use ratatui::{
            layout::{Constraint, Direction, Layout},
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Paragraph, Wrap},
        };

        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(10),
                Constraint::Min(1),
            ])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Recovery Not Available ")
            .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(Color::Red));

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No recovery phrase has been configured.",
                Style::default().fg(Color::White),
            )),
            Line::from(Span::styled(
                "  Set one up in Settings the next time you log in.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "  F2",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " Delete vault & start over",
                    Style::default().fg(Color::White),
                ),
                Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
                Span::styled("Esc", Style::default().fg(Color::Cyan)),
                Span::styled(" Cancel", Style::default().fg(Color::DarkGray)),
            ]),
        ];

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, chunks[1]);
    }

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
            .constraints([
                Constraint::Min(1),
                Constraint::Length(7),
                Constraint::Min(1),
            ])
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
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from("  ↑/↓       Navigate entry list"),
            Line::from("  0-9       Type an entry number"),
            Line::from("  Enter     Jump to typed number or view selected entry"),
            Line::from("  /         Start filtering entries"),
            Line::from("  Esc       Clear filter or number entry"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Commands:",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from("  Shift+A   Add new entry"),
            Line::from("            Password entries can generate a strong secret in the add form"),
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
            Line::from("  Entry View: r reveal, c copy secret, u copy URL, o open URL"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Global Shortcuts:",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
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
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
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

    fn render_copy_countdown_static(frame: &mut Frame, entry_name: &str, seconds_left: u64) {
        use ratatui::{
            layout::{Constraint, Direction, Layout},
            style::{Color, Modifier, Style},
            widgets::{Block, Borders, Paragraph, Wrap},
        };

        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(7),
                Constraint::Min(1),
            ])
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
            "Secret for '{}' copied to clipboard!\n\nClears in {} second{} only if unchanged.\n\nPress Esc to dismiss",
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
            .constraints([
                Constraint::Min(1),
                Constraint::Length(5),
                Constraint::Min(1),
            ])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Find Entries ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
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

    fn handle_input_result(
        &mut self,
        result: super::screens::input::InputResult,
        purpose: InputPurpose,
    ) -> Result<()> {
        use super::screens::input::InputResult;
        match result {
            InputResult::Cancel => {
                self.pending_export_password = None;
                self.pending_new_password = None;
                self.return_to_dashboard();
            }
            InputResult::Submit(value) => match purpose {
                InputPurpose::ExportPath => {
                    let input = InputScreen::new("Export Vault", "Enter backup password:", true);
                    self.pending_export_password = Some(value.to_string());
                    self.view = AppView::Input(input, InputPurpose::ExportPassword);
                }
                InputPurpose::ExportPassword => {
                    let input = InputScreen::new("Export Vault", "Confirm backup password:", true);
                    self.pending_new_password = Some(value);
                    self.view = AppView::Input(input, InputPurpose::ConfirmExportPassword);
                }
                InputPurpose::ConfirmExportPassword => {
                    if let Some(path) = self.pending_export_password.take() {
                        if let Some(export_pass) = self.pending_new_password.take() {
                            if *export_pass != *value {
                                self.show_message(
                                    "Export Error".to_string(),
                                    "Passwords do not match!".to_string(),
                                    true,
                                );
                            } else if let Some(session) = &self.session {
                                let backup_path = std::path::Path::new(&path).join("backup.ck");
                                match crate::vault::storage::write_backup(
                                    &session.vault,
                                    export_pass.as_bytes(),
                                    &backup_path,
                                ) {
                                    Ok(_) => {
                                        self.show_success(format!(
                                            "Vault exported to {}/backup.ck",
                                            path
                                        ));
                                    }
                                    Err(e) => {
                                        self.show_message(
                                            "Export Error".to_string(),
                                            format!("Failed to export: {}", e),
                                            true,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                InputPurpose::ImportPath => {
                    let input = InputScreen::new("Import Vault", "Enter backup password:", true);
                    self.pending_export_password = Some(value.to_string());
                    self.view = AppView::Input(input, InputPurpose::ImportPassword);
                }
                InputPurpose::ImportPassword => {
                    if let Some(path) = self.pending_export_password.take() {
                        if let Some(session) = &mut self.session {
                            match crate::vault::storage::read_backup(
                                value.as_bytes(),
                                std::path::Path::new(&path),
                            ) {
                                Ok(backup) => {
                                    let snapshot = session.vault.clone();
                                    match crate::commands::import::merge_non_conflicting_entries(
                                        &mut session.vault,
                                        backup,
                                    ) {
                                        Ok(imported) => {
                                            if imported > 0 {
                                                if let Err(error) = session.save() {
                                                    session.vault = snapshot;
                                                    return Err(error);
                                                }
                                            }
                                            self.show_success(format!(
                                                "Imported {} entries from backup",
                                                imported
                                            ));
                                        }
                                        Err(error) => {
                                            session.vault = snapshot;
                                            self.show_message(
                                                "Import Error".to_string(),
                                                format!("Failed to import: {}", error),
                                                true,
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.show_message(
                                        "Import Error".to_string(),
                                        format!("Failed to import: {}", e),
                                        true,
                                    );
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
                        if *new_pass != *value {
                            self.show_message(
                                "Error".to_string(),
                                "Passwords do not match!".to_string(),
                                true,
                            );
                            return Ok(());
                        }
                        let save_result = if let Some(session) = &mut self.session {
                            session.change_master_password(new_pass)
                        } else {
                            return Ok(());
                        };
                        match save_result {
                            Ok(_) => {
                                let active_recovery = self
                                    .session
                                    .as_ref()
                                    .map(|session| session.vault_id())
                                    .and_then(|vault_id| {
                                        crate::config::load_config()
                                            .ok()
                                            .map(|config| config.has_active_recovery_for(vault_id))
                                    })
                                    .unwrap_or(false);
                                self.show_success(
                                    crate::commands::passwd::password_change_success_message(
                                        active_recovery,
                                    )
                                    .to_string(),
                                );
                            }
                            Err(e) => {
                                self.show_message(
                                    "Password Change Error".to_string(),
                                    format!("Failed to change password: {}", e),
                                    true,
                                );
                            }
                        }
                    }
                }
            },
        }
        Ok(())
    }
}

#[derive(Clone)]
pub enum ConfirmAction {
    Delete(String),
}

#[cfg(test)]
mod tests {
    use super::{
        add_entry_to_vault, classify_secondary_reveal_error, delete_entry_from_vault,
        replace_entry_in_vault, App, AppView, ConfirmAction, SecondaryRevealError,
    };
    use crate::commands::test_support::protected_entry;
    use crate::config::model::Config;
    use crate::error::TermKeyError;
    use crate::ui::screens::confirm::ConfirmScreen;
    use crate::ui::widgets::dashboard::Dashboard;
    use crate::update::UpdateStatus;
    use crate::vault::format::decode_v3;
    use crate::vault::model::VaultHeader;
    use crate::vault::model::{Entry, SecretType, VaultData};
    use crate::vault::session::VaultSession;
    use chrono::Utc;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::{backend::TestBackend, Terminal};
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;
    use zeroize::Zeroizing;

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            secret: "secret".to_string(),
            secret_type: SecretType::Password,
            network: String::new(),
            public_address: None,
            username: None,
            url: None,
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

    fn app_with_protected_entry() -> (TempDir, PathBuf, App) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let vault = VaultData {
            entries: vec![protected_entry("Éntry", "secret", "view-pass")],
            version: 1,
            revision: 0,
        };
        let session = VaultSession::create(
            vault,
            Zeroizing::new("master-password".to_string()),
            path.clone(),
        )
        .unwrap();
        let view = AppView::Dashboard(Dashboard::new(session.vault.metadata()));
        let app = App {
            config: Config::default(),
            session: Some(session),
            view,
            should_quit: false,
            clipboard_clear_time: None,
            pending_export_password: None,
            pending_new_password: None,
            pending_view_entry_idx: None,
            pending_copy_entry_idx: None,
            pending_delete_entry_name: None,
            update_check_rx: None,
            update_status: UpdateStatus::Unknown,
        };
        (dir, path, app)
    }

    fn enter_text(app: &mut App, text: &str) {
        for character in text.chars() {
            app.handle_key(KeyCode::Char(character), KeyModifiers::NONE)
                .unwrap();
        }
    }

    fn persisted_vault(path: &std::path::Path) -> VaultData {
        decode_v3(
            b"master-password",
            &std::fs::read(path).unwrap(),
            VaultHeader::MAGIC,
        )
        .unwrap()
        .vault
    }

    #[test]
    fn tui_login_surfaces_post_migration_recovery_notice() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        crate::vault::storage::write_vault(&VaultData::new(), b"correct-password", &path).unwrap();
        std::fs::write(dir.path().join("config.json"), b"{not valid json").unwrap();
        let mut app = App {
            config: Config::default(),
            session: None,
            view: AppView::Login(crate::ui::screens::login::LoginScreen::new()),
            should_quit: false,
            clipboard_clear_time: None,
            pending_export_password: None,
            pending_new_password: None,
            pending_view_entry_idx: None,
            pending_copy_entry_idx: None,
            pending_delete_entry_name: None,
            update_check_rx: None,
            update_status: UpdateStatus::Unknown,
        };

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());
        app.unlock_vault(Zeroizing::new("correct-password".into()))
            .unwrap();
        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        assert!(app.session.is_some());
        assert!(matches!(
            app.view,
            AppView::Message {
                message,
                is_error: false,
                ..
            } if message.to_ascii_lowercase().contains("recovery phrase")
        ));
    }

    #[test]
    fn tui_rejects_duplicate_add_and_rename() {
        let mut vault = VaultData::new();
        add_entry_to_vault(&mut vault, entry("Alpha")).unwrap();
        add_entry_to_vault(&mut vault, entry("Bravo")).unwrap();

        let duplicate_add = add_entry_to_vault(&mut vault, entry(" alpha "));
        assert!(matches!(
            duplicate_add,
            Err(TermKeyError::EntryAlreadyExists(_))
        ));

        let duplicate_rename = replace_entry_in_vault(&mut vault, "Bravo", entry("ALPHA"), None);
        assert!(matches!(
            duplicate_rename,
            Err(TermKeyError::EntryAlreadyExists(_))
        ));
        assert!(vault.find_entry("Bravo").is_some());
        assert_eq!(vault.entries.len(), 2);
    }

    #[test]
    fn tui_protected_delete_requires_correct_password_transactionally() {
        let protected = protected_entry("Protected", "secret", "view-pass");
        let mut vault = VaultData {
            entries: vec![protected],
            version: 1,
            revision: 0,
        };
        let original = serde_json::to_vec(&vault).unwrap();

        assert!(matches!(
            delete_entry_from_vault(&mut vault, "Protected", None),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);
        assert!(matches!(
            delete_entry_from_vault(&mut vault, "Protected", Some("wrong-pass")),
            Err(TermKeyError::SecondaryPasswordWrong)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);

        delete_entry_from_vault(&mut vault, "Protected", Some("view-pass")).unwrap();
        assert!(vault.entries.is_empty());
    }

    #[test]
    fn tui_protected_replace_requires_correct_password_transactionally() {
        let protected = protected_entry("Protected", "secret", "view-pass");
        let mut replacement = entry("Protected");
        replacement.notes = "updated".to_string();
        let mut vault = VaultData {
            entries: vec![protected],
            version: 1,
            revision: 0,
        };
        let original = serde_json::to_vec(&vault).unwrap();

        assert!(matches!(
            replace_entry_in_vault(&mut vault, "Protected", replacement.clone(), None),
            Err(TermKeyError::SecondaryPasswordRequired)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);
        assert!(matches!(
            replace_entry_in_vault(
                &mut vault,
                "Protected",
                replacement.clone(),
                Some("wrong-pass")
            ),
            Err(TermKeyError::SecondaryPasswordWrong)
        ));
        assert_eq!(serde_json::to_vec(&vault).unwrap(), original);

        replace_entry_in_vault(&mut vault, "Protected", replacement, Some("view-pass")).unwrap();
        assert_eq!(vault.entries[0].notes, "updated");
    }

    #[test]
    fn tui_protected_delete_confirmation_routes_through_password_retry_and_submission() {
        let (_dir, path, mut app) = app_with_protected_entry();
        app.view = AppView::Confirm(ConfirmScreen::new(
            "Delete Entry",
            "Delete protected entry?",
            ConfirmAction::Delete("éntry".to_string()),
        ));
        let original_memory = serde_json::to_vec(&app.session.as_ref().unwrap().vault).unwrap();
        let original_disk = std::fs::read(&path).unwrap();

        app.handle_key(KeyCode::Char('y'), KeyModifiers::NONE)
            .unwrap();

        assert!(matches!(app.view, AppView::ViewPassword(_)));
        assert_eq!(app.pending_delete_entry_name.as_deref(), Some("éntry"));
        assert_eq!(
            serde_json::to_vec(&app.session.as_ref().unwrap().vault).unwrap(),
            original_memory
        );
        assert_eq!(std::fs::read(&path).unwrap(), original_disk);

        enter_text(&mut app, "wrong-pass");
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();

        assert!(matches!(app.view, AppView::ViewPassword(_)));
        assert_eq!(app.pending_delete_entry_name.as_deref(), Some("éntry"));
        assert_eq!(
            serde_json::to_vec(&app.session.as_ref().unwrap().vault).unwrap(),
            original_memory
        );
        assert_eq!(std::fs::read(&path).unwrap(), original_disk);

        enter_text(&mut app, "view-pass");
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();

        assert!(matches!(
            app.view,
            AppView::Message {
                is_error: false,
                ..
            }
        ));
        assert!(app.session.as_ref().unwrap().vault.entries.is_empty());
        assert!(persisted_vault(&path).entries.is_empty());
    }

    #[test]
    fn tui_protected_edit_transitions_from_dashboard_and_submits_authenticated_change() {
        let (_dir, path, mut app) = app_with_protected_entry();
        let original_memory = serde_json::to_vec(&app.session.as_ref().unwrap().vault).unwrap();
        let original_disk = std::fs::read(&path).unwrap();

        app.handle_key(KeyCode::Char('E'), KeyModifiers::SHIFT)
            .unwrap();

        assert!(matches!(app.view, AppView::EditEntry(_)));
        app.handle_key(KeyCode::Char('!'), KeyModifiers::NONE)
            .unwrap();
        app.handle_key(KeyCode::Char('s'), KeyModifiers::CONTROL)
            .unwrap();
        assert!(matches!(app.view, AppView::EditEntry(_)));
        assert_eq!(
            serde_json::to_vec(&app.session.as_ref().unwrap().vault).unwrap(),
            original_memory
        );
        assert_eq!(std::fs::read(&path).unwrap(), original_disk);

        app.handle_key(KeyCode::BackTab, KeyModifiers::SHIFT)
            .unwrap();
        enter_text(&mut app, "view-pass");
        app.handle_key(KeyCode::Char('s'), KeyModifiers::CONTROL)
            .unwrap();

        assert!(matches!(
            app.view,
            AppView::Message {
                is_error: false,
                ..
            }
        ));
        let saved = app
            .session
            .as_ref()
            .unwrap()
            .vault
            .find_entry("éntry!")
            .unwrap();
        assert_eq!(&*saved.reveal_secret(Some("view-pass")).unwrap(), "secret");
        let persisted = persisted_vault(&path);
        let saved = persisted.find_entry("éNTRY!").unwrap();
        assert_eq!(&*saved.reveal_secret(Some("view-pass")).unwrap(), "secret");
    }

    #[test]
    fn wrong_secondary_password_is_retryable_but_structural_error_is_fatal() {
        assert!(matches!(
            classify_secondary_reveal_error(TermKeyError::SecondaryPasswordWrong),
            SecondaryRevealError::Retry
        ));

        let classified = classify_secondary_reveal_error(TermKeyError::InvalidEntry(
            "malformed protected fields".to_string(),
        ));
        assert!(matches!(
            classified,
            SecondaryRevealError::Fatal(message)
                if message.contains("malformed protected fields")
        ));
    }

    #[test]
    fn copy_countdown_renders_large_timeout_and_conditional_cleanup() {
        let backend = TestBackend::new(100, 9);
        let mut terminal = Terminal::new(backend).unwrap();
        let configured_timeout = 300_u64;

        terminal
            .draw(|frame| {
                App::render_copy_countdown_static(frame, "Large Timeout", configured_timeout);
            })
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("300 seconds"));
        assert!(rendered.contains("only if unchanged"));
        assert!(rendered.contains("Press Esc to dismiss"));
    }
}
