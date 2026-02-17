use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::config::model::Config;

#[derive(Clone, PartialEq)]
enum SettingsField {
    ClipboardTimeout,
    RecoveryStatus,
}

const FIELDS: [SettingsField; 2] = [
    SettingsField::ClipboardTimeout,
    SettingsField::RecoveryStatus,
];

pub enum SettingsAction {
    Continue,
    Save(Config),
    Cancel,
    SetupRecovery,
}

pub struct SettingsScreen {
    config: Config,
    selected: usize,
    editing: bool,
    edit_buffer: String,
}

impl SettingsScreen {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            selected: 0,
            editing: false,
            edit_buffer: String::new(),
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> SettingsAction {
        if key == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            return SettingsAction::Cancel;
        }

        if self.editing {
            return self.handle_editing_key(key);
        }

        match key {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                SettingsAction::Continue
            }
            KeyCode::Down => {
                if self.selected < FIELDS.len() - 1 {
                    self.selected += 1;
                }
                SettingsAction::Continue
            }
            KeyCode::Enter => {
                match FIELDS[self.selected] {
                    SettingsField::ClipboardTimeout => {
                        self.editing = true;
                        self.edit_buffer = self.config.clipboard_timeout_secs.to_string();
                    }
                    SettingsField::RecoveryStatus => {
                        return SettingsAction::SetupRecovery;
                    }
                }
                SettingsAction::Continue
            }
            KeyCode::Esc => SettingsAction::Save(self.config.clone()),
            KeyCode::Char('q') => SettingsAction::Save(self.config.clone()),
            _ => SettingsAction::Continue,
        }
    }

    fn handle_editing_key(&mut self, key: KeyCode) -> SettingsAction {
        match key {
            KeyCode::Char(c) if c.is_ascii_digit() => {
                self.edit_buffer.push(c);
                SettingsAction::Continue
            }
            KeyCode::Backspace => {
                self.edit_buffer.pop();
                SettingsAction::Continue
            }
            KeyCode::Enter => {
                if let Ok(val) = self.edit_buffer.parse::<u64>() {
                    if val > 0 {
                        self.config.clipboard_timeout_secs = val;
                    }
                }
                self.editing = false;
                self.edit_buffer.clear();
                SettingsAction::Continue
            }
            KeyCode::Esc => {
                self.editing = false;
                self.edit_buffer.clear();
                SettingsAction::Continue
            }
            _ => SettingsAction::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(14),
                Constraint::Min(1),
            ])
            .split(area);

        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Settings",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        // Clipboard timeout
        let timeout_selected = self.selected == 0;
        let timeout_style = if timeout_selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        if self.editing && timeout_selected {
            lines.push(Line::from(vec![
                Span::styled("  Clipboard timeout: ", Style::default().fg(Color::White)),
                Span::styled(&self.edit_buffer, Style::default().fg(Color::Yellow)),
                Span::styled("█", Style::default().fg(Color::Cyan)),
                Span::styled(" seconds", Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                format!(
                    "  Clipboard timeout: {} seconds",
                    self.config.clipboard_timeout_secs
                ),
                timeout_style,
            )));
        }

        lines.push(Line::from(""));

        // Recovery status
        let recovery_selected = self.selected == 1;
        let recovery_style = if recovery_selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let recovery_status = if self.config.recovery.is_some() {
            "Configured"
        } else {
            "Not set"
        };
        lines.push(Line::from(Span::styled(
            format!("  Recovery question: {}", recovery_status),
            recovery_style,
        )));

        lines.push(Line::from(""));

        // Vault path (display only)
        lines.push(Line::from(vec![
            Span::styled("  Vault path: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&self.config.vault_path, Style::default().fg(Color::DarkGray)),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  ↑/↓ Navigate  |  Enter Edit  |  Esc Save & Close",
            Style::default().fg(Color::DarkGray),
        )));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Settings ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, chunks[1]);
    }
}
