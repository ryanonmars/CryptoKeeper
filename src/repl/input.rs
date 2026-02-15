use crossterm::{
    cursor, event::{self, Event, KeyCode, KeyModifiers},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::io::{self, Write};

pub struct CommandInput {
    commands: Vec<(&'static str, &'static str)>,
    completion_lines: usize,
    display_upward: bool,
}

impl CommandInput {
    pub fn new(commands: Vec<(&'static str, &'static str)>) -> Self {
        Self {
            commands,
            completion_lines: 0,
            display_upward: false,
        }
    }

    pub fn read_line(&mut self, prompt: &str) -> io::Result<Option<String>> {
        let mut stdout = io::stdout();
        let mut buffer = String::new();
        let mut show_completions = false;
        let mut selected_completion = 0;

        execute!(stdout, Print(prompt))?;
        stdout.flush()?;

        loop {
            if let Event::Key(key_event) = event::read()? {
                match key_event.code {
                    KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        if show_completions {
                            self.clear_completions()?;
                        }
                        execute!(stdout, Print("\n"))?;
                        return Ok(None);
                    }
                    KeyCode::Char('d') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        if buffer.is_empty() {
                            if show_completions {
                                self.clear_completions()?;
                            }
                            execute!(stdout, Print("\n"))?;
                            return Ok(None);
                        }
                    }
                    KeyCode::Char('/') => {
                        if show_completions {
                            self.clear_completions()?;
                        }
                        buffer.push('/');
                        
                        execute!(stdout, Clear(ClearType::CurrentLine), cursor::MoveToColumn(0))?;
                        execute!(stdout, Print(prompt), Print(&buffer))?;
                        stdout.flush()?;
                        
                        show_completions = true;
                        selected_completion = 0;
                        self.display_completions(&buffer, selected_completion)?;
                    }
                    KeyCode::Char(c) => {
                        if show_completions {
                            self.clear_completions()?;
                        }
                        buffer.push(c);
                        
                        execute!(stdout, Clear(ClearType::CurrentLine), cursor::MoveToColumn(0))?;
                        execute!(stdout, Print(prompt), Print(&buffer))?;
                        stdout.flush()?;
                        
                        if buffer.starts_with('/') {
                            show_completions = true;
                            selected_completion = 0;
                            self.display_completions(&buffer, selected_completion)?;
                        }
                    }
                    KeyCode::Backspace => {
                        if !buffer.is_empty() {
                            if show_completions {
                                self.clear_completions()?;
                            }
                            buffer.pop();
                            
                            execute!(stdout, Clear(ClearType::CurrentLine), cursor::MoveToColumn(0))?;
                            execute!(stdout, Print(prompt), Print(&buffer))?;
                            stdout.flush()?;
                            
                            if buffer.starts_with('/') {
                                show_completions = true;
                                selected_completion = 0;
                                self.display_completions(&buffer, selected_completion)?;
                            } else {
                                show_completions = false;
                            }
                        }
                    }
                    KeyCode::Down if show_completions => {
                        let matches = self.get_matching_commands(&buffer);
                        if !matches.is_empty() {
                            self.clear_completions()?;
                            selected_completion = (selected_completion + 1) % matches.len();
                            self.display_completions(&buffer, selected_completion)?;
                        }
                    }
                    KeyCode::Up if show_completions => {
                        let matches = self.get_matching_commands(&buffer);
                        if !matches.is_empty() {
                            self.clear_completions()?;
                            selected_completion = if selected_completion == 0 {
                                matches.len() - 1
                            } else {
                                selected_completion - 1
                            };
                            self.display_completions(&buffer, selected_completion)?;
                        }
                    }
                    KeyCode::Tab if show_completions => {
                        let matches = self.get_matching_commands(&buffer);
                        if !matches.is_empty() && selected_completion < matches.len() {
                            buffer = format!("/{}", matches[selected_completion].0);
                            self.clear_completions()?;
                            
                            execute!(stdout, Clear(ClearType::CurrentLine), cursor::MoveToColumn(0))?;
                            execute!(stdout, Print(prompt), Print(&buffer))?;
                            stdout.flush()?;
                        }
                    }
                    KeyCode::Enter => {
                        if show_completions {
                            let matches = self.get_matching_commands(&buffer);
                            if !matches.is_empty() && selected_completion < matches.len() {
                                buffer = format!("/{}", matches[selected_completion].0);
                            }
                            self.clear_completions()?;
                        }
                        
                        execute!(stdout, Print("\n"))?;
                        return Ok(Some(buffer));
                    }
                    KeyCode::Esc if show_completions => {
                        self.clear_completions()?;
                        show_completions = false;
                    }
                    _ => {}
                }
            }
        }
    }

    fn get_matching_commands(&self, input: &str) -> Vec<(&'static str, &'static str)> {
        if !input.starts_with('/') {
            return vec![];
        }

        let prefix = &input[1..];
        self.commands
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(prefix))
            .copied()
            .collect()
    }

    fn display_completions(&mut self, input: &str, selected: usize) -> io::Result<()> {
        let matches = self.get_matching_commands(input);
        if matches.is_empty() {
            return Ok(());
        }

        let mut stdout = io::stdout();
        
        // If this is the first time showing completions, make room by printing newlines
        if self.completion_lines == 0 {
            for _ in 0..matches.len() {
                execute!(stdout, Print("\n"))?;
            }
            // Move cursor back up to where we started
            for _ in 0..matches.len() {
                execute!(stdout, cursor::MoveToPreviousLine(1))?;
            }
        }
        
        self.completion_lines = matches.len();
        self.display_upward = false;
        
        queue!(stdout, cursor::SavePosition)?;
        
        for (i, (cmd, desc)) in matches.iter().enumerate() {
            queue!(stdout, cursor::MoveToNextLine(1), cursor::MoveToColumn(2))?;
            
            if i == selected {
                queue!(
                    stdout,
                    SetForegroundColor(Color::Cyan),
                    Print("▸ "),
                    ResetColor,
                )?;
            } else {
                queue!(stdout, Print("  "))?;
            }
            
            queue!(
                stdout,
                SetForegroundColor(Color::Cyan),
                Print(format!("/{:<10}", cmd)),
                ResetColor,
                Print(" "),
                Print(desc),
                Clear(ClearType::UntilNewLine),
            )?;
        }
        
        queue!(stdout, cursor::RestorePosition)?;
        stdout.flush()?;
        
        Ok(())
    }

    fn clear_completions(&mut self) -> io::Result<()> {
        if self.completion_lines == 0 {
            return Ok(());
        }

        let mut stdout = io::stdout();
        
        queue!(stdout, cursor::SavePosition)?;
        
        for _ in 0..self.completion_lines {
            queue!(
                stdout,
                cursor::MoveToNextLine(1),
                Clear(ClearType::CurrentLine),
            )?;
        }
        
        queue!(stdout, cursor::RestorePosition)?;
        stdout.flush()?;
        
        self.completion_lines = 0;
        
        Ok(())
    }
}
