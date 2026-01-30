use crate::{
    event::{AppEvent, Event, EventHandler},
    preview::{MAX_PREVIEW_BYTES, PreviewData, PreviewFileType, parse_preview},
    terminal_icons::{IconSet, detect_terminal_icons},
};
use arboard::Clipboard;
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
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum SortCriteria {
    Name,
    DateModified,
    DateCreated,
    Size,
}

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

#[derive(Debug, Clone)]
pub struct FileItem {
    pub display_name: String, // What to show in the UI (with icon)
    pub actual_name: String,  // The actual file/folder name
    pub is_folder: bool,
    pub size: Option<u64>,
    pub last_modified: Option<chrono::DateTime<chrono::Utc>>,
    pub created: Option<chrono::DateTime<chrono::Utc>>,
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
    /// List of file items with metadata for sorting.
    pub file_items: Vec<FileItem>,
    /// All files (unfiltered) for search functionality.
    pub all_files: Vec<String>,
    /// All file items (unfiltered) for search functionality.
    pub all_file_items: Vec<FileItem>,
    /// Currently selected file index.
    pub selected_index: usize,
    /// Loading state for async operations.
    pub is_loading: bool,
    /// Error message to display.
    pub error_message: Option<String>,
    /// Success message to display.
    pub success_message: Option<String>,
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
    /// Whether to show the download destination picker popup.
    pub show_download_picker: bool,
    /// Current download destination path.
    pub download_destination: Option<PathBuf>,
    /// Download progress information.
    pub download_progress: Option<DownloadProgress>,
    /// Whether a download is currently in progress.
    pub is_downloading: bool,
    /// Current sort criteria for blobs.
    pub sort_criteria: SortCriteria,
    /// Whether to show the sort selection popup.
    pub show_sort_popup: bool,
    /// Whether to show the clone dialog.
    pub show_clone_dialog: bool,
    /// Clone dialog input (new path).
    pub clone_input: String,
    /// Original path being cloned (to compare against).
    pub clone_original_path: String,
    /// Whether the item being cloned is a folder.
    pub clone_is_folder: bool,
    /// Whether a clone operation is in progress.
    pub is_cloning: bool,
    /// Clone progress information.
    pub clone_progress: Option<CloneProgress>,
    /// Whether to show the delete confirmation dialog.
    pub show_delete_dialog: bool,
    /// Delete dialog input (user must type the name to confirm).
    pub delete_input: String,
    /// Target path being deleted.
    pub delete_target_path: String,
    /// Display name of item being deleted (for confirmation).
    pub delete_target_name: String,
    /// Whether the item being deleted is a folder.
    pub delete_is_folder: bool,
    /// Whether a delete operation is in progress.
    pub is_deleting: bool,
    /// Delete progress information.
    pub delete_progress: Option<DeleteProgress>,
    /// Whether to show the preview panel.
    pub show_preview: bool,
    /// Preview data for the current file.
    pub preview_data: Option<PreviewData>,
    /// Preview file type.
    pub preview_file_type: Option<PreviewFileType>,
    /// Preview scroll offset (row, column).
    pub preview_scroll: (usize, usize),
    /// Whether preview is loading.
    pub is_loading_preview: bool,
    /// Preview error message.
    pub preview_error: Option<String>,
    /// Selected row in table preview.
    pub preview_selected_row: usize,
}

#[derive(Debug, Clone)]
pub struct DeleteProgress {
    pub current_file: String,
    pub files_completed: usize,
    pub total_files: usize,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CloneProgress {
    pub current_file: String,
    pub files_completed: usize,
    pub total_files: usize,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub current_file: String,
    pub files_completed: usize,
    pub total_files: usize,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub error_message: Option<String>,
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
            .field("file_items", &self.file_items)
            .field("all_files", &self.all_files)
            .field("all_file_items", &self.all_file_items)
            .field("selected_index", &self.selected_index)
            .field("is_loading", &self.is_loading)
            .field("error_message", &self.error_message)
            .field("success_message", &self.success_message)
            .field("container_search_mode", &self.container_search_mode)
            .field("container_search_query", &self.container_search_query)
            .field("search_mode", &self.search_mode)
            .field("search_query", &self.search_query)
            .field("icons", &self.icons)
            .field("current_blob_info", &self.current_blob_info)
            .field("show_blob_info_popup", &self.show_blob_info_popup)
            .field("show_download_picker", &self.show_download_picker)
            .field("download_destination", &self.download_destination)
            .field("download_progress", &self.download_progress)
            .field("is_downloading", &self.is_downloading)
            .field("sort_criteria", &self.sort_criteria)
            .field("show_sort_popup", &self.show_sort_popup)
            .field("show_clone_dialog", &self.show_clone_dialog)
            .field("clone_input", &self.clone_input)
            .field("clone_original_path", &self.clone_original_path)
            .field("clone_is_folder", &self.clone_is_folder)
            .field("is_cloning", &self.is_cloning)
            .field("clone_progress", &self.clone_progress)
            .field("show_delete_dialog", &self.show_delete_dialog)
            .field("delete_input", &self.delete_input)
            .field("delete_target_path", &self.delete_target_path)
            .field("delete_target_name", &self.delete_target_name)
            .field("delete_is_folder", &self.delete_is_folder)
            .field("is_deleting", &self.is_deleting)
            .field("delete_progress", &self.delete_progress)
            .field("show_preview", &self.show_preview)
            .field("preview_file_type", &self.preview_file_type)
            .field("preview_scroll", &self.preview_scroll)
            .field("is_loading_preview", &self.is_loading_preview)
            .field("preview_error", &self.preview_error)
            .field("preview_selected_row", &self.preview_selected_row)
            .finish()
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    ///
    /// # Errors
    ///
    /// Returns an error if loading containers from Azure Storage fails.
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
            file_items: Vec::new(),
            all_files: Vec::new(),
            all_file_items: Vec::new(),
            selected_index: 0,
            is_loading: false,
            error_message: None,
            success_message: None,
            container_search_mode: false,
            container_search_query: String::new(),
            search_mode: false,
            search_query: String::new(),
            icons: detect_terminal_icons(),
            current_blob_info: None,
            show_blob_info_popup: false,
            show_download_picker: false,
            download_destination: None,
            download_progress: None,
            is_downloading: false,
            sort_criteria: SortCriteria::Name,
            show_sort_popup: false,
            show_clone_dialog: false,
            clone_input: String::new(),
            clone_original_path: String::new(),
            clone_is_folder: false,
            is_cloning: false,
            clone_progress: None,
            show_delete_dialog: false,
            delete_input: String::new(),
            delete_target_path: String::new(),
            delete_target_name: String::new(),
            delete_is_folder: false,
            is_deleting: false,
            delete_progress: None,
            show_preview: false,
            preview_data: None,
            preview_file_type: None,
            preview_scroll: (0, 0),
            is_loading_preview: false,
            preview_error: None,
            preview_selected_row: 0,
        };

        // Load container list
        app.load_containers().await?;
        Ok(app)
    }

    /// Run the application's main loop.
    ///
    /// # Errors
    ///
    /// Returns an error if terminal drawing or event handling fails.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        while self.running {
            terminal.draw(|frame| frame.render_widget(&self, frame.area()))?;
            self.handle_events().await?;
        }
        Ok(())
    }

    /// Handle incoming events from the terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if event reception or key handling fails.
    pub async fn handle_events(&mut self) -> color_eyre::Result<()> {
        match self.events.next()? {
            Event::Tick => self.tick(),
            Event::Crossterm(event) => {
                if let ratatui::crossterm::event::Event::Key(key_event) = event {
                    self.handle_key_event(key_event).await?;
                }
            }
            Event::App(app_event) => match app_event {
                AppEvent::Quit => self.quit(),
            },
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    ///
    /// # Errors
    ///
    /// Returns an error if an async operation triggered by a key event fails.
    pub async fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        // Handle delete dialog separately
        if self.show_delete_dialog {
            return self.handle_delete_dialog_key_event(key_event).await;
        }

        // Handle clone dialog separately
        if self.show_clone_dialog {
            return self.handle_clone_dialog_key_event(key_event).await;
        }

        // Handle search mode separately
        if self.container_search_mode && self.state == AppState::ContainerSelection {
            return self.handle_container_search_key_event(key_event).await;
        }

        if self.search_mode && self.state == AppState::BlobBrowsing {
            return self.handle_search_key_event(key_event).await;
        }

        // Don't process keys while loading, cloning, or deleting
        if self.is_loading || self.is_cloning || self.is_deleting {
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
                        self.error_message = Some(format!("Failed to select container: {e}"));
                    }
                }
                KeyCode::Char('r') | KeyCode::F(5) => {
                    if let Err(e) = self.load_containers().await {
                        self.error_message = Some(format!("Refresh failed: {e}"));
                    }
                }
                _ => {
                    self.error_message = None;
                    self.success_message = None;
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
                        if !self.show_blob_info_popup
                            && let Err(e) = self.refresh_files().await
                        {
                            self.error_message = Some(format!("Refresh failed: {e}"));
                        }
                    }
                    KeyCode::Char('i') => {
                        if !self.show_blob_info_popup
                            && !self.show_preview
                            && let Err(e) = self.show_blob_info().await
                        {
                            self.error_message = Some(format!("Failed to get blob info: {e}"));
                        }
                    }
                    KeyCode::Char('p') => {
                        if !self.show_blob_info_popup
                            && !self.show_download_picker
                            && !self.is_downloading
                            && !self.show_sort_popup
                            && !self.show_clone_dialog
                            && !self.is_cloning
                            && !self.show_delete_dialog
                            && !self.is_deleting
                        {
                            if self.show_preview {
                                // Toggle off
                                self.close_preview();
                            } else {
                                // Toggle on - load preview
                                if let Err(e) = self.load_preview().await {
                                    self.error_message = Some(format!("Preview failed: {e}"));
                                }
                            }
                        }
                    }
                    KeyCode::Char('d') => {
                        if !self.show_blob_info_popup
                            && !self.show_download_picker
                            && !self.is_downloading
                            && !self.show_sort_popup
                        {
                            self.show_download_picker().await;
                        }
                    }
                    KeyCode::Char('s') => {
                        if !self.show_blob_info_popup
                            && !self.show_download_picker
                            && !self.is_downloading
                            && !self.show_sort_popup
                        {
                            self.show_sort_popup = true;
                        } else if self.show_sort_popup {
                            // Handle sort selection
                            if let Err(e) = self.apply_sort(SortCriteria::Size).await {
                                self.error_message = Some(format!("Failed to sort: {e}"));
                            }
                            self.show_sort_popup = false;
                        }
                    }
                    KeyCode::Char('n') => {
                        if self.show_sort_popup {
                            if let Err(e) = self.apply_sort(SortCriteria::Name).await {
                                self.error_message = Some(format!("Failed to sort: {e}"));
                            }
                            self.show_sort_popup = false;
                        }
                    }
                    KeyCode::Char('m') => {
                        if self.show_sort_popup {
                            if let Err(e) = self.apply_sort(SortCriteria::DateModified).await {
                                self.error_message = Some(format!("Failed to sort: {e}"));
                            }
                            self.show_sort_popup = false;
                        }
                    }
                    KeyCode::Char('t') => {
                        if self.show_sort_popup {
                            if let Err(e) = self.apply_sort(SortCriteria::DateCreated).await {
                                self.error_message = Some(format!("Failed to sort: {e}"));
                            }
                            self.show_sort_popup = false;
                        }
                    }
                    KeyCode::Char('y') => {
                        if !self.show_blob_info_popup
                            && !self.show_download_picker
                            && !self.is_downloading
                            && !self.show_sort_popup
                            && !self.show_clone_dialog
                            && !self.is_cloning
                        {
                            // Copy blob path to clipboard
                            if let Err(e) = self.copy_blob_path_to_clipboard() {
                                self.error_message =
                                    Some(format!("Failed to copy to clipboard: {e}"));
                            }
                        }
                    }
                    KeyCode::Char('c') => {
                        if !self.show_blob_info_popup
                            && !self.show_download_picker
                            && !self.is_downloading
                            && !self.show_sort_popup
                            && !self.show_clone_dialog
                            && !self.is_cloning
                            && !self.show_delete_dialog
                            && !self.is_deleting
                        {
                            // Open clone dialog
                            self.open_clone_dialog();
                        }
                    }
                    KeyCode::Char('x') | KeyCode::Delete => {
                        if !self.show_blob_info_popup
                            && !self.show_download_picker
                            && !self.is_downloading
                            && !self.show_sort_popup
                            && !self.show_clone_dialog
                            && !self.is_cloning
                            && !self.show_delete_dialog
                            && !self.is_deleting
                        {
                            // Open delete dialog
                            self.open_delete_dialog();
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.show_preview {
                            self.preview_scroll_up();
                        } else if !self.show_blob_info_popup && !self.show_sort_popup {
                            self.move_up();
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if self.show_preview {
                            self.preview_scroll_down();
                        } else if !self.show_blob_info_popup && !self.show_sort_popup {
                            self.move_down();
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                        if self.show_preview {
                            self.preview_scroll_right();
                        } else if self.show_download_picker {
                            if let Err(e) = self.confirm_download().await {
                                self.error_message = Some(format!("Download failed: {e}"));
                            }
                        } else if !self.show_blob_info_popup
                            && !self.show_sort_popup
                            && let Err(e) = self.enter_directory().await
                        {
                            self.error_message = Some(format!("Enter directory failed: {e}"));
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        if self.show_preview {
                            self.preview_scroll_left();
                        } else if self.show_download_picker {
                            // Close download picker
                            self.show_download_picker = false;
                            self.download_destination = None;
                        } else if self.show_blob_info_popup {
                            // Close popup
                            self.show_blob_info_popup = false;
                            self.current_blob_info = None;
                        } else if self.show_sort_popup {
                            // Close sort popup
                            self.show_sort_popup = false;
                        } else if let Err(e) = self.go_up_directory().await {
                            self.error_message = Some(format!("Go up failed: {e}"));
                        }
                    }
                    KeyCode::Esc => {
                        if self.show_preview {
                            // Close preview panel
                            self.close_preview();
                        } else if self.show_download_picker {
                            // Close download picker
                            self.show_download_picker = false;
                            self.download_destination = None;
                        } else if self.show_blob_info_popup {
                            // Close popup
                            self.show_blob_info_popup = false;
                            self.current_blob_info = None;
                        } else if self.show_sort_popup {
                            // Close sort popup
                            self.show_sort_popup = false;
                        } else if !self.current_path.is_empty() {
                            // Go up one directory level if not at container root
                            if let Err(e) = self.go_up_directory().await {
                                self.error_message = Some(format!("Go up failed: {e}"));
                            }
                        } else {
                            // At container root, go back to container selection
                            self.state = AppState::ContainerSelection;
                            self.object_store = None;
                            self.current_path.clear();
                            self.files.clear();
                            self.file_items.clear();
                            self.all_files.clear();
                            self.all_file_items.clear();
                            self.selected_index = 0;
                            self.search_mode = false;
                            self.search_query.clear();
                            self.container_search_mode = false;
                            self.container_search_query.clear();
                        }
                    }
                    KeyCode::Backspace => {
                        if self.show_download_picker {
                            // Close download picker
                            self.show_download_picker = false;
                            self.download_destination = None;
                        } else if self.show_blob_info_popup {
                            // Close popup
                            self.show_blob_info_popup = false;
                            self.current_blob_info = None;
                        } else if self.show_sort_popup {
                            // Close sort popup
                            self.show_sort_popup = false;
                        } else {
                            // Go back to container selection
                            self.state = AppState::ContainerSelection;
                            self.object_store = None;
                            self.current_path.clear();
                            self.files.clear();
                            self.file_items.clear();
                            self.all_files.clear();
                            self.all_file_items.clear();
                            self.selected_index = 0;
                            self.search_mode = false;
                            self.search_query.clear();
                            self.container_search_mode = false;
                            self.container_search_query.clear();
                        }
                    }
                    _ => {
                        self.error_message = None;
                        self.success_message = None;
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

    /// List blobs and prefixes with metadata for sorting.
    async fn list_file_items(&self, prefix: &str) -> color_eyre::Result<Vec<FileItem>> {
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
                items.push(FileItem {
                    display_name: format!("{} {}", self.icons.folder, last_part),
                    actual_name: last_part.to_string(),
                    is_folder: true,
                    size: None,
                    last_modified: None,
                    created: None,
                });
            }
        }

        // Add files (objects) with metadata
        for meta in result.objects {
            let name = meta.location.as_ref();
            if let Some(last_part) = name.split('/').next_back() {
                items.push(FileItem {
                    display_name: format!("{} {}", self.icons.file, last_part),
                    actual_name: last_part.to_string(),
                    is_folder: false,
                    size: Some(meta.size),
                    last_modified: Some(meta.last_modified),
                    created: None, // Azure Blob Storage doesn't provide creation time in list operation
                });
            }
        }

        Ok(items)
    }

    /// Apply sorting to the current file list.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but uses `Result` for API consistency.
    pub async fn apply_sort(&mut self, criteria: SortCriteria) -> color_eyre::Result<()> {
        self.sort_criteria = criteria.clone();

        if !self.file_items.is_empty() {
            Self::sort_file_items_static(&mut self.file_items, &criteria);
            // Update the display list
            self.files = self
                .file_items
                .iter()
                .map(|item| item.display_name.clone())
                .collect();
        }

        // Also sort the unfiltered list
        if !self.all_file_items.is_empty() {
            Self::sort_file_items_static(&mut self.all_file_items, &criteria);
            self.all_files = self
                .all_file_items
                .iter()
                .map(|item| item.display_name.clone())
                .collect();
        }

        Ok(())
    }

    /// Sort file items based on the given criteria.
    fn sort_file_items_static(items: &mut [FileItem], criteria: &SortCriteria) {
        items.sort_by(|a, b| {
            // Always put folders first
            match (a.is_folder, b.is_folder) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    // Both are folders or both are files, sort by criteria
                    match criteria {
                        SortCriteria::Name => a.actual_name.cmp(&b.actual_name),
                        SortCriteria::DateModified => {
                            match (a.last_modified, b.last_modified) {
                                (Some(a_time), Some(b_time)) => b_time.cmp(&a_time), // Newest first
                                (Some(_), None) => std::cmp::Ordering::Less,
                                (None, Some(_)) => std::cmp::Ordering::Greater,
                                (None, None) => a.actual_name.cmp(&b.actual_name), // Fallback to name
                            }
                        }
                        SortCriteria::DateCreated => {
                            // Since Azure Blob Storage doesn't provide creation time in list operations,
                            // fall back to last_modified
                            match (a.last_modified, b.last_modified) {
                                (Some(a_time), Some(b_time)) => b_time.cmp(&a_time), // Newest first
                                (Some(_), None) => std::cmp::Ordering::Less,
                                (None, Some(_)) => std::cmp::Ordering::Greater,
                                (None, None) => a.actual_name.cmp(&b.actual_name), // Fallback to name
                            }
                        }
                        SortCriteria::Size => {
                            match (a.size, b.size) {
                                (Some(a_size), Some(b_size)) => b_size.cmp(&a_size), // Largest first
                                (Some(_), None) => std::cmp::Ordering::Less,
                                (None, Some(_)) => std::cmp::Ordering::Greater,
                                (None, None) => a.actual_name.cmp(&b.actual_name), // Fallback to name
                            }
                        }
                    }
                }
            }
        });
    }

    /// Refresh the file list for the current path.
    ///
    /// # Errors
    ///
    /// Returns an error if listing blobs from Azure Storage fails.
    pub async fn refresh_files(&mut self) -> color_eyre::Result<()> {
        self.is_loading = true;
        self.error_message = None;
        self.success_message = None;

        match self.list_file_items(&self.current_path).await {
            Ok(mut file_items) => {
                // Apply current sorting
                Self::sort_file_items_static(&mut file_items, &self.sort_criteria);

                // Create display strings
                let files: Vec<String> = file_items
                    .iter()
                    .map(|item| item.display_name.clone())
                    .collect();

                self.all_file_items.clone_from(&file_items);
                self.all_files.clone_from(&files);

                if self.search_mode && !self.search_query.is_empty() {
                    self.filter_files();
                } else {
                    self.file_items = file_items;
                    self.files = files;
                    self.selected_index = 0;
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to list blobs: {e}"));
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
    ///
    /// # Errors
    ///
    /// Returns an error if refreshing the file list fails.
    pub async fn enter_directory(&mut self) -> color_eyre::Result<()> {
        if self.files.is_empty() {
            return Ok(());
        }

        let selected_file = &self.files[self.selected_index];
        // Check if the selected item is a directory (starts with folder icon)
        let folder_prefix = format!("{} ", self.icons.folder);
        if let Some(dir_name) = selected_file.strip_prefix(&folder_prefix) {
            let new_path = if self.current_path.is_empty() {
                format!("{dir_name}/")
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
    ///
    /// # Errors
    ///
    /// Returns an error if refreshing the file list fails.
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
        self.success_message = None;
    }

    /// Exit search mode and restore original file list.
    pub fn exit_search_mode(&mut self) {
        self.search_mode = false;
        self.search_query.clear();
        self.files = self.all_files.clone();
        self.selected_index = 0;
    }

    /// Handle key events when in search mode.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but uses `Result` for API consistency.
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

    /// Handle key events when in clone dialog mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the clone operation fails.
    pub async fn handle_clone_dialog_key_event(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<()> {
        match key_event.code {
            KeyCode::Esc => {
                self.close_clone_dialog();
            }
            KeyCode::Enter => {
                // Only allow confirm if name is different from original
                if self.clone_input != self.clone_original_path && !self.clone_input.is_empty() {
                    if let Err(e) = self.execute_clone().await {
                        self.error_message = Some(format!("Clone failed: {e}"));
                    }
                    self.close_clone_dialog();
                }
            }
            KeyCode::Backspace => {
                self.clone_input.pop();
            }
            KeyCode::Char(c) => {
                self.clone_input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    /// Open the clone dialog for the selected item.
    pub fn open_clone_dialog(&mut self) {
        if self.files.is_empty() {
            return;
        }

        let selected_file = &self.files[self.selected_index];
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);

        let (item_name, is_folder) = if selected_file.starts_with(&folder_prefix) {
            let name = selected_file
                .strip_prefix(&folder_prefix)
                .unwrap_or(selected_file);
            (name.to_string(), true)
        } else {
            let name = selected_file
                .strip_prefix(&file_prefix)
                .unwrap_or(selected_file);
            (name.to_string(), false)
        };

        // Construct the full path
        let full_path = if self.current_path.is_empty() {
            if is_folder {
                format!("{item_name}/")
            } else {
                item_name
            }
        } else if self.current_path.ends_with('/') {
            if is_folder {
                format!("{}{}/", self.current_path, item_name)
            } else {
                format!("{}{}", self.current_path, item_name)
            }
        } else if is_folder {
            format!("{}/{}/", self.current_path, item_name)
        } else {
            format!("{}/{}", self.current_path, item_name)
        };

        self.clone_original_path.clone_from(&full_path);
        self.clone_input = full_path;
        self.clone_is_folder = is_folder;
        self.show_clone_dialog = true;
    }

    /// Close the clone dialog.
    pub fn close_clone_dialog(&mut self) {
        self.show_clone_dialog = false;
        self.clone_input.clear();
        self.clone_original_path.clear();
        self.clone_is_folder = false;
    }

    /// Execute the clone operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the clone or subsequent file refresh fails.
    pub async fn execute_clone(&mut self) -> color_eyre::Result<()> {
        let mut new_path = self.clone_input.clone();
        let original_path = self.clone_original_path.clone();
        let is_folder = self.clone_is_folder;

        // Ensure folder paths end with /
        if is_folder && !new_path.ends_with('/') {
            new_path.push('/');
        }

        self.is_cloning = true;
        self.clone_progress = Some(CloneProgress {
            current_file: String::new(),
            files_completed: 0,
            total_files: 0,
            error_message: None,
        });

        let result = if is_folder {
            self.clone_folder(&original_path, &new_path).await
        } else {
            self.clone_blob(&original_path, &new_path).await
        };

        self.is_cloning = false;
        self.clone_progress = None;

        if result.is_ok() {
            let orig = original_path.trim_end_matches('/');
            let new = new_path.trim_end_matches('/');
            self.success_message = Some(format!("Successfully cloned {orig} to {new}"));
            // Refresh the file list
            if let Err(e) = self.refresh_files().await {
                self.error_message = Some(format!("Refresh failed after clone: {e}"));
            }
        }

        result
    }

    /// Clone a single blob.
    async fn clone_blob(&mut self, source: &str, destination: &str) -> color_eyre::Result<()> {
        let object_store = self
            .object_store
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;

        let source_path = ObjectPath::from(source);
        let dest_path = ObjectPath::from(destination);

        // Update progress
        if let Some(progress) = &mut self.clone_progress {
            progress.current_file = source.to_string();
            progress.total_files = 1;
        }

        // Use copy operation (server-side copy)
        object_store.copy(&source_path, &dest_path).await?;

        // Update progress
        if let Some(progress) = &mut self.clone_progress {
            progress.files_completed = 1;
        }

        Ok(())
    }

    /// Clone all blobs in a folder (prefix).
    async fn clone_folder(&mut self, source: &str, destination: &str) -> color_eyre::Result<()> {
        let object_store = self
            .object_store
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;

        let source_path = ObjectPath::from(source);

        // List all files in the source folder
        let stream = object_store.list(Some(&source_path));
        let objects: Vec<_> = stream.collect().await;

        let total_files = objects.len();

        // Update progress
        if let Some(progress) = &mut self.clone_progress {
            progress.total_files = total_files;
        }

        let mut files_completed = 0;

        for result in objects {
            match result {
                Ok(meta) => {
                    let file_path = meta.location.as_ref();

                    // Calculate relative path from source
                    let relative_path = file_path.strip_prefix(source).unwrap_or(file_path);

                    // Construct destination path
                    let dest_file_path = format!("{destination}{relative_path}");

                    // Update progress
                    if let Some(progress) = &mut self.clone_progress {
                        progress.current_file = file_path.to_string();
                    }

                    // Copy the file
                    let dest_object_path = ObjectPath::from(dest_file_path.as_str());
                    if let Err(e) = object_store.copy(&meta.location, &dest_object_path).await {
                        if let Some(progress) = &mut self.clone_progress {
                            progress.error_message =
                                Some(format!("Failed to clone {file_path}: {e}"));
                        }
                        // Continue with other files even if one fails
                    } else {
                        files_completed += 1;

                        // Update progress
                        if let Some(progress) = &mut self.clone_progress {
                            progress.files_completed = files_completed;
                        }
                    }
                }
                Err(e) => {
                    if let Some(progress) = &mut self.clone_progress {
                        progress.error_message = Some(format!("Failed to list file: {e}"));
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle key events when in delete dialog mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the delete operation fails.
    pub async fn handle_delete_dialog_key_event(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<()> {
        match key_event.code {
            KeyCode::Esc => {
                self.close_delete_dialog();
            }
            KeyCode::Enter => {
                // Only allow confirm if the typed name matches the target name
                if self.delete_input == self.delete_target_name {
                    if let Err(e) = self.execute_delete().await {
                        self.error_message = Some(format!("Delete failed: {e}"));
                    }
                    self.close_delete_dialog();
                }
            }
            KeyCode::Backspace => {
                self.delete_input.pop();
            }
            KeyCode::Char(c) => {
                self.delete_input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    /// Open the delete confirmation dialog for the selected item.
    pub fn open_delete_dialog(&mut self) {
        if self.files.is_empty() {
            return;
        }

        let selected_file = &self.files[self.selected_index];
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);

        let (item_name, is_folder) = if selected_file.starts_with(&folder_prefix) {
            let name = selected_file
                .strip_prefix(&folder_prefix)
                .unwrap_or(selected_file);
            (name.to_string(), true)
        } else {
            let name = selected_file
                .strip_prefix(&file_prefix)
                .unwrap_or(selected_file);
            (name.to_string(), false)
        };

        // Construct the full path
        let full_path = if self.current_path.is_empty() {
            if is_folder {
                format!("{item_name}/")
            } else {
                item_name.clone()
            }
        } else if self.current_path.ends_with('/') {
            if is_folder {
                format!("{}{}/", self.current_path, item_name)
            } else {
                format!("{}{}", self.current_path, item_name)
            }
        } else if is_folder {
            format!("{}/{}/", self.current_path, item_name)
        } else {
            format!("{}/{}", self.current_path, item_name)
        };

        self.delete_target_path = full_path;
        self.delete_target_name = item_name;
        self.delete_is_folder = is_folder;
        self.delete_input.clear();
        self.show_delete_dialog = true;
    }

    /// Close the delete dialog.
    pub fn close_delete_dialog(&mut self) {
        self.show_delete_dialog = false;
        self.delete_input.clear();
        self.delete_target_path.clear();
        self.delete_target_name.clear();
        self.delete_is_folder = false;
    }

    /// Execute the delete operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the delete or subsequent file refresh fails.
    pub async fn execute_delete(&mut self) -> color_eyre::Result<()> {
        let target_path = self.delete_target_path.clone();
        let is_folder = self.delete_is_folder;

        self.is_deleting = true;
        self.delete_progress = Some(DeleteProgress {
            current_file: String::new(),
            files_completed: 0,
            total_files: 0,
            error_message: None,
        });

        let result = if is_folder {
            self.delete_folder(&target_path).await
        } else {
            self.delete_blob(&target_path).await
        };

        self.is_deleting = false;
        self.delete_progress = None;

        if result.is_ok() {
            let name = target_path.trim_end_matches('/');
            self.success_message = Some(format!("Successfully deleted {name}"));
            // Refresh the file list
            if let Err(e) = self.refresh_files().await {
                self.error_message = Some(format!("Refresh failed after delete: {e}"));
            }
        }

        result
    }

    /// Delete a single blob.
    async fn delete_blob(&mut self, path: &str) -> color_eyre::Result<()> {
        let object_store = self
            .object_store
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;

        let object_path = ObjectPath::from(path);

        // Update progress
        if let Some(progress) = &mut self.delete_progress {
            progress.current_file = path.to_string();
            progress.total_files = 1;
        }

        object_store.delete(&object_path).await?;

        // Update progress
        if let Some(progress) = &mut self.delete_progress {
            progress.files_completed = 1;
        }

        Ok(())
    }

    /// Delete all blobs in a folder (prefix).
    async fn delete_folder(&mut self, prefix: &str) -> color_eyre::Result<()> {
        let object_store = self
            .object_store
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;

        let prefix_path = ObjectPath::from(prefix);

        // List all files in the folder
        let stream = object_store.list(Some(&prefix_path));
        let objects: Vec<_> = stream.collect().await;

        let total_files = objects.len();

        // Update progress
        if let Some(progress) = &mut self.delete_progress {
            progress.total_files = total_files;
        }

        let mut files_completed = 0;

        for result in objects {
            match result {
                Ok(meta) => {
                    let file_path = meta.location.as_ref();

                    // Update progress
                    if let Some(progress) = &mut self.delete_progress {
                        progress.current_file = file_path.to_string();
                    }

                    // Delete the file
                    if let Err(e) = object_store.delete(&meta.location).await {
                        if let Some(progress) = &mut self.delete_progress {
                            progress.error_message =
                                Some(format!("Failed to delete {file_path}: {e}"));
                        }
                        // Continue with other files even if one fails
                    } else {
                        files_completed += 1;

                        // Update progress
                        if let Some(progress) = &mut self.delete_progress {
                            progress.files_completed = files_completed;
                        }
                    }
                }
                Err(e) => {
                    if let Some(progress) = &mut self.delete_progress {
                        progress.error_message = Some(format!("Failed to list file: {e}"));
                    }
                }
            }
        }

        Ok(())
    }

    /// Filter files based on search query.
    pub fn filter_files(&mut self) {
        if self.search_query.is_empty() {
            self.files = self.all_files.clone();
            self.file_items = self.all_file_items.clone();
        } else {
            let filtered_items: Vec<FileItem> = self
                .all_file_items
                .iter()
                .filter(|item| {
                    item.actual_name
                        .to_lowercase()
                        .contains(&self.search_query.to_lowercase())
                })
                .cloned()
                .collect();

            self.file_items.clone_from(&filtered_items);
            self.files = filtered_items
                .iter()
                .map(|item| item.display_name.clone())
                .collect();
        }
        self.selected_index = 0;
    }

    /// Load the list of containers from Azure Storage.
    async fn load_containers(&mut self) -> color_eyre::Result<()> {
        self.is_loading = true;
        self.error_message = None;
        self.success_message = None;

        match self.list_containers().await {
            Ok(containers) => {
                self.all_containers.clone_from(&containers);
                if self.container_search_mode && !self.container_search_query.is_empty() {
                    self.filter_containers();
                } else {
                    self.containers = containers;
                    self.selected_container_index = 0;
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to list containers: {e}"));
            }
        }

        self.is_loading = false;
        Ok(())
    }

    /// List all containers in the storage account with pagination support.
    async fn list_containers(&mut self) -> Result<Vec<ContainerInfo>, String> {
        let account_name = &self.storage_account;
        let access_key = &self.access_key;

        // Decode the base64 access key
        let key = general_purpose::STANDARD
            .decode(access_key)
            .map_err(|e| format!("Failed to decode access key: {e}"))?;

        let mut all_containers = Vec::new();
        let mut next_marker: Option<String> = None;

        // Continue fetching pages until no more results
        loop {
            // Create the request URL with pagination support
            let mut url =
                format!("https://{account_name}.blob.core.windows.net/?comp=list&maxresults=5000");
            if let Some(ref marker) = next_marker {
                url.push_str(&format!("&marker={}", urlencoding::encode(marker)));
            }

            // Create timestamp in RFC 1123 format
            let now = Utc::now();
            let date = now.format("%a, %d %b %Y %H:%M:%S GMT").to_string();

            // Construct the string to sign for Azure Storage API
            // Format: VERB + "\n" + Content-Encoding + "\n" + Content-Language + "\n" + Content-Length + "\n" +
            //         Content-MD5 + "\n" + Content-Type + "\n" + Date + "\n" + If-Modified-Since + "\n" +
            //         If-Match + "\n" + If-None-Match + "\n" + If-Unmodified-Since + "\n" + Range + "\n" +
            //         CanonicalizedHeaders + CanonicalizedResource
            let canonicalized_resource = if let Some(ref marker) = next_marker {
                format!("/{account_name}/\ncomp:list\nmarker:{marker}\nmaxresults:5000")
            } else {
                format!("/{account_name}/\ncomp:list\nmaxresults:5000")
            };

            let string_to_sign = format!(
                "GET\n\n\n\n\n\n\n\n\n\n\n\nx-ms-date:{date}\nx-ms-version:2020-08-04\n{canonicalized_resource}"
            );

            // Generate HMAC-SHA256 signature
            let mut mac = Hmac::<Sha256>::new_from_slice(&key)
                .map_err(|e| format!("Failed to create HMAC: {e}"))?;
            mac.update(string_to_sign.as_bytes());
            let signature = general_purpose::STANDARD.encode(mac.finalize().into_bytes());

            // Create authorization header
            let authorization = format!("SharedKey {account_name}:{signature}");

            // Make the HTTP request
            let client = reqwest::Client::new();
            let response = client
                .get(&url)
                .header("x-ms-date", &date)
                .header("x-ms-version", "2020-08-04")
                .header("Authorization", &authorization)
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;

            let status = response.status();
            let response_text = response
                .text()
                .await
                .map_err(|e| format!("Failed to read response: {e}"))?;

            if !status.is_success() {
                return Err(format!(
                    "HTTP {} {} - {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or(""),
                    response_text
                ));
            }

            // Parse XML response and extract containers and next marker
            let (mut containers, next_marker_option) =
                Self::parse_containers_xml_with_marker(&response_text)
                    .map_err(|e| format!("Failed to parse XML response: {e}"))?;

            // Add containers from this page to our collection
            all_containers.append(&mut containers);

            // Check if there are more pages
            if let Some(marker) = next_marker_option {
                next_marker = Some(marker);
            } else {
                // No more pages, break the loop
                break;
            }
        }

        Ok(all_containers)
    }

    /// Parse XML response from Azure Storage list containers API with pagination marker support.
    fn parse_containers_xml_with_marker(
        xml: &str,
    ) -> color_eyre::Result<(Vec<ContainerInfo>, Option<String>)> {
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

        // Look for NextMarker to determine if there are more pages
        // Pattern: <NextMarker>marker_value</NextMarker>
        let next_marker_regex = Regex::new(r"<NextMarker>(.*?)</NextMarker>")?;
        let next_marker = next_marker_regex
            .captures(xml)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
            .filter(|s| !s.is_empty());

        Ok((containers, next_marker))
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
        self.success_message = None;
    }

    /// Exit container search mode and restore original container list.
    pub fn exit_container_search_mode(&mut self) {
        self.container_search_mode = false;
        self.container_search_query.clear();
        self.containers = self.all_containers.clone();
        self.selected_container_index = 0;
    }

    /// Handle key events when in container search mode.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but uses `Result` for API consistency.
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
    ///
    /// # Errors
    ///
    /// Returns an error if fetching blob/folder metadata fails.
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
            format!("{folder_name}/")
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

        for meta in objects.into_iter().flatten() {
            blob_count += 1;
            total_size += meta.size;
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

    /// Copy the full blob path to clipboard.
    ///
    /// # Errors
    ///
    /// Returns an error if clipboard access fails.
    pub fn copy_blob_path_to_clipboard(&mut self) -> color_eyre::Result<()> {
        if self.files.is_empty() {
            return Ok(());
        }

        let selected_file = &self.files[self.selected_index];
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);

        let (item_name, is_folder) = if selected_file.starts_with(&folder_prefix) {
            // It's a folder
            let folder_name = if let Some(name) = selected_file.strip_prefix(&folder_prefix) {
                name
            } else {
                selected_file
            };
            (folder_name, true)
        } else {
            // It's a file
            let file_name = if let Some(name) = selected_file.strip_prefix(&file_prefix) {
                name
            } else {
                selected_file
            };
            (file_name, false)
        };

        // Construct the full path
        let full_path = if self.current_path.is_empty() {
            if is_folder {
                format!("{item_name}/")
            } else {
                item_name.to_string()
            }
        } else if self.current_path.ends_with('/') {
            if is_folder {
                format!("{}{}/", self.current_path, item_name)
            } else {
                format!("{}{}", self.current_path, item_name)
            }
        } else if is_folder {
            format!("{}/{}/", self.current_path, item_name)
        } else {
            format!("{}/{}", self.current_path, item_name)
        };

        // Copy to clipboard
        let mut clipboard = Clipboard::new()?;
        clipboard.set_text(full_path.clone())?;

        // Set success message
        let item_type = if is_folder { "folder" } else { "file" };
        self.success_message = Some(format!("Copied {item_type} path to clipboard: {full_path}"));
        self.error_message = None; // Clear any existing error message

        Ok(())
    }

    /// Show the download destination picker.
    pub async fn show_download_picker(&mut self) {
        if self.files.is_empty() {
            return;
        }

        self.show_download_picker = true;
    }

    /// Start the download process for the selected file or folder.
    ///
    /// # Errors
    ///
    /// Returns an error if the download operation fails.
    pub async fn start_download(&mut self) -> color_eyre::Result<()> {
        if self.files.is_empty() || self.download_destination.is_none() {
            return Ok(());
        }

        let selected_file = self.files[self.selected_index].clone();
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);

        let is_folder = selected_file.starts_with(&folder_prefix);
        let name = if is_folder {
            selected_file
                .strip_prefix(&folder_prefix)
                .unwrap_or(&selected_file)
                .to_string()
        } else {
            selected_file
                .strip_prefix(&file_prefix)
                .unwrap_or(&selected_file)
                .to_string()
        };

        self.is_downloading = true;
        self.show_download_picker = false;

        if is_folder {
            self.download_folder(&name).await?;
        } else {
            self.download_file(&name).await?;
        }

        self.is_downloading = false;
        self.download_progress = None;
        Ok(())
    }

    /// Download a single file.
    async fn download_file(&mut self, file_name: &str) -> color_eyre::Result<()> {
        let object_store = self
            .object_store
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;

        let destination = self
            .download_destination
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No download destination selected"))?;

        let blob_path = if self.current_path.is_empty() {
            file_name.to_string()
        } else if self.current_path.ends_with('/') {
            format!("{}{}", self.current_path, file_name)
        } else {
            format!("{}/{}", self.current_path, file_name)
        };

        let object_path = ObjectPath::from(blob_path.as_str());

        // Initialize progress
        self.download_progress = Some(DownloadProgress {
            current_file: file_name.to_string(),
            files_completed: 0,
            total_files: 1,
            bytes_downloaded: 0,
            total_bytes: None,
            error_message: None,
        });

        // Get file metadata for total size
        if let Ok(meta) = object_store.head(&object_path).await
            && let Some(progress) = &mut self.download_progress
        {
            progress.total_bytes = Some(meta.size);
        }

        // Create destination file path
        let file_destination = destination.join(file_name);

        // Ensure parent directory exists
        if let Some(parent) = file_destination.parent() {
            fs::create_dir_all(parent)?;
        }

        // Download the file
        match object_store.get(&object_path).await {
            Ok(get_result) => {
                let bytes = get_result.bytes().await?;
                fs::write(&file_destination, &bytes)?;

                if let Some(progress) = &mut self.download_progress {
                    progress.bytes_downloaded = bytes.len() as u64;
                    progress.files_completed = 1;
                }
            }
            Err(e) => {
                if let Some(progress) = &mut self.download_progress {
                    progress.error_message = Some(format!("Failed to download {file_name}: {e}"));
                }
                return Err(color_eyre::eyre::eyre!("Download failed: {}", e));
            }
        }

        Ok(())
    }

    /// Download all files in a folder.
    async fn download_folder(&mut self, folder_name: &str) -> color_eyre::Result<()> {
        let object_store = self
            .object_store
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;

        let destination = self
            .download_destination
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No download destination selected"))?;

        let folder_path = if self.current_path.is_empty() {
            format!("{folder_name}/")
        } else if self.current_path.ends_with('/') {
            format!("{}{}/", self.current_path, folder_name)
        } else {
            format!("{}/{}/", self.current_path, folder_name)
        };

        let object_path = ObjectPath::from(folder_path.as_str());

        // Create destination folder
        let folder_destination = destination.join(folder_name);
        fs::create_dir_all(&folder_destination)?;

        // List all files in the folder
        let stream = object_store.list(Some(&object_path));
        let objects: Vec<_> = stream.collect().await;

        let total_files = objects.len();
        let mut files_completed = 0;
        let mut total_bytes_downloaded = 0u64;

        // Initialize progress
        self.download_progress = Some(DownloadProgress {
            current_file: String::new(),
            files_completed: 0,
            total_files,
            bytes_downloaded: 0,
            total_bytes: None,
            error_message: None,
        });

        for result in objects {
            match result {
                Ok(meta) => {
                    let file_path = meta.location.as_ref();
                    let relative_path = file_path.strip_prefix(&folder_path).unwrap_or(file_path);

                    // Update progress
                    if let Some(progress) = &mut self.download_progress {
                        progress.current_file = relative_path.to_string();
                    }

                    // Create full destination path
                    let file_destination = folder_destination.join(relative_path);

                    // Ensure parent directory exists
                    if let Some(parent) = file_destination.parent() {
                        fs::create_dir_all(parent)?;
                    }

                    // Download the file
                    match object_store.get(&meta.location).await {
                        Ok(get_result) => {
                            let bytes = get_result.bytes().await?;
                            fs::write(&file_destination, &bytes)?;

                            files_completed += 1;
                            total_bytes_downloaded += bytes.len() as u64;

                            // Update progress
                            if let Some(progress) = &mut self.download_progress {
                                progress.files_completed = files_completed;
                                progress.bytes_downloaded = total_bytes_downloaded;
                            }
                        }
                        Err(e) => {
                            if let Some(progress) = &mut self.download_progress {
                                progress.error_message =
                                    Some(format!("Failed to download {relative_path}: {e}"));
                            }
                            // Continue with other files even if one fails
                        }
                    }
                }
                Err(e) => {
                    if let Some(progress) = &mut self.download_progress {
                        progress.error_message = Some(format!("Failed to list file: {e}"));
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle Enter key when download picker is shown.
    ///
    /// # Errors
    ///
    /// Returns an error if the file dialog or download fails.
    pub async fn confirm_download(&mut self) -> color_eyre::Result<()> {
        if self.show_download_picker {
            // Use the file dialog to pick a destination folder
            let file_dialog = rfd::FileDialog::new();

            // Run the file dialog in a spawn_blocking since it's blocking
            let path_result = tokio::task::spawn_blocking(move || file_dialog.pick_folder()).await;

            match path_result {
                Ok(Some(path)) => {
                    self.download_destination = Some(path);
                    self.start_download().await?;
                }
                Ok(None) => {
                    // User cancelled the dialog
                    self.show_download_picker = false;
                }
                Err(e) => {
                    self.show_download_picker = false;
                    self.error_message = Some(format!("Failed to open file dialog: {e}"));
                }
            }
        }
        Ok(())
    }

    // ========================================
    // Preview Panel Methods
    // ========================================

    /// Load preview for the currently selected file.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching or parsing the file fails.
    pub async fn load_preview(&mut self) -> color_eyre::Result<()> {
        if self.files.is_empty() {
            return Ok(());
        }

        let selected_file = &self.files[self.selected_index];
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);

        // Check if it's a folder
        if selected_file.starts_with(&folder_prefix) {
            self.preview_error = Some("Cannot preview folders".to_string());
            self.show_preview = true;
            return Ok(());
        }

        let name = selected_file
            .strip_prefix(&file_prefix)
            .unwrap_or(selected_file);

        // Check file type
        let file_type = PreviewFileType::from_extension(name);
        if !file_type.is_supported() {
            self.preview_error =
                Some("Unsupported file type. Preview supports: CSV, TSV, JSON".to_string());
            self.preview_file_type = Some(file_type);
            self.show_preview = true;
            return Ok(());
        }

        self.preview_file_type = Some(file_type.clone());
        self.is_loading_preview = true;
        self.show_preview = true;
        self.preview_error = None;
        self.preview_data = None;
        self.preview_scroll = (0, 0);
        self.preview_selected_row = 0;

        // Fetch file content (up to MAX_PREVIEW_BYTES)
        let object_store = self
            .object_store
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;

        let blob_path = if self.current_path.is_empty() {
            name.to_string()
        } else if self.current_path.ends_with('/') {
            format!("{}{}", self.current_path, name)
        } else {
            format!("{}/{}", self.current_path, name)
        };

        let object_path = ObjectPath::from(blob_path.as_str());

        // Get file content with size limit
        let get_result = object_store
            .get_range(&object_path, 0..(MAX_PREVIEW_BYTES as u64))
            .await;

        self.is_loading_preview = false;

        match get_result {
            Ok(bytes) => {
                // Parse the data
                match parse_preview(&bytes, &file_type) {
                    Ok(data) => {
                        self.preview_data = Some(data);
                    }
                    Err(e) => {
                        self.preview_error = Some(e);
                    }
                }
            }
            Err(e) => {
                self.preview_error = Some(format!("Failed to fetch file: {e}"));
            }
        }

        Ok(())
    }

    /// Close the preview panel.
    pub fn close_preview(&mut self) {
        self.show_preview = false;
        self.preview_data = None;
        self.preview_file_type = None;
        self.preview_error = None;
        self.preview_scroll = (0, 0);
        self.preview_selected_row = 0;
        self.is_loading_preview = false;
    }

    /// Scroll preview up (decrease row offset).
    pub fn preview_scroll_up(&mut self) {
        if self.preview_selected_row > 0 {
            self.preview_selected_row = self.preview_selected_row.saturating_sub(1);
        }
        // Also adjust scroll offset if needed
        if self.preview_selected_row < self.preview_scroll.0 {
            self.preview_scroll.0 = self.preview_selected_row;
        }
    }

    /// Scroll preview down (increase row offset).
    pub fn preview_scroll_down(&mut self) {
        let max_row = match &self.preview_data {
            Some(PreviewData::Table(table)) => table.rows.len().saturating_sub(1),
            Some(PreviewData::Json(json)) => json.content.lines().count().saturating_sub(1),
            None => 0,
        };
        if self.preview_selected_row < max_row {
            self.preview_selected_row += 1;
        }
    }

    /// Scroll preview left (decrease column offset).
    pub fn preview_scroll_left(&mut self) {
        self.preview_scroll.1 = self.preview_scroll.1.saturating_sub(1);
    }

    /// Scroll preview right (increase column offset).
    pub fn preview_scroll_right(&mut self) {
        self.preview_scroll.1 += 1;
    }
}
