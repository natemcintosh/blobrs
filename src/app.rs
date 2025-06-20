use crate::event::{AppEvent, Event, EventHandler};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use std::{env, fs};

/// Application.
#[derive(Debug)]
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Event handler.
    pub events: EventHandler,
    /// Current working directory.
    pub current_dir: String,
    /// List of files in the current directory.
    pub files: Vec<String>,
    /// Currently selected file index.
    pub selected_index: usize,
}

impl Default for App {
    fn default() -> Self {
        let current_dir = env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .to_string_lossy()
            .to_string();
        let files = Self::read_directory(&current_dir);

        Self {
            running: true,
            events: EventHandler::new(),
            current_dir,
            files,
            selected_index: 0,
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Run the application's main loop.
    pub fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        while self.running {
            terminal.draw(|frame| frame.render_widget(&self, frame.area()))?;
            self.handle_events()?;
        }
        Ok(())
    }

    pub fn handle_events(&mut self) -> color_eyre::Result<()> {
        match self.events.next()? {
            Event::Tick => self.tick(),
            Event::Crossterm(event) => {
                if let ratatui::crossterm::event::Event::Key(key_event) = event {
                    self.handle_key_event(key_event)?
                }
            }
            Event::App(app_event) => match app_event {
                AppEvent::Quit => self.quit(),
            },
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') => self.events.send(AppEvent::Quit),
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.events.send(AppEvent::Quit)
            }

            KeyCode::Char('r') | KeyCode::F(5) => self.refresh_files(),
            KeyCode::Up | KeyCode::Char('k') => self.move_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_down(),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => self.enter_directory(),
            KeyCode::Left | KeyCode::Char('h') => self.go_up_directory(),
            // Other handlers you could add here.
            _ => {}
        }
        Ok(())
    }

    /// Handles the tick event of the terminal.
    ///
    /// The tick event is where you can update the state of your application with any logic that
    /// needs to be updated at a fixed frame rate. E.g. polling a server, updating an animation.
    pub fn tick(&self) {}

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Read the contents of a directory and return a sorted list of file names.
    fn read_directory(path: &str) -> Vec<String> {
        match fs::read_dir(path) {
            Ok(entries) => {
                let mut files = Vec::new();
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        let prefix = if entry.path().is_dir() {
                            "📁 "
                        } else {
                            "📄 "
                        };
                        files.push(format!("{}{}", prefix, name));
                    }
                }
                files.sort();
                files
            }
            Err(_) => vec!["Failed to read directory".to_string()],
        }
    }

    /// Refresh the file list for the current directory.
    pub fn refresh_files(&mut self) {
        self.files = Self::read_directory(&self.current_dir);
        self.selected_index = 0; // Reset selection to top
    }

    /// Move the selection up.
    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move the selection down.
    pub fn move_down(&mut self) {
        if self.selected_index < self.files.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Enter a directory if the selected item is a folder.
    pub fn enter_directory(&mut self) {
        if self.files.is_empty() {
            return;
        }

        let selected_file = &self.files[self.selected_index];
        // Check if the selected item is a directory (starts with folder emoji)
        if let Some(dir_name) = selected_file.strip_prefix("📁 ") {
            let new_path = if self.current_dir.ends_with('/') {
                format!("{}{}", self.current_dir, dir_name)
            } else {
                format!("{}/{}", self.current_dir, dir_name)
            };

            // Try to change to the new directory
            if let Ok(canonical_path) = fs::canonicalize(&new_path) {
                if let Some(path_str) = canonical_path.to_str() {
                    self.current_dir = path_str.to_string();
                    self.files = Self::read_directory(&self.current_dir);
                    self.selected_index = 0; // Reset selection to top
                }
            }
        }
    }

    /// Go up one directory level.
    pub fn go_up_directory(&mut self) {
        let current_path = std::path::Path::new(&self.current_dir);
        if let Some(parent) = current_path.parent() {
            if let Some(parent_str) = parent.to_str() {
                self.current_dir = parent_str.to_string();
                self.files = Self::read_directory(&self.current_dir);
                self.selected_index = 0; // Reset selection to top
            }
        }
    }
}
