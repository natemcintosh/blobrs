use crate::event::{AppEvent, Event, EventHandler};
use object_store::{ObjectStore, path::Path as ObjectPath};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use std::sync::Arc;

/// Application.
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Event handler.
    pub events: EventHandler,
    /// Azure Blob Storage client.
    pub object_store: Arc<dyn ObjectStore>,
    /// Current path prefix in blob storage.
    pub current_path: String,
    /// List of blobs/prefixes in the current path.
    pub files: Vec<String>,
    /// All files (unfiltered) for search functionality.
    pub all_files: Vec<String>,
    /// Currently selected file index.
    pub selected_index: usize,
    /// Loading state for async operations.
    pub is_loading: bool,
    /// Error message to display.
    pub error_message: Option<String>,
    /// Search mode state.
    pub search_mode: bool,
    /// Current search query.
    pub search_query: String,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("running", &self.running)
            .field("current_path", &self.current_path)
            .field("files", &self.files)
            .field("all_files", &self.all_files)
            .field("selected_index", &self.selected_index)
            .field("is_loading", &self.is_loading)
            .field("error_message", &self.error_message)
            .field("search_mode", &self.search_mode)
            .field("search_query", &self.search_query)
            .finish()
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub async fn new(object_store: Arc<dyn ObjectStore>) -> color_eyre::Result<Self> {
        let mut app = Self {
            running: true,
            events: EventHandler::new(),
            object_store,
            current_path: String::new(),
            files: Vec::new(),
            all_files: Vec::new(),
            selected_index: 0,
            is_loading: false,
            error_message: None,
            search_mode: false,
            search_query: String::new(),
        };

        // Load initial file list
        app.refresh_files().await?;
        Ok(app)
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        while self.running {
            terminal.draw(|frame| frame.render_widget(&self, frame.area()))?;
            self.handle_events().await?;
        }
        Ok(())
    }

    pub async fn handle_events(&mut self) -> color_eyre::Result<()> {
        match self.events.next()? {
            Event::Tick => self.tick(),
            Event::Crossterm(event) => {
                if let ratatui::crossterm::event::Event::Key(key_event) = event {
                    self.handle_key_event(key_event).await?
                }
            }
            Event::App(app_event) => match app_event {
                AppEvent::Quit => self.quit(),
            },
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    pub async fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        // Handle search mode separately
        if self.search_mode {
            return self.handle_search_key_event(key_event).await;
        }

        // Don't process keys while loading
        if self.is_loading {
            return Ok(());
        }

        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') => self.events.send(AppEvent::Quit),
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.events.send(AppEvent::Quit)
            }

            KeyCode::Char('/') => {
                self.enter_search_mode();
            }

            KeyCode::Char('r') | KeyCode::F(5) => {
                if let Err(e) = self.refresh_files().await {
                    self.error_message = Some(format!("Refresh failed: {}", e));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_down(),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                if let Err(e) = self.enter_directory().await {
                    self.error_message = Some(format!("Enter directory failed: {}", e));
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if let Err(e) = self.go_up_directory().await {
                    self.error_message = Some(format!("Go up failed: {}", e));
                }
            }
            // Clear error message on any other key
            _ => {
                self.error_message = None;
            }
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

    /// List blobs and prefixes in the current path.
    async fn list_blobs(&self, prefix: &str) -> color_eyre::Result<Vec<String>> {
        let result = if prefix.is_empty() {
            self.object_store.list_with_delimiter(None).await?
        } else {
            let object_path = ObjectPath::from(prefix);
            self.object_store
                .list_with_delimiter(Some(&object_path))
                .await?
        };
        let mut items = Vec::new();

        // Add "directories" (common prefixes)
        for prefix in result.common_prefixes {
            let name = prefix.as_ref().trim_end_matches('/');
            if let Some(last_part) = name.split('/').next_back() {
                items.push(format!("📁 {}", last_part));
            }
        }

        // Add files (objects)
        for meta in result.objects {
            let name = meta.location.as_ref();
            if let Some(last_part) = name.split('/').next_back() {
                items.push(format!("📄 {}", last_part));
            }
        }

        items.sort();
        Ok(items)
    }

    /// Refresh the file list for the current path.
    pub async fn refresh_files(&mut self) -> color_eyre::Result<()> {
        self.is_loading = true;
        self.error_message = None;

        match self.list_blobs(&self.current_path).await {
            Ok(files) => {
                self.all_files = files.clone();
                if self.search_mode && !self.search_query.is_empty() {
                    self.filter_files();
                } else {
                    self.files = files;
                    self.selected_index = 0;
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to list blobs: {}", e));
            }
        }

        self.is_loading = false;
        Ok(())
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
    pub async fn enter_directory(&mut self) -> color_eyre::Result<()> {
        if self.files.is_empty() {
            return Ok(());
        }

        let selected_file = &self.files[self.selected_index];
        // Check if the selected item is a directory (starts with folder emoji)
        if let Some(dir_name) = selected_file.strip_prefix("📁 ") {
            let new_path = if self.current_path.is_empty() {
                format!("{}/", dir_name)
            } else if self.current_path.ends_with('/') {
                format!("{}{}/", self.current_path, dir_name)
            } else {
                format!("{}/{}/", self.current_path, dir_name)
            };

            self.current_path = new_path;
            // Exit search mode when navigating
            if self.search_mode {
                self.search_mode = false;
                self.search_query.clear();
            }
            self.refresh_files().await?;
        }
        Ok(())
    }

    /// Go up one directory level.
    pub async fn go_up_directory(&mut self) -> color_eyre::Result<()> {
        if self.current_path.is_empty() {
            return Ok(()); // Already at root
        }

        // Remove trailing slash and go up one level
        let trimmed = self.current_path.trim_end_matches('/');
        if let Some(last_slash) = trimmed.rfind('/') {
            self.current_path = format!("{}/", &trimmed[..last_slash]);
        } else {
            self.current_path = String::new(); // Go to root
        }

        // Exit search mode when navigating
        if self.search_mode {
            self.search_mode = false;
            self.search_query.clear();
        }
        self.refresh_files().await?;
        Ok(())
    }

    /// Enter search mode.
    pub fn enter_search_mode(&mut self) {
        self.search_mode = true;
        self.search_query.clear();
        self.error_message = None;
    }

    /// Exit search mode and restore original file list.
    pub fn exit_search_mode(&mut self) {
        self.search_mode = false;
        self.search_query.clear();
        self.files = self.all_files.clone();
        self.selected_index = 0;
    }

    /// Handle key events when in search mode.
    pub async fn handle_search_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        match key_event.code {
            KeyCode::Esc => {
                self.exit_search_mode();
            }
            KeyCode::Enter => {
                // Exit search mode but keep the filtered results
                self.search_mode = false;
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.filter_files();
            }
            KeyCode::Up if key_event.modifiers == KeyModifiers::CONTROL => {
                self.move_up();
            }
            KeyCode::Down if key_event.modifiers == KeyModifiers::CONTROL => {
                self.move_down();
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.filter_files();
            }
            _ => {}
        }
        Ok(())
    }

    /// Filter files based on search query.
    pub fn filter_files(&mut self) {
        if self.search_query.is_empty() {
            self.files = self.all_files.clone();
        } else {
            self.files = self.all_files
                .iter()
                .filter(|file| {
                    file.to_lowercase().contains(&self.search_query.to_lowercase())
                })
                .cloned()
                .collect();
        }
        self.selected_index = 0;
    }
}
