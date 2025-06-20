use crate::{
    event::{AppEvent, Event, EventHandler},
    terminal_icons::{IconSet, detect_terminal_icons},
};
use base64::{Engine as _, engine::general_purpose};
use chrono::Utc;
use futures::stream::StreamExt;
use hmac::{Hmac, Mac};
use object_store::{ObjectStore, azure::MicrosoftAzureBuilder, path::Path as ObjectPath};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use regex::Regex;
use reqwest;
use sha2::Sha256;
use std::sync::Arc; // Add this import for collect() on streams

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    ContainerSelection,
    BlobBrowsing,
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct BlobInfo {
    pub name: String,
    pub size: Option<u64>, // Changed from usize to u64
    pub last_modified: Option<String>,
    pub etag: Option<String>,
    pub is_folder: bool,
    pub blob_count: Option<usize>, // For folders: number of blobs
    pub total_size: Option<u64>, // For folders: total size of all blobs (changed from usize to u64)
}

/// Application.
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Event handler.
    pub events: EventHandler,
    /// Current application state.
    pub state: AppState,
    /// Azure Storage Account name.
    pub storage_account: String,
    /// Azure Storage Access Key.
    pub access_key: String,
    /// List of available containers.
    pub containers: Vec<ContainerInfo>,
    /// All containers (unfiltered) for search functionality.
    pub all_containers: Vec<ContainerInfo>,
    /// Currently selected container index.
    pub selected_container_index: usize,
    /// Azure Blob Storage client (only available after container selection).
    pub object_store: Option<Arc<dyn ObjectStore>>,
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
    /// Search mode state for containers.
    pub container_search_mode: bool,
    /// Current container search query.
    pub container_search_query: String,
    /// Search mode state.
    pub search_mode: bool,
    /// Current search query.
    pub search_query: String,
    /// Icon set based on terminal capabilities.
    pub icons: IconSet,
    /// Current blob information being displayed.
    pub current_blob_info: Option<BlobInfo>,
    /// Whether to show the blob info popup.
    pub show_blob_info_popup: bool,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("running", &self.running)
            .field("state", &self.state)
            .field("storage_account", &self.storage_account)
            .field("containers", &self.containers)
            .field("all_containers", &self.all_containers)
            .field("selected_container_index", &self.selected_container_index)
            .field("current_path", &self.current_path)
            .field("files", &self.files)
            .field("all_files", &self.all_files)
            .field("selected_index", &self.selected_index)
            .field("is_loading", &self.is_loading)
            .field("error_message", &self.error_message)
            .field("container_search_mode", &self.container_search_mode)
            .field("container_search_query", &self.container_search_query)
            .field("search_mode", &self.search_mode)
            .field("search_query", &self.search_query)
            .field("icons", &self.icons)
            .field("current_blob_info", &self.current_blob_info)
            .field("show_blob_info_popup", &self.show_blob_info_popup)
            .finish()
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub async fn new(storage_account: String, access_key: String) -> color_eyre::Result<Self> {
        let mut app = Self {
            running: true,
            events: EventHandler::new(),
            state: AppState::ContainerSelection,
            storage_account,
            access_key,
            containers: Vec::new(),
            all_containers: Vec::new(),
            selected_container_index: 0,
            object_store: None,
            current_path: String::new(),
            files: Vec::new(),
            all_files: Vec::new(),
            selected_index: 0,
            is_loading: false,
            error_message: None,
            container_search_mode: false,
            container_search_query: String::new(),
            search_mode: false,
            search_query: String::new(),
            icons: detect_terminal_icons(),
            current_blob_info: None,
            show_blob_info_popup: false,
        };

        // Load container list
        app.load_containers().await?;
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
        if self.container_search_mode && self.state == AppState::ContainerSelection {
            return self.handle_container_search_key_event(key_event).await;
        }

        if self.search_mode && self.state == AppState::BlobBrowsing {
            return self.handle_search_key_event(key_event).await;
        }

        // Don't process keys while loading
        if self.is_loading {
            return Ok(());
        }

        // Global keys
        match key_event.code {
            KeyCode::Char('q') => {
                self.events.send(AppEvent::Quit);
                return Ok(());
            }
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.events.send(AppEvent::Quit);
                return Ok(());
            }
            _ => {}
        }

        // State-specific key handling
        match self.state {
            AppState::ContainerSelection => match key_event.code {
                KeyCode::Esc => {
                    // Only quit at the top level (container selection)
                    self.events.send(AppEvent::Quit);
                    return Ok(());
                }
                KeyCode::Char('/') => {
                    self.enter_container_search_mode();
                }
                KeyCode::Up | KeyCode::Char('k') => self.move_container_up(),
                KeyCode::Down | KeyCode::Char('j') => self.move_container_down(),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    if let Err(e) = self.select_container().await {
                        self.error_message = Some(format!("Failed to select container: {}", e));
                    }
                }
                KeyCode::Char('r') | KeyCode::F(5) => {
                    if let Err(e) = self.load_containers().await {
                        self.error_message = Some(format!("Refresh failed: {}", e));
                    }
                }
                _ => {
                    self.error_message = None;
                }
            },
            AppState::BlobBrowsing => {
                match key_event.code {
                    KeyCode::Char('/') => {
                        if !self.show_blob_info_popup {
                            self.enter_search_mode();
                        }
                    }
                    KeyCode::Char('r') | KeyCode::F(5) => {
                        if !self.show_blob_info_popup {
                            if let Err(e) = self.refresh_files().await {
                                self.error_message = Some(format!("Refresh failed: {}", e));
                            }
                        }
                    }
                    KeyCode::Char('i') => {
                        if !self.show_blob_info_popup {
                            if let Err(e) = self.show_blob_info().await {
                                self.error_message =
                                    Some(format!("Failed to get blob info: {}", e));
                            }
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if !self.show_blob_info_popup {
                            self.move_up();
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !self.show_blob_info_popup {
                            self.move_down();
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                        if !self.show_blob_info_popup {
                            if let Err(e) = self.enter_directory().await {
                                self.error_message = Some(format!("Enter directory failed: {}", e));
                            }
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        if self.show_blob_info_popup {
                            // Close popup
                            self.show_blob_info_popup = false;
                            self.current_blob_info = None;
                        } else if let Err(e) = self.go_up_directory().await {
                            self.error_message = Some(format!("Go up failed: {}", e));
                        }
                    }
                    KeyCode::Esc => {
                        if self.show_blob_info_popup {
                            // Close popup
                            self.show_blob_info_popup = false;
                            self.current_blob_info = None;
                        } else if !self.current_path.is_empty() {
                            // Go up one directory level if not at container root
                            if let Err(e) = self.go_up_directory().await {
                                self.error_message = Some(format!("Go up failed: {}", e));
                            }
                        } else {
                            // At container root, go back to container selection
                            self.state = AppState::ContainerSelection;
                            self.object_store = None;
                            self.current_path.clear();
                            self.files.clear();
                            self.all_files.clear();
                            self.selected_index = 0;
                            self.search_mode = false;
                            self.search_query.clear();
                            self.container_search_mode = false;
                            self.container_search_query.clear();
                        }
                    }
                    KeyCode::Backspace => {
                        if self.show_blob_info_popup {
                            // Close popup
                            self.show_blob_info_popup = false;
                            self.current_blob_info = None;
                        } else {
                            // Go back to container selection
                            self.state = AppState::ContainerSelection;
                            self.object_store = None;
                            self.current_path.clear();
                            self.files.clear();
                            self.all_files.clear();
                            self.selected_index = 0;
                            self.search_mode = false;
                            self.search_query.clear();
                            self.container_search_mode = false;
                            self.container_search_query.clear();
                        }
                    }
                    _ => {
                        self.error_message = None;
                    }
                }
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
        let object_store = self
            .object_store
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;

        let result = if prefix.is_empty() {
            object_store.list_with_delimiter(None).await?
        } else {
            let object_path = ObjectPath::from(prefix);
            object_store.list_with_delimiter(Some(&object_path)).await?
        };
        let mut items = Vec::new();

        // Add "directories" (common prefixes)
        for prefix in result.common_prefixes {
            let name = prefix.as_ref().trim_end_matches('/');
            if let Some(last_part) = name.split('/').next_back() {
                items.push(format!("{} {}", self.icons.folder, last_part));
            }
        }

        // Add files (objects)
        for meta in result.objects {
            let name = meta.location.as_ref();
            if let Some(last_part) = name.split('/').next_back() {
                items.push(format!("{} {}", self.icons.file, last_part));
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
        // Check if the selected item is a directory (starts with folder icon)
        let folder_prefix = format!("{} ", self.icons.folder);
        if let Some(dir_name) = selected_file.strip_prefix(&folder_prefix) {
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
            self.files = self
                .all_files
                .iter()
                .filter(|file| {
                    file.to_lowercase()
                        .contains(&self.search_query.to_lowercase())
                })
                .cloned()
                .collect();
        }
        self.selected_index = 0;
    }

    /// Load the list of containers from Azure Storage.
    async fn load_containers(&mut self) -> color_eyre::Result<()> {
        self.is_loading = true;
        self.error_message = None;

        match self.list_containers().await {
            Ok(containers) => {
                self.all_containers = containers.clone();
                if self.container_search_mode && !self.container_search_query.is_empty() {
                    self.filter_containers();
                } else {
                    self.containers = containers;
                    self.selected_container_index = 0;
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to list containers: {}", e));
            }
        }

        self.is_loading = false;
        Ok(())
    }

    /// List all containers in the storage account.
    async fn list_containers(&mut self) -> Result<Vec<ContainerInfo>, String> {
        let account_name = &self.storage_account;
        let access_key = &self.access_key;

        // Decode the base64 access key
        let key = general_purpose::STANDARD
            .decode(access_key)
            .map_err(|e| format!("Failed to decode access key: {}", e))?;

        // Create the request URL
        let url = format!("https://{account_name}.blob.core.windows.net/?comp=list");

        // Create timestamp in RFC 1123 format
        let now = Utc::now();
        let date = now.format("%a, %d %b %Y %H:%M:%S GMT").to_string();

        // Construct the string to sign for Azure Storage API
        // Format: VERB + "\n" + Content-Encoding + "\n" + Content-Language + "\n" + Content-Length + "\n" +
        //         Content-MD5 + "\n" + Content-Type + "\n" + Date + "\n" + If-Modified-Since + "\n" +
        //         If-Match + "\n" + If-None-Match + "\n" + If-Unmodified-Since + "\n" + Range + "\n" +
        //         CanonicalizedHeaders + CanonicalizedResource
        let string_to_sign = format!(
            "GET\n\n\n\n\n\n\n\n\n\n\n\nx-ms-date:{}\nx-ms-version:2020-08-04\n/{}/\ncomp:list",
            date, account_name
        );

        // Generate HMAC-SHA256 signature
        let mut mac = Hmac::<Sha256>::new_from_slice(&key)
            .map_err(|e| format!("Failed to create HMAC: {}", e))?;
        mac.update(string_to_sign.as_bytes());
        let signature = general_purpose::STANDARD.encode(mac.finalize().into_bytes());

        // Create authorization header
        let authorization = format!("SharedKey {}:{}", account_name, signature);

        // Debug: Log the request details
        let debug_request = format!(
            "Request URL: {}\nAuthorization: {}\nDate: {}\nString to sign:\n{}",
            url, authorization, date, string_to_sign
        );
        std::fs::write("debug_request.log", debug_request).ok();

        // Make the HTTP request
        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("x-ms-date", &date)
            .header("x-ms-version", "2020-08-04")
            .header("Authorization", &authorization)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        if !status.is_success() {
            return Err(format!(
                "HTTP {} {} - {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or(""),
                response_text
            ));
        }

        // Debug: Write the XML response to a log file for troubleshooting
        std::fs::write("debug_azure_response.xml", &response_text).ok();

        // Parse XML response
        let containers = self
            .parse_containers_xml(&response_text)
            .map_err(|e| format!("Failed to parse XML response: {}", e))?;

        // Debug: Write container info to log file
        let debug_info = format!("Parsed {} containers:\n{:#?}", containers.len(), containers);
        std::fs::write("debug_containers.log", debug_info).ok();

        Ok(containers)
    }

    /// Parse XML response from Azure Storage list containers API.
    fn parse_containers_xml(&self, xml: &str) -> color_eyre::Result<Vec<ContainerInfo>> {
        let mut containers = Vec::new();

        // Use regex to find all container names
        // Pattern: <Container>...<Name>container_name</Name>...</Container>
        let container_regex = Regex::new(r"<Container>.*?<Name>(.*?)</Name>.*?</Container>")?;

        for cap in container_regex.captures_iter(xml) {
            if let Some(name_match) = cap.get(1) {
                let name = name_match.as_str().to_string();
                if !name.is_empty() {
                    containers.push(ContainerInfo { name });
                }
            }
        }

        Ok(containers)
    }

    /// Select a container and initialize the object store.
    async fn select_container(&mut self) -> color_eyre::Result<()> {
        if self.containers.is_empty() {
            return Ok(());
        }

        let selected_container = &self.containers[self.selected_container_index];

        let azure_client = MicrosoftAzureBuilder::new()
            .with_account(&self.storage_account)
            .with_container_name(&selected_container.name)
            .with_access_key(&self.access_key)
            .build()?;

        self.object_store = Some(Arc::new(azure_client));
        self.state = AppState::BlobBrowsing;

        // Load initial file list
        self.refresh_files().await?;
        Ok(())
    }

    /// Move container selection up.
    fn move_container_up(&mut self) {
        if !self.containers.is_empty() && self.selected_container_index > 0 {
            self.selected_container_index -= 1;
        }
    }

    /// Move container selection down.
    fn move_container_down(&mut self) {
        if !self.containers.is_empty() && self.selected_container_index < self.containers.len() - 1
        {
            self.selected_container_index += 1;
        }
    }

    /// Enter container search mode.
    pub fn enter_container_search_mode(&mut self) {
        self.container_search_mode = true;
        self.container_search_query.clear();
        self.error_message = None;
    }

    /// Exit container search mode and restore original container list.
    pub fn exit_container_search_mode(&mut self) {
        self.container_search_mode = false;
        self.container_search_query.clear();
        self.containers = self.all_containers.clone();
        self.selected_container_index = 0;
    }

    /// Handle key events when in container search mode.
    pub async fn handle_container_search_key_event(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<()> {
        match key_event.code {
            KeyCode::Esc => {
                self.exit_container_search_mode();
            }
            KeyCode::Enter => {
                // Exit search mode but keep the filtered results
                self.container_search_mode = false;
            }
            KeyCode::Backspace => {
                self.container_search_query.pop();
                self.filter_containers();
            }
            KeyCode::Up if key_event.modifiers == KeyModifiers::CONTROL => {
                self.move_container_up();
            }
            KeyCode::Down if key_event.modifiers == KeyModifiers::CONTROL => {
                self.move_container_down();
            }
            KeyCode::Char(c) => {
                self.container_search_query.push(c);
                self.filter_containers();
            }
            _ => {}
        }
        Ok(())
    }

    /// Filter containers based on search query.
    pub fn filter_containers(&mut self) {
        if self.container_search_query.is_empty() {
            self.containers = self.all_containers.clone();
        } else {
            self.containers = self
                .all_containers
                .iter()
                .filter(|container| {
                    container
                        .name
                        .to_lowercase()
                        .contains(&self.container_search_query.to_lowercase())
                })
                .cloned()
                .collect();
        }
        self.selected_container_index = 0;
    }

    /// Show information about the currently selected blob or folder.
    pub async fn show_blob_info(&mut self) -> color_eyre::Result<()> {
        if self.files.is_empty() {
            return Ok(());
        }

        let selected_file = &self.files[self.selected_index];
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);

        let is_folder = selected_file.starts_with(&folder_prefix);
        let name = if is_folder {
            selected_file
                .strip_prefix(&folder_prefix)
                .unwrap_or(selected_file)
        } else {
            selected_file
                .strip_prefix(&file_prefix)
                .unwrap_or(selected_file)
        };

        if is_folder {
            // Get folder information (blob count and total size)
            self.current_blob_info = Some(self.get_folder_info(name).await?);
        } else {
            // Get individual blob information
            self.current_blob_info = Some(self.get_blob_info(name).await?);
        }

        self.show_blob_info_popup = true;
        Ok(())
    }

    /// Get information about a folder (blob count and total size).
    async fn get_folder_info(&self, folder_name: &str) -> color_eyre::Result<BlobInfo> {
        let object_store = self
            .object_store
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;

        let folder_path = if self.current_path.is_empty() {
            format!("{}/", folder_name)
        } else if self.current_path.ends_with('/') {
            format!("{}{}/", self.current_path, folder_name)
        } else {
            format!("{}/{}/", self.current_path, folder_name)
        };

        let object_path = ObjectPath::from(folder_path.as_str());

        // List all objects in this folder (recursively)
        let mut blob_count = 0;
        let mut total_size: u64 = 0; // Explicitly type as u64

        let stream = object_store.list(Some(&object_path));
        let objects: Vec<_> = stream.collect().await;

        for result in objects {
            match result {
                Ok(meta) => {
                    blob_count += 1;
                    total_size += meta.size;
                }
                Err(_) => continue, // Skip errors and continue counting
            }
        }

        Ok(BlobInfo {
            name: folder_name.to_string(),
            size: None,
            last_modified: None,
            etag: None,
            is_folder: true,
            blob_count: Some(blob_count),
            total_size: Some(total_size),
        })
    }

    /// Get information about a specific blob.
    async fn get_blob_info(&self, blob_name: &str) -> color_eyre::Result<BlobInfo> {
        let object_store = self
            .object_store
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;

        let blob_path = if self.current_path.is_empty() {
            blob_name.to_string()
        } else if self.current_path.ends_with('/') {
            format!("{}{}", self.current_path, blob_name)
        } else {
            format!("{}/{}", self.current_path, blob_name)
        };

        let object_path = ObjectPath::from(blob_path.as_str());

        match object_store.head(&object_path).await {
            Ok(meta) => Ok(BlobInfo {
                name: blob_name.to_string(),
                size: Some(meta.size),
                last_modified: Some(
                    meta.last_modified
                        .format("%Y-%m-%d %H:%M:%S UTC")
                        .to_string(),
                ),
                etag: meta.e_tag.clone(),
                is_folder: false,
                blob_count: None,
                total_size: None,
            }),
            Err(e) => Err(color_eyre::eyre::eyre!(
                "Failed to get blob metadata: {}",
                e
            )),
        }
    }
}
