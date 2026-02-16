use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{List, ListItem, Paragraph},
    Frame,
};

pub struct PasswordInput {
    pub buffer: String,
    pub prompt: String,
}

impl PasswordInput {
    pub fn new(prompt: &str) -> Self {
        Self {
            buffer: String::new(),
            prompt: prompt.to_string(),
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> InputResult {
        match key {
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                InputResult::Cancelled
            }
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                if self.buffer.is_empty() {
                    InputResult::Cancelled
                } else {
                    InputResult::Continue
                }
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                InputResult::Continue
            }
            KeyCode::Backspace => {
                self.buffer.pop();
                InputResult::Continue
            }
            KeyCode::Enter => InputResult::Done(self.buffer.clone()),
            _ => InputResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let prompt_text = format!("{}{}", self.prompt, "*".repeat(self.buffer.len()));
        let paragraph = Paragraph::new(prompt_text);
        frame.render_widget(paragraph, area);
    }
}

pub enum InputResult {
    Continue,
    Done(String),
    Cancelled,
}

pub struct CommandInput {
    pub buffer: String,
    pub prompt: String,
    commands: Vec<(&'static str, &'static str)>,
    pub show_completions: bool,
    pub selected_completion: usize,
}

impl CommandInput {
    pub fn new(commands: Vec<(&'static str, &'static str)>) -> Self {
        Self {
            buffer: String::new(),
            prompt: "cryptokeeper> ".to_string(),
            commands,
            show_completions: false,
            selected_completion: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> InputResult {
        match key {
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                InputResult::Cancelled
            }
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                if self.buffer.is_empty() {
                    InputResult::Cancelled
                } else {
                    InputResult::Continue
                }
            }
            KeyCode::Char('/') => {
                self.buffer.push('/');
                self.show_completions = true;
                self.selected_completion = 0;
                InputResult::Continue
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                if self.buffer.starts_with('/') {
                    self.show_completions = true;
                    self.selected_completion = 0;
                }
                InputResult::Continue
            }
            KeyCode::Backspace => {
                self.buffer.pop();
                if self.buffer.starts_with('/') {
                    self.show_completions = true;
                    self.selected_completion = 0;
                } else {
                    self.show_completions = false;
                }
                InputResult::Continue
            }
            KeyCode::Down if self.show_completions => {
                let matches = self.get_matching_commands();
                if !matches.is_empty() {
                    self.selected_completion = (self.selected_completion + 1) % matches.len();
                }
                InputResult::Continue
            }
            KeyCode::Up if self.show_completions => {
                let matches = self.get_matching_commands();
                if !matches.is_empty() {
                    self.selected_completion = if self.selected_completion == 0 {
                        matches.len() - 1
                    } else {
                        self.selected_completion - 1
                    };
                }
                InputResult::Continue
            }
            KeyCode::Tab if self.show_completions => {
                let matches = self.get_matching_commands();
                if !matches.is_empty() && self.selected_completion < matches.len() {
                    self.buffer = format!("/{}", matches[self.selected_completion].0);
                }
                InputResult::Continue
            }
            KeyCode::Enter => {
                if self.show_completions {
                    let matches = self.get_matching_commands();
                    if !matches.is_empty() && self.selected_completion < matches.len() {
                        self.buffer = format!("/{}", matches[self.selected_completion].0);
                    }
                }
                self.show_completions = false;
                let result = self.buffer.clone();
                self.buffer.clear();
                InputResult::Done(result)
            }
            KeyCode::Esc if self.show_completions => {
                self.show_completions = false;
                InputResult::Continue
            }
            _ => InputResult::Continue,
        }
    }

    fn get_matching_commands(&self) -> Vec<(&'static str, &'static str)> {
        if !self.buffer.starts_with('/') {
            return vec![];
        }

        let prefix = &self.buffer[1..];
        self.commands
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(prefix))
            .copied()
            .collect()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(area);

        let input_text = format!("{}{}", self.prompt, self.buffer);
        let input_paragraph = Paragraph::new(input_text);
        frame.render_widget(input_paragraph, chunks[0]);

        if self.show_completions {
            let matches = self.get_matching_commands();
            if !matches.is_empty() {
                let items: Vec<ListItem> = matches
                    .iter()
                    .enumerate()
                    .map(|(i, (cmd, desc))| {
                        let prefix = if i == self.selected_completion {
                            "▸ "
                        } else {
                            "  "
                        };
                        let content = format!("{}/{:<10} {}", prefix, cmd, desc);
                        let style = if i == self.selected_completion {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default()
                        };
                        ListItem::new(content).style(style)
                    })
                    .collect();

                let list = List::new(items);
                
                let completion_height = matches.len().min(10) as u16;
                let completion_area = Rect {
                    x: chunks[1].x,
                    y: chunks[1].y,
                    width: chunks[1].width,
                    height: completion_height,
                };
                
                frame.render_widget(list, completion_area);
            }
        }
    }
}
