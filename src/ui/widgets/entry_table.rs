use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame,
};

use crate::vault::model::EntryMeta;

pub struct EntryTable {
    entries: Vec<EntryMeta>,
    selected: usize,
    filter: String,
    scroll_offset: usize,
    number_buffer: String,
}

impl EntryTable {
    pub fn new(entries: Vec<EntryMeta>) -> Self {
        Self {
            entries,
            selected: 0,
            filter: String::new(),
            scroll_offset: 0,
            number_buffer: String::new(),
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        let filtered = self.filtered_entries();
        if filtered.is_empty() {
            None
        } else {
            Some(filtered[self.selected].0)
        }
    }

    pub fn filter_text(&self) -> &str {
        &self.filter
    }

    pub fn filtered_count(&self) -> usize {
        self.filtered_entries().len()
    }

    pub fn number_buffer(&self) -> &str {
        &self.number_buffer
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.selected = 0;
    }

    pub fn handle_key(&mut self, key: KeyCode, _modifiers: KeyModifiers) {
        let filtered_len = self.filtered_entries().len();

        if filtered_len == 0 {
            return;
        }

        match key {
            KeyCode::Up => {
                self.number_buffer.clear();
                if self.selected > 0 {
                    self.selected -= 1;
                } else {
                    self.selected = filtered_len - 1;
                }
            }
            KeyCode::Down => {
                self.number_buffer.clear();
                self.selected = (self.selected + 1) % filtered_len;
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                self.number_buffer.push(c);
            }
            KeyCode::Enter if !self.number_buffer.is_empty() => {
                if let Ok(num) = self.number_buffer.parse::<usize>() {
                    if num > 0 && num <= filtered_len {
                        self.selected = num - 1;
                    }
                }
                self.number_buffer.clear();
            }
            KeyCode::Char('/') => {
                self.number_buffer.clear();
            }
            KeyCode::Backspace => {
                if !self.number_buffer.is_empty() {
                    self.number_buffer.pop();
                } else if !self.filter.is_empty() {
                    self.filter.pop();
                    self.selected = 0;
                }
            }
            KeyCode::Esc => {
                if !self.number_buffer.is_empty() {
                    self.number_buffer.clear();
                } else if !self.filter.is_empty() {
                    self.filter.clear();
                    self.selected = 0;
                }
            }
            _ => {}
        }
    }

    fn filtered_entries(&self) -> Vec<(usize, &EntryMeta)> {
        if self.filter.is_empty() {
            self.entries.iter().enumerate().collect()
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    e.name.to_lowercase().contains(&filter_lower)
                        || e.network.to_lowercase().contains(&filter_lower)
                })
                .collect()
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let filtered = self.filtered_entries();

        if filtered.is_empty() {
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Entries ")
                .border_style(Style::default().fg(Color::Cyan));

            let empty_msg = if self.filter.is_empty() {
                "No entries yet. Press 'a' to add one."
            } else {
                "No entries match filter."
            };

            let empty = ratatui::widgets::Paragraph::new(empty_msg)
                .block(block)
                .style(Style::default().fg(Color::DarkGray));

            frame.render_widget(empty, area);
            return;
        }

        let header_cells = ["#", "Name", "Type", "Network", "Public Key/Address"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
        let header = Row::new(header_cells).height(1);

        let rows = filtered.iter().enumerate().map(|(idx, (_original_idx, entry))| {
            let display_num = idx + 1;
            let address_display = entry.public_address.as_ref()
                .or(entry.username.as_ref())
                .map(|s| {
                    if s.len() > 30 {
                        format!("{}...", &s[..27])
                    } else {
                        s.clone()
                    }
                })
                .unwrap_or_else(|| String::from(""));

            let lock_indicator = if entry.has_secondary_password { " [locked]" } else { "" };
            let name_display = format!("{}{}", entry.name, lock_indicator);

            let cells = vec![
                Cell::from(display_num.to_string()),
                Cell::from(name_display),
                Cell::from(entry.secret_type.to_string()),
                Cell::from(entry.network.clone()),
                Cell::from(address_display),
            ];

            let style = if idx == self.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            Row::new(cells).style(style)
        });

        let widths = [
            ratatui::layout::Constraint::Length(4),
            ratatui::layout::Constraint::Percentage(30),
            ratatui::layout::Constraint::Percentage(20),
            ratatui::layout::Constraint::Percentage(20),
            ratatui::layout::Constraint::Percentage(30),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Entries ")
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .column_spacing(1);

        frame.render_widget(table, area);
    }
}
