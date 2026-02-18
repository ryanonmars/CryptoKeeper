use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

use crate::vault::model::EntryMeta;

use super::entry_table::EntryTable;
use super::menu_bar::MenuBar;
use super::status_bar::StatusBar;

pub struct Dashboard {
    table: EntryTable,
    menu_bar: MenuBar,
}

impl Dashboard {
    pub fn new(entries: Vec<EntryMeta>) -> Self {
        Self {
            table: EntryTable::new(entries),
            menu_bar: MenuBar::new(),
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.table.selected_index()
    }

    pub fn set_filter(&mut self, filter: String) {
        self.table.set_filter(filter);
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        self.table.handle_key(key, modifiers);
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let menu_lines = self.menu_bar.lines_for_width(area.width).max(1).min(3);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(5),
                Constraint::Length(menu_lines),
            ])
            .split(area);

        let entry_count = self.table.filtered_count();
        let status_bar = StatusBar::new(
            "CryptoKeeper",
            entry_count,
            self.table.filter_text(),
            self.table.number_buffer(),
        );
        status_bar.render(frame, chunks[0]);

        self.table.render(frame, chunks[1]);

        self.menu_bar.render(frame, chunks[2]);
    }
}
