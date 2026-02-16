use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::ui::app::ConfirmAction;

pub struct ConfirmScreen {
    title: String,
    message: String,
    pub action: ConfirmAction,
    selected: bool,
}

impl ConfirmScreen {
    pub fn new(title: &str, message: &str, action: ConfirmAction) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            action,
            selected: false,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, _modifiers: KeyModifiers) -> Option<bool> {
        match key {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                self.selected = !self.selected;
                None
            }
            KeyCode::Char('y') => Some(true),
            KeyCode::Char('n') => Some(false),
            KeyCode::Enter => Some(self.selected),
            KeyCode::Esc => Some(false),
            _ => None,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(8), Constraint::Min(1)])
            .split(area);

        let inner_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(3)])
            .split(centered_rect(60, chunks[1]));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(Color::Yellow));

        let message_para = Paragraph::new(self.message.as_str())
            .style(Style::default().fg(Color::White));

        frame.render_widget(block.clone(), chunks[1]);
        frame.render_widget(message_para, inner_chunks[0]);

        let yes_style = if self.selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };

        let no_style = if !self.selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red)
        };

        let buttons = Line::from(vec![
            Span::raw("         "),
            Span::styled(" Yes ", yes_style),
            Span::raw("   "),
            Span::styled(" No ", no_style),
        ]);

        let button_para = Paragraph::new(buttons);
        frame.render_widget(button_para, inner_chunks[1]);
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
