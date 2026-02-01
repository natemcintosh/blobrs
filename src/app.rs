use crate::{
    event::{AppEvent, Event, EventHandler},
    preview::{
        MAX_PARQUET_PREVIEW_BYTES, MAX_PREVIEW_BYTES, PreviewData, PreviewFileType,
        parse_parquet_schema, parse_preview,
    },
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
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortCriteria {
    Name,
    DateModified,
    DateCreated,
    Size,
}

#[derive(Debug, Clone)]
pub struct BrowsingState {
    pub object_store: Arc<dyn ObjectStore>,
    pub current_path: String,
    pub files: Vec<String>,
    pub file_items: Vec<FileItem>,
    pub selected_index: usize,
}

#[derive(Debug, Clone)]
pub enum Session {
    Selecting,
    Browsing(BrowsingState),
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum BlobInfo {
    File {
        name: String,
        size: u64,
        last_modified: String,
        etag: Option<String>,
    },
    Folder {
        name: String,
        blob_count: usize,
        total_size: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Folder,
}


#[derive(Debug, Clone)]
pub struct FileItem {
    pub display_name: String, // What to show in the UI (with icon)
    pub actual_name: String,  // The actual file/folder name
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub last_modified: Option<chrono::DateTime<chrono::Utc>>,
    pub created: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub enum Modal {
    None,
    BlobInfo { info: BlobInfo },
    DownloadPicker { destination: Option<PathBuf> },
    SortPicker,
    Clone {
        input: String,
        original_path: String,
        is_folder: bool,
    },
    DeleteConfirm {
        input: String,
        target_path: String,
        target_name: String,
        is_folder: bool,
    },
}

#[derive(Debug, Clone)]
pub enum AsyncOp {
    None,
    LoadingContainers,
    LoadingFiles,
    Downloading(DownloadProgress),
    Cloning(CloneProgress),
    Deleting(DeleteProgress),
}

#[derive(Debug, Clone)]
pub enum Search {
    Inactive,
    Containers {
        query: String,
        all_containers: Vec<ContainerInfo>,
    },
    Files {
        query: String,
        all_files: Vec<String>,
        all_file_items: Vec<FileItem>,
    },
}

#[derive(Debug, Clone)]
pub struct UiToggles {
    pub show_preview: bool,
    pub is_loading_preview: bool,
}

/// Application.
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Event handler.
    pub events: EventHandler,
    /// Current application session.
    pub session: Session,
    /// Azure Storage Account name.
    pub storage_account: String,
    /// Azure Storage Access Key.
    pub access_key: String,
    /// List of available containers.
    pub containers: Vec<ContainerInfo>,
    /// Currently selected container index.
    pub selected_container_index: usize,
    /// Current async operation (loading, downloading, etc.).
    pub async_op: AsyncOp,
    /// Error message to display.
    pub error_message: Option<String>,
    /// Success message to display.
    pub success_message: Option<String>,
    /// Search state for containers or files.
    pub search: Search,
    /// Icon set based on terminal capabilities.
    pub icons: IconSet,
    /// Current modal state.
    pub modal: Modal,
    /// Independent UI toggles.
    pub ui: UiToggles,
    /// Current sort criteria for blobs.
    pub sort_criteria: SortCriteria,
    /// Preview data for the current file.
    pub preview_data: Option<PreviewData>,
    /// Preview file type.
    pub preview_file_type: Option<PreviewFileType>,
    /// Preview scroll offset (row, column).
    pub preview_scroll: (usize, usize),
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
            .field("session", &self.session)
            .field("storage_account", &self.storage_account)
            .field("containers", &self.containers)
            .field("selected_container_index", &self.selected_container_index)
            .field("async_op", &self.async_op)
            .field("error_message", &self.error_message)
            .field("success_message", &self.success_message)
            .field("search", &self.search)
            .field("icons", &self.icons)
            .field("modal", &self.modal)
            .field("ui", &self.ui)
            .field("sort_criteria", &self.sort_criteria)
            .field("preview_file_type", &self.preview_file_type)
            .field("preview_scroll", &self.preview_scroll)
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
            session: Session::Selecting,
            storage_account,
            access_key,
            containers: Vec::new(),
            selected_container_index: 0,
            async_op: AsyncOp::None,
            error_message: None,
            success_message: None,
            search: Search::Inactive,
            icons: detect_terminal_icons(),
            modal: Modal::None,
            ui: UiToggles {
                show_preview: false,
                is_loading_preview: false,
            },
            sort_criteria: SortCriteria::Name,
            preview_data: None,
            preview_file_type: None,
            preview_scroll: (0, 0),
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
        if self.is_modal_delete_dialog() {
            return self.handle_delete_dialog_key_event(key_event).await;
        }

        // Handle clone dialog separately
        if self.is_modal_clone_dialog() {
            return self.handle_clone_dialog_key_event(key_event).await;
        }

        // Handle search mode separately
        if self.is_searching_containers() && self.is_selecting() {
            return self.handle_container_search_key_event(key_event);
        }

        if self.is_searching_files() && self.is_browsing() {
            return self.handle_search_key_event(key_event);
        }

        // Don't process keys while loading, cloning, or deleting
        if self.blocks_input() {
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
        if self.is_selecting() {
            match key_event.code {
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
            }
        } else if self.is_browsing() {
            match key_event.code {
                    KeyCode::Char('/') => {
                        if !self.is_modal_blob_info() {
                            self.enter_search_mode();
                        }
                    }
                    KeyCode::Char('r') | KeyCode::F(5) => {
                        if !self.is_modal_blob_info()
                            && let Err(e) = self.refresh_files().await
                        {
                            self.error_message = Some(format!("Refresh failed: {e}"));
                        }
                    }
                    KeyCode::Char('i') => {
                        if !self.is_modal_blob_info()
                            && !self.ui.show_preview
                            && let Err(e) = self.show_blob_info().await
                        {
                            self.error_message = Some(format!("Failed to get blob info: {e}"));
                        }
                    }
                    KeyCode::Char('p') => {
                        if !self.is_modal_blob_info()
                            && !self.is_modal_download_picker()
                            && !self.is_downloading()
                            && !self.is_modal_sort_picker()
                            && !self.is_modal_clone_dialog()
                            && !self.is_cloning()
                            && !self.is_modal_delete_dialog()
                            && !self.is_deleting()
                        {
                            if self.ui.show_preview {
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
                        if !self.is_modal_blob_info()
                            && !self.is_modal_download_picker()
                            && !self.is_downloading()
                            && !self.is_modal_sort_picker()
                        {
                            self.show_download_picker();
                        }
                    }
                    KeyCode::Char('s') => {
                        if !self.is_modal_blob_info()
                            && !self.is_modal_download_picker()
                            && !self.is_downloading()
                            && !self.is_modal_sort_picker()
                        {
                            self.modal = Modal::SortPicker;
                        } else if self.is_modal_sort_picker() {
                            // Handle sort selection
                            if let Err(e) = self.apply_sort(SortCriteria::Size) {
                                self.error_message = Some(format!("Failed to sort: {e}"));
                            }
                            self.close_modal();
                        }
                    }
                    KeyCode::Char('n') => {
                        if self.is_modal_sort_picker() {
                            if let Err(e) = self.apply_sort(SortCriteria::Name) {
                                self.error_message = Some(format!("Failed to sort: {e}"));
                            }
                            self.close_modal();
                        }
                    }
                    KeyCode::Char('m') => {
                        if self.is_modal_sort_picker() {
                            if let Err(e) = self.apply_sort(SortCriteria::DateModified) {
                                self.error_message = Some(format!("Failed to sort: {e}"));
                            }
                            self.close_modal();
                        }
                    }
                    KeyCode::Char('t') => {
                        if self.is_modal_sort_picker() {
                            if let Err(e) = self.apply_sort(SortCriteria::DateCreated) {
                                self.error_message = Some(format!("Failed to sort: {e}"));
                            }
                            self.close_modal();
                        }
                    }
                    KeyCode::Char('y') => {
                        if !self.is_modal_blob_info()
                            && !self.is_modal_download_picker()
                            && !self.is_downloading()
                            && !self.is_modal_sort_picker()
                            && !self.is_modal_clone_dialog()
                            && !self.is_cloning()
                        {
                            // Copy blob path to clipboard
                            if let Err(e) = self.copy_blob_path_to_clipboard() {
                                self.error_message =
                                    Some(format!("Failed to copy to clipboard: {e}"));
                            }
                        }
                    }
                    KeyCode::Char('c') => {
                        if !self.is_modal_blob_info()
                            && !self.is_modal_download_picker()
                            && !self.is_downloading()
                            && !self.is_modal_sort_picker()
                            && !self.is_modal_clone_dialog()
                            && !self.is_cloning()
                            && !self.is_modal_delete_dialog()
                            && !self.is_deleting()
                        {
                            // Open clone dialog
                            self.open_clone_dialog();
                        }
                    }
                    KeyCode::Char('x') | KeyCode::Delete => {
                        if !self.is_modal_blob_info()
                            && !self.is_modal_download_picker()
                            && !self.is_downloading()
                            && !self.is_modal_sort_picker()
                            && !self.is_modal_clone_dialog()
                            && !self.is_cloning()
                            && !self.is_modal_delete_dialog()
                            && !self.is_deleting()
                        {
                            // Open delete dialog
                            self.open_delete_dialog();
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.ui.show_preview {
                            self.preview_scroll_up();
                        } else if !self.is_modal_blob_info() && !self.is_modal_sort_picker() {
                            self.move_up();
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if self.ui.show_preview {
                            self.preview_scroll_down();
                        } else if !self.is_modal_blob_info() && !self.is_modal_sort_picker() {
                            self.move_down();
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                        if self.ui.show_preview {
                            self.preview_scroll_right();
                        } else if self.is_modal_download_picker() {
                            if let Err(e) = self.confirm_download().await {
                                self.error_message = Some(format!("Download failed: {e}"));
                            }
                        } else if !self.is_modal_blob_info()
                            && !self.is_modal_sort_picker()
                            && let Err(e) = self.enter_directory().await
                        {
                            self.error_message = Some(format!("Enter directory failed: {e}"));
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        if self.ui.show_preview {
                            self.preview_scroll_left();
                        } else if self.is_modal_download_picker() {
                            // Close download picker
                            self.close_modal();
                        } else if self.is_modal_blob_info() {
                            // Close popup
                            self.close_modal();
                        } else if self.is_modal_sort_picker() {
                            // Close sort popup
                            self.close_modal();
                        } else if let Err(e) = self.go_up_directory().await {
                            self.error_message = Some(format!("Go up failed: {e}"));
                        }
                    }
                    KeyCode::Esc => {
                        if self.ui.show_preview {
                            // Close preview panel
                            self.close_preview();
                        } else if self.is_modal_download_picker() {
                            // Close download picker
                            self.close_modal();
                        } else if self.is_modal_blob_info() {
                            // Close popup
                            self.close_modal();
                        } else if self.is_modal_sort_picker() {
                            // Close sort popup
                            self.close_modal();
                        } else if self
                            .browsing()
                            .map(|state| !state.current_path.is_empty())
                            .unwrap_or(false)
                        {
                            // Go up one directory level if not at container root
                            if let Err(e) = self.go_up_directory().await {
                                self.error_message = Some(format!("Go up failed: {e}"));
                            }
                        } else {
                            // At container root, go back to container selection
                            self.session = Session::Selecting;
                            self.search = Search::Inactive;
                            self.close_modal();
                        }
                    }
                    KeyCode::Backspace => {
                        if self.is_modal_download_picker() {
                            // Close download picker
                            self.close_modal();
                        } else if self.is_modal_blob_info() {
                            // Close popup
                            self.close_modal();
                        } else if self.is_modal_sort_picker() {
                            // Close sort popup
                            self.close_modal();
                        } else {
                            // Go back to container selection
                            self.session = Session::Selecting;
                            self.search = Search::Inactive;
                            self.close_modal();
                        }
                    }
                    _ => {
                        self.error_message = None;
                        self.success_message = None;
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

    fn is_selecting(&self) -> bool {
        matches!(self.session, Session::Selecting)
    }

    fn is_browsing(&self) -> bool {
        matches!(self.session, Session::Browsing(_))
    }

    pub(crate) fn browsing(&self) -> Option<&BrowsingState> {
        match &self.session {
            Session::Browsing(state) => Some(state),
            _ => None,
        }
    }

    fn browsing_mut(&mut self) -> Option<&mut BrowsingState> {
        match &mut self.session {
            Session::Browsing(state) => Some(state),
            _ => None,
        }
    }

    fn is_modal_blob_info(&self) -> bool {
        matches!(self.modal, Modal::BlobInfo { .. })
    }

    fn is_modal_download_picker(&self) -> bool {
        matches!(self.modal, Modal::DownloadPicker { .. })
    }

    fn is_modal_sort_picker(&self) -> bool {
        matches!(self.modal, Modal::SortPicker)
    }

    fn is_modal_clone_dialog(&self) -> bool {
        matches!(self.modal, Modal::Clone { .. })
    }

    fn is_modal_delete_dialog(&self) -> bool {
        matches!(self.modal, Modal::DeleteConfirm { .. })
    }

    fn close_modal(&mut self) {
        self.modal = Modal::None;
    }

    pub(crate) fn is_loading_containers(&self) -> bool {
        matches!(self.async_op, AsyncOp::LoadingContainers)
    }

    pub(crate) fn is_loading_files(&self) -> bool {
        matches!(self.async_op, AsyncOp::LoadingFiles)
    }

    pub(crate) fn is_searching_containers(&self) -> bool {
        matches!(self.search, Search::Containers { .. })
    }

    pub(crate) fn is_searching_files(&self) -> bool {
        matches!(self.search, Search::Files { .. })
    }

    pub(crate) fn container_search_query(&self) -> Option<&str> {
        match &self.search {
            Search::Containers { query, .. } => Some(query.as_str()),
            _ => None,
        }
    }

    pub(crate) fn file_search_query(&self) -> Option<&str> {
        match &self.search {
            Search::Files { query, .. } => Some(query.as_str()),
            _ => None,
        }
    }

    fn is_downloading(&self) -> bool {
        matches!(self.async_op, AsyncOp::Downloading(_))
    }

    fn is_cloning(&self) -> bool {
        matches!(self.async_op, AsyncOp::Cloning(_))
    }

    fn is_deleting(&self) -> bool {
        matches!(self.async_op, AsyncOp::Deleting(_))
    }

    fn blocks_input(&self) -> bool {
        matches!(
            self.async_op,
            AsyncOp::LoadingContainers | AsyncOp::LoadingFiles | AsyncOp::Cloning(_) | AsyncOp::Deleting(_)
        )
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// List blobs and prefixes with metadata for sorting.
    async fn list_file_items(&self, prefix: &str) -> color_eyre::Result<Vec<FileItem>> {
        let object_store = self
            .browsing()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?
            .object_store
            .clone();

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
                    kind: EntryKind::Folder,
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
                    kind: EntryKind::File,
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
    pub fn apply_sort(&mut self, criteria: SortCriteria) -> color_eyre::Result<()> {
        self.sort_criteria = criteria;

        if let Some(state) = self.browsing_mut()
            && !state.file_items.is_empty()
        {
            Self::sort_file_items_static(&mut state.file_items, criteria);
            // Update the display list
            state.files = state
                .file_items
                .iter()
                .map(|item| item.display_name.clone())
                .collect();
        }

        // Also sort the unfiltered list when searching
        if let Search::Files {
            all_file_items,
            all_files,
            ..
        } = &mut self.search
            && !all_file_items.is_empty()
        {
            Self::sort_file_items_static(all_file_items, criteria);
            *all_files = all_file_items
                .iter()
                .map(|item| item.display_name.clone())
                .collect();
        }

        Ok(())
    }

    /// Sort file items based on the given criteria.
    fn sort_file_items_static(items: &mut [FileItem], criteria: SortCriteria) {
        items.sort_by(|a, b| {
            // Always put folders first
            match (a.kind, b.kind) {
                (EntryKind::Folder, EntryKind::File) => std::cmp::Ordering::Less,
                (EntryKind::File, EntryKind::Folder) => std::cmp::Ordering::Greater,
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
        let current_path = match self.browsing() {
            Some(state) => state.current_path.clone(),
            None => return Ok(()),
        };

        self.async_op = AsyncOp::LoadingFiles;
        self.error_message = None;
        self.success_message = None;

        match self.list_file_items(&current_path).await {
            Ok(mut file_items) => {
                // Apply current sorting
                Self::sort_file_items_static(&mut file_items, self.sort_criteria);

                // Create display strings
                let files: Vec<String> = file_items
                    .iter()
                    .map(|item| item.display_name.clone())
                    .collect();

                let search_query = match &self.search {
                    Search::Files { query, .. } => Some(query.clone()),
                    _ => None,
                };

                if let Search::Files {
                    all_files,
                    all_file_items,
                    ..
                } = &mut self.search
                {
                    all_file_items.clone_from(&file_items);
                    all_files.clone_from(&files);
                }

                if let Some(query) = search_query {
                    if !query.is_empty() {
                        self.apply_file_search(&query);
                    } else if let Some(state) = self.browsing_mut() {
                        state.file_items = file_items;
                        state.files = files;
                        state.selected_index = 0;
                    }
                } else if let Some(state) = self.browsing_mut() {
                    state.file_items = file_items;
                    state.files = files;
                    state.selected_index = 0;
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to list blobs: {e}"));
            }
        }

        self.async_op = AsyncOp::None;
        Ok(())
    }

    /// Move the selection up.
    pub fn move_up(&mut self) {
        if let Some(state) = self.browsing_mut()
            && state.selected_index > 0
        {
            state.selected_index -= 1;
        }
    }

    /// Move the selection down.
    pub fn move_down(&mut self) {
        if let Some(state) = self.browsing_mut()
            && state.selected_index < state.files.len().saturating_sub(1)
        {
            state.selected_index += 1;
        }
    }

    /// Enter a directory if the selected item is a folder.
    ///
    /// # Errors
    ///
    /// Returns an error if refreshing the file list fails.
    pub async fn enter_directory(&mut self) -> color_eyre::Result<()> {
        let (selected_file, current_path) = match self.browsing() {
            Some(state) => {
                if state.files.is_empty() {
                    return Ok(());
                }
                (
                    state.files[state.selected_index].clone(),
                    state.current_path.clone(),
                )
            }
            None => return Ok(()),
        };

        if selected_file.is_empty() {
            return Ok(());
        }
        // Check if the selected item is a directory (starts with folder icon)
        let folder_prefix = format!("{} ", self.icons.folder);
        if let Some(dir_name) = selected_file.strip_prefix(&folder_prefix) {
            let new_path = if current_path.is_empty() {
                format!("{dir_name}/")
            } else if current_path.ends_with('/') {
                format!("{}{}/", current_path, dir_name)
            } else {
                format!("{}/{}/", current_path, dir_name)
            };

            if let Some(state) = self.browsing_mut() {
                state.current_path = new_path;
            }
            // Exit search mode when navigating
            if self.is_searching_files() {
                self.search = Search::Inactive;
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
        let current_path = match self.browsing() {
            Some(state) => state.current_path.clone(),
            None => return Ok(()),
        };

        if current_path.is_empty() {
            return Ok(()); // Already at root
        }

        // Remove trailing slash and go up one level
        let trimmed = current_path.trim_end_matches('/');
        if let Some(last_slash) = trimmed.rfind('/') {
            if let Some(state) = self.browsing_mut() {
                state.current_path = format!("{}/", &trimmed[..last_slash]);
            }
        } else if let Some(state) = self.browsing_mut() {
            state.current_path = String::new(); // Go to root
        }

        // Exit search mode when navigating
        if self.is_searching_files() {
            self.search = Search::Inactive;
        }
        self.refresh_files().await?;
        Ok(())
    }

    /// Enter search mode.
    pub fn enter_search_mode(&mut self) {
        let (files, file_items) = match self.browsing() {
            Some(state) => (state.files.clone(), state.file_items.clone()),
            None => return,
        };

        self.search = Search::Files {
            query: String::new(),
            all_files: files,
            all_file_items: file_items,
        };
        self.error_message = None;
        self.success_message = None;
    }

    /// Exit search mode and restore original file list.
    pub fn exit_search_mode(&mut self) {
        let (all_files, all_file_items) = match &self.search {
            Search::Files {
                all_files,
                all_file_items,
                ..
            } => (all_files.clone(), all_file_items.clone()),
            _ => (Vec::new(), Vec::new()),
        };

        if let Some(state) = self.browsing_mut() {
            state.files = all_files;
            state.file_items = all_file_items;
            state.selected_index = 0;
        }
        self.search = Search::Inactive;
    }

    /// Handle key events when in search mode.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but uses `Result` for API consistency.
    pub fn handle_search_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        let query = match &mut self.search {
            Search::Files { query, .. } => query,
            _ => return Ok(()),
        };

        match key_event.code {
            KeyCode::Esc => {
                self.exit_search_mode();
            }
            KeyCode::Enter => {
                // Exit search mode but keep the filtered results
                self.search = Search::Inactive;
            }
            KeyCode::Backspace => {
                query.pop();
                let current = query.clone();
                self.apply_file_search(&current);
            }
            KeyCode::Up if key_event.modifiers == KeyModifiers::CONTROL => {
                self.move_up();
            }
            KeyCode::Down if key_event.modifiers == KeyModifiers::CONTROL => {
                self.move_down();
            }
            KeyCode::Char(c) => {
                query.push(c);
                let current = query.clone();
                self.apply_file_search(&current);
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
        let (input, original_path, _is_folder) = match &mut self.modal {
            Modal::Clone {
                input,
                original_path,
                is_folder,
            } => (input, original_path, is_folder),
            _ => return Ok(()),
        };

        match key_event.code {
            KeyCode::Esc => {
                self.close_modal();
            }
            KeyCode::Enter => {
                // Only allow confirm if name is different from original
                if input != original_path && !input.is_empty() {
                    if let Err(e) = self.execute_clone().await {
                        self.error_message = Some(format!("Clone failed: {e}"));
                    }
                    self.close_modal();
                }
            }
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char(c) => {
                input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    /// Open the clone dialog for the selected item.
    pub fn open_clone_dialog(&mut self) {
        let (selected_file, current_path) = match self.browsing() {
            Some(state) => {
                if state.files.is_empty() {
                    return;
                }
                (
                    state.files[state.selected_index].clone(),
                    state.current_path.clone(),
                )
            }
            None => return,
        };

        let selected_file = selected_file.as_str();
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
        let full_path = if current_path.is_empty() {
            if is_folder {
                format!("{item_name}/")
            } else {
                item_name
            }
        } else if current_path.ends_with('/') {
            if is_folder {
                format!("{}{}/", current_path, item_name)
            } else {
                format!("{}{}", current_path, item_name)
            }
        } else if is_folder {
            format!("{}/{}/", current_path, item_name)
        } else {
            format!("{}/{}", current_path, item_name)
        };

        self.modal = Modal::Clone {
            input: full_path.clone(),
            original_path: full_path,
            is_folder,
        };
    }

    /// Execute the clone operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the clone or subsequent file refresh fails.
    pub async fn execute_clone(&mut self) -> color_eyre::Result<()> {
        let (mut new_path, original_path, is_folder) = match &self.modal {
            Modal::Clone {
                input,
                original_path,
                is_folder,
            } => (input.clone(), original_path.clone(), *is_folder),
            _ => return Ok(()),
        };

        // Ensure folder paths end with /
        if is_folder && !new_path.ends_with('/') {
            new_path.push('/');
        }

        self.async_op = AsyncOp::Cloning(CloneProgress {
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

        self.async_op = AsyncOp::None;

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
            .browsing()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?
            .object_store
            .clone();

        let source_path = ObjectPath::from(source);
        let dest_path = ObjectPath::from(destination);

        // Update progress
        if let AsyncOp::Cloning(progress) = &mut self.async_op {
            progress.current_file = source.to_string();
            progress.total_files = 1;
        }

        // Use copy operation (server-side copy)
        object_store.copy(&source_path, &dest_path).await?;

        // Update progress
        if let AsyncOp::Cloning(progress) = &mut self.async_op {
            progress.files_completed = 1;
        }

        Ok(())
    }

    /// Clone all blobs in a folder (prefix).
    async fn clone_folder(&mut self, source: &str, destination: &str) -> color_eyre::Result<()> {
        let object_store = self
            .browsing()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?
            .object_store
            .clone();

        let source_path = ObjectPath::from(source);

        // List all files in the source folder
        let stream = object_store.list(Some(&source_path));
        let objects: Vec<_> = stream.collect().await;

        let total_files = objects.len();

        // Update progress
        if let AsyncOp::Cloning(progress) = &mut self.async_op {
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
                    if let AsyncOp::Cloning(progress) = &mut self.async_op {
                        progress.current_file = file_path.to_string();
                    }

                    // Copy the file
                    let dest_object_path = ObjectPath::from(dest_file_path.as_str());
                    if let Err(e) = object_store.copy(&meta.location, &dest_object_path).await {
                        if let AsyncOp::Cloning(progress) = &mut self.async_op {
                            progress.error_message =
                                Some(format!("Failed to clone {file_path}: {e}"));
                        }
                        // Continue with other files even if one fails
                    } else {
                        files_completed += 1;

                        // Update progress
                        if let AsyncOp::Cloning(progress) = &mut self.async_op {
                            progress.files_completed = files_completed;
                        }
                    }
                }
                Err(e) => {
                    if let AsyncOp::Cloning(progress) = &mut self.async_op {
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
        let (input, target_name) = match &mut self.modal {
            Modal::DeleteConfirm {
                input,
                target_name,
                ..
            } => (input, target_name),
            _ => return Ok(()),
        };

        match key_event.code {
            KeyCode::Esc => {
                self.close_modal();
            }
            KeyCode::Enter => {
                // Only allow confirm if the typed name matches the target name
                if input == target_name {
                    if let Err(e) = self.execute_delete().await {
                        self.error_message = Some(format!("Delete failed: {e}"));
                    }
                    self.close_modal();
                }
            }
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char(c) => {
                input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    /// Open the delete confirmation dialog for the selected item.
    pub fn open_delete_dialog(&mut self) {
        let (selected_file, current_path) = match self.browsing() {
            Some(state) => {
                if state.files.is_empty() {
                    return;
                }
                (
                    state.files[state.selected_index].clone(),
                    state.current_path.clone(),
                )
            }
            None => return,
        };

        let selected_file = selected_file.as_str();
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
        let full_path = if current_path.is_empty() {
            if is_folder {
                format!("{item_name}/")
            } else {
                item_name.clone()
            }
        } else if current_path.ends_with('/') {
            if is_folder {
                format!("{}{}/", current_path, item_name)
            } else {
                format!("{}{}", current_path, item_name)
            }
        } else if is_folder {
            format!("{}/{}/", current_path, item_name)
        } else {
            format!("{}/{}", current_path, item_name)
        };

        self.modal = Modal::DeleteConfirm {
            input: String::new(),
            target_path: full_path,
            target_name: item_name,
            is_folder,
        };
    }

    /// Execute the delete operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the delete or subsequent file refresh fails.
    pub async fn execute_delete(&mut self) -> color_eyre::Result<()> {
        let (target_path, is_folder) = match &self.modal {
            Modal::DeleteConfirm {
                target_path,
                is_folder,
                ..
            } => (target_path.clone(), *is_folder),
            _ => return Ok(()),
        };

        self.async_op = AsyncOp::Deleting(DeleteProgress {
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

        self.async_op = AsyncOp::None;

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
            .browsing()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?
            .object_store
            .clone();

        let object_path = ObjectPath::from(path);

        // Update progress
        if let AsyncOp::Deleting(progress) = &mut self.async_op {
            progress.current_file = path.to_string();
            progress.total_files = 1;
        }

        object_store.delete(&object_path).await?;

        // Update progress
        if let AsyncOp::Deleting(progress) = &mut self.async_op {
            progress.files_completed = 1;
        }

        Ok(())
    }

    /// Delete all blobs in a folder (prefix).
    async fn delete_folder(&mut self, prefix: &str) -> color_eyre::Result<()> {
        let object_store = self
            .browsing()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?
            .object_store
            .clone();

        let prefix_path = ObjectPath::from(prefix);

        // List all files in the folder
        let stream = object_store.list(Some(&prefix_path));
        let objects: Vec<_> = stream.collect().await;

        let total_files = objects.len();

        // Update progress
        if let AsyncOp::Deleting(progress) = &mut self.async_op {
            progress.total_files = total_files;
        }

        let mut files_completed = 0;

        for result in objects {
            match result {
                Ok(meta) => {
                    let file_path = meta.location.as_ref();

                    // Update progress
                    if let AsyncOp::Deleting(progress) = &mut self.async_op {
                        progress.current_file = file_path.to_string();
                    }

                    // Delete the file
                    if let Err(e) = object_store.delete(&meta.location).await {
                        if let AsyncOp::Deleting(progress) = &mut self.async_op {
                            progress.error_message =
                                Some(format!("Failed to delete {file_path}: {e}"));
                        }
                        // Continue with other files even if one fails
                    } else {
                        files_completed += 1;

                        // Update progress
                        if let AsyncOp::Deleting(progress) = &mut self.async_op {
                            progress.files_completed = files_completed;
                        }
                    }
                }
                Err(e) => {
                    if let AsyncOp::Deleting(progress) = &mut self.async_op {
                        progress.error_message = Some(format!("Failed to list file: {e}"));
                    }
                }
            }
        }

        Ok(())
    }

    /// Filter files based on search query.
    pub fn filter_files(&mut self) {
        let query = match &self.search {
            Search::Files { query, .. } => query.clone(),
            _ => return,
        };
        self.apply_file_search(&query);
        if let Some(state) = self.browsing_mut() {
            state.selected_index = 0;
        }
    }

    fn apply_file_search(&mut self, query: &str) {
        let (all_files, all_file_items) = match &self.search {
            Search::Files {
                all_files,
                all_file_items,
                ..
            } => (all_files.clone(), all_file_items.clone()),
            _ => return,
        };

        if let Some(state) = self.browsing_mut() {
            if query.is_empty() {
                state.files = all_files;
                state.file_items = all_file_items;
            } else {
                let filtered_items: Vec<FileItem> = all_file_items
                    .iter()
                    .filter(|item| {
                        item.actual_name
                            .to_lowercase()
                            .contains(&query.to_lowercase())
                    })
                    .cloned()
                    .collect();

                state.file_items.clone_from(&filtered_items);
                state.files = filtered_items
                    .iter()
                    .map(|item| item.display_name.clone())
                    .collect();
            }
            state.selected_index = 0;
        }
    }

    fn apply_container_search(&mut self, query: &str) {
        let all_containers = match &self.search {
            Search::Containers { all_containers, .. } => all_containers.clone(),
            _ => return,
        };

        if query.is_empty() {
            self.containers = all_containers;
        } else {
            self.containers = all_containers
                .iter()
                .filter(|container| {
                    container
                        .name
                        .to_lowercase()
                        .contains(&query.to_lowercase())
                })
                .cloned()
                .collect();
        }
        self.selected_container_index = 0;
    }

    /// Load the list of containers from Azure Storage.
    async fn load_containers(&mut self) -> color_eyre::Result<()> {
        self.async_op = AsyncOp::LoadingContainers;
        self.error_message = None;
        self.success_message = None;

        match self.list_containers().await {
            Ok(containers) => {
                let search_query = match &self.search {
                    Search::Containers { query, .. } => Some(query.clone()),
                    _ => None,
                };

                if let Search::Containers { all_containers, .. } = &mut self.search {
                    all_containers.clone_from(&containers);
                }

                if let Some(query) = search_query {
                    if !query.is_empty() {
                        self.apply_container_search(&query);
                    } else {
                        self.containers = containers;
                        self.selected_container_index = 0;
                    }
                } else {
                    self.containers = containers;
                    self.selected_container_index = 0;
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to list containers: {e}"));
            }
        }

        self.async_op = AsyncOp::None;
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
                let _ = write!(url, "&marker={}", urlencoding::encode(marker));
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

        self.session = Session::Browsing(BrowsingState {
            object_store: Arc::new(azure_client),
            current_path: String::new(),
            files: Vec::new(),
            file_items: Vec::new(),
            selected_index: 0,
        });
        self.search = Search::Inactive;

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
        self.search = Search::Containers {
            query: String::new(),
            all_containers: self.containers.clone(),
        };
        self.error_message = None;
        self.success_message = None;
    }

    /// Exit container search mode and restore original container list.
    pub fn exit_container_search_mode(&mut self) {
        if let Search::Containers { all_containers, .. } = &self.search {
            self.containers = all_containers.clone();
            self.selected_container_index = 0;
        }
        self.search = Search::Inactive;
    }

    /// Handle key events when in container search mode.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but uses `Result` for API consistency.
    pub fn handle_container_search_key_event(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<()> {
        let query = match &mut self.search {
            Search::Containers { query, .. } => query,
            _ => return Ok(()),
        };

        match key_event.code {
            KeyCode::Esc => {
                self.exit_container_search_mode();
            }
            KeyCode::Enter => {
                // Exit search mode but keep the filtered results
                self.search = Search::Inactive;
            }
            KeyCode::Backspace => {
                query.pop();
                let current = query.clone();
                self.apply_container_search(&current);
            }
            KeyCode::Up if key_event.modifiers == KeyModifiers::CONTROL => {
                self.move_container_up();
            }
            KeyCode::Down if key_event.modifiers == KeyModifiers::CONTROL => {
                self.move_container_down();
            }
            KeyCode::Char(c) => {
                query.push(c);
                let current = query.clone();
                self.apply_container_search(&current);
            }
            _ => {}
        }
        Ok(())
    }

    /// Filter containers based on search query.
    pub fn filter_containers(&mut self) {
        let query = match &self.search {
            Search::Containers { query, .. } => query.clone(),
            _ => return,
        };
        self.apply_container_search(&query);
    }

    /// Show information about the currently selected blob or folder.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching blob/folder metadata fails.
    pub async fn show_blob_info(&mut self) -> color_eyre::Result<()> {
        let selected_file = match self.browsing() {
            Some(state) => {
                if state.files.is_empty() {
                    return Ok(());
                }
                state.files[state.selected_index].clone()
            }
            None => return Ok(()),
        };

        if selected_file.is_empty() {
            return Ok(());
        }
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);

        let is_folder = selected_file.starts_with(&folder_prefix);
        let name = if is_folder {
            selected_file
                .strip_prefix(&folder_prefix)
                .unwrap_or(&selected_file)
        } else {
            selected_file
                .strip_prefix(&file_prefix)
                .unwrap_or(&selected_file)
        };

        let info = if is_folder {
            // Get folder information (blob count and total size)
            self.get_folder_info(name).await?
        } else {
            // Get individual blob information
            self.get_blob_info(name).await?
        };

        self.modal = Modal::BlobInfo { info };
        Ok(())
    }

    /// Get information about a folder (blob count and total size).
    async fn get_folder_info(&self, folder_name: &str) -> color_eyre::Result<BlobInfo> {
        let browsing = self
            .browsing()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;
        let object_store = browsing.object_store.clone();

        let folder_path = if browsing.current_path.is_empty() {
            format!("{folder_name}/")
        } else if browsing.current_path.ends_with('/') {
            format!("{}{}/", browsing.current_path, folder_name)
        } else {
            format!("{}/{}/", browsing.current_path, folder_name)
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

        Ok(BlobInfo::Folder {
            name: folder_name.to_string(),
            blob_count,
            total_size,
        })
    }

    /// Get information about a specific blob.
    async fn get_blob_info(&self, blob_name: &str) -> color_eyre::Result<BlobInfo> {
        let browsing = self
            .browsing()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;
        let object_store = browsing.object_store.clone();

        let blob_path = if browsing.current_path.is_empty() {
            blob_name.to_string()
        } else if browsing.current_path.ends_with('/') {
            format!("{}{}", browsing.current_path, blob_name)
        } else {
            format!("{}/{}", browsing.current_path, blob_name)
        };

        let object_path = ObjectPath::from(blob_path.as_str());

        match object_store.head(&object_path).await {
            Ok(meta) => Ok(BlobInfo::File {
                name: blob_name.to_string(),
                size: meta.size,
                last_modified: meta
                    .last_modified
                    .format("%Y-%m-%d %H:%M:%S UTC")
                    .to_string(),
                etag: meta.e_tag.clone(),
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
        let (selected_file, current_path) = match self.browsing() {
            Some(state) => {
                if state.files.is_empty() {
                    return Ok(());
                }
                (
                    state.files[state.selected_index].clone(),
                    state.current_path.clone(),
                )
            }
            None => return Ok(()),
        };

        if selected_file.is_empty() {
            return Ok(());
        }
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);
        let selected_file = selected_file.as_str();

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
        let full_path = if current_path.is_empty() {
            if is_folder {
                format!("{item_name}/")
            } else {
                item_name.to_string()
            }
        } else if current_path.ends_with('/') {
            if is_folder {
                format!("{}{}/", current_path, item_name)
            } else {
                format!("{}{}", current_path, item_name)
            }
        } else if is_folder {
            format!("{}/{}/", current_path, item_name)
        } else {
            format!("{}/{}", current_path, item_name)
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
    pub fn show_download_picker(&mut self) {
        if let Some(state) = self.browsing() {
            if state.files.is_empty() {
                return;
            }
        } else {
            return;
        }

        self.modal = Modal::DownloadPicker { destination: None };
    }

    /// Start the download process for the selected file or folder.
    ///
    /// # Errors
    ///
    /// Returns an error if the download operation fails.
    pub async fn start_download(&mut self) -> color_eyre::Result<()> {
        let destination = match &self.modal {
            Modal::DownloadPicker { destination } => destination.clone(),
            _ => None,
        };

        let selected_file = match self.browsing() {
            Some(state) => {
                if state.files.is_empty() {
                    return Ok(());
                }
                state.files[state.selected_index].clone()
            }
            None => return Ok(()),
        };

        if selected_file.is_empty() || destination.is_none() {
            return Ok(());
        }

        let destination = destination.expect("destination checked");
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

        self.async_op = AsyncOp::Downloading(DownloadProgress {
            current_file: String::new(),
            files_completed: 0,
            total_files: 0,
            bytes_downloaded: 0,
            total_bytes: None,
            error_message: None,
        });
        self.close_modal();

        if is_folder {
            self.download_folder(&name, &destination).await?;
        } else {
            self.download_file(&name, &destination).await?;
        }

        self.async_op = AsyncOp::None;
        Ok(())
    }

    /// Download a single file.
    async fn download_file(
        &mut self,
        file_name: &str,
        destination: &Path,
    ) -> color_eyre::Result<()> {
        let browsing = self
            .browsing()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;
        let object_store = browsing.object_store.clone();

        let blob_path = if browsing.current_path.is_empty() {
            file_name.to_string()
        } else if browsing.current_path.ends_with('/') {
            format!("{}{}", browsing.current_path, file_name)
        } else {
            format!("{}/{}", browsing.current_path, file_name)
        };

        let object_path = ObjectPath::from(blob_path.as_str());

        // Initialize progress
        self.async_op = AsyncOp::Downloading(DownloadProgress {
            current_file: file_name.to_string(),
            files_completed: 0,
            total_files: 1,
            bytes_downloaded: 0,
            total_bytes: None,
            error_message: None,
        });

        // Get file metadata for total size
        if let Ok(meta) = object_store.head(&object_path).await
            && let AsyncOp::Downloading(progress) = &mut self.async_op
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

                if let AsyncOp::Downloading(progress) = &mut self.async_op {
                    progress.bytes_downloaded = bytes.len() as u64;
                    progress.files_completed = 1;
                }
            }
            Err(e) => {
                if let AsyncOp::Downloading(progress) = &mut self.async_op {
                    progress.error_message = Some(format!("Failed to download {file_name}: {e}"));
                }
                return Err(color_eyre::eyre::eyre!("Download failed: {}", e));
            }
        }

        Ok(())
    }

    /// Download all files in a folder.
    async fn download_folder(
        &mut self,
        folder_name: &str,
        destination: &Path,
    ) -> color_eyre::Result<()> {
        let browsing = self
            .browsing()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?;
        let object_store = browsing.object_store.clone();

        let folder_path = if browsing.current_path.is_empty() {
            format!("{folder_name}/")
        } else if browsing.current_path.ends_with('/') {
            format!("{}{}/", browsing.current_path, folder_name)
        } else {
            format!("{}/{}/", browsing.current_path, folder_name)
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
        self.async_op = AsyncOp::Downloading(DownloadProgress {
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
                    if let AsyncOp::Downloading(progress) = &mut self.async_op {
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
                            if let AsyncOp::Downloading(progress) = &mut self.async_op {
                                progress.files_completed = files_completed;
                                progress.bytes_downloaded = total_bytes_downloaded;
                            }
                        }
                        Err(e) => {
                            if let AsyncOp::Downloading(progress) = &mut self.async_op {
                                progress.error_message =
                                    Some(format!("Failed to download {relative_path}: {e}"));
                            }
                            // Continue with other files even if one fails
                        }
                    }
                }
                Err(e) => {
                    if let AsyncOp::Downloading(progress) = &mut self.async_op {
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
        if self.is_modal_download_picker() {
            // Use the file dialog to pick a destination folder
            let file_dialog = rfd::FileDialog::new();

            // Run the file dialog in a spawn_blocking since it's blocking
            let path_result = tokio::task::spawn_blocking(move || file_dialog.pick_folder()).await;

            match path_result {
                Ok(Some(path)) => {
                    self.modal = Modal::DownloadPicker {
                        destination: Some(path),
                    };
                    self.start_download().await?;
                }
                Ok(None) => {
                    // User cancelled the dialog
                    self.close_modal();
                }
                Err(e) => {
                    self.close_modal();
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
        let (selected_file, current_path) = match self.browsing() {
            Some(state) => {
                if state.files.is_empty() {
                    return Ok(());
                }
                (
                    state.files[state.selected_index].clone(),
                    state.current_path.clone(),
                )
            }
            None => return Ok(()),
        };

        if selected_file.is_empty() {
            return Ok(());
        }
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);

        // Check if it's a folder
        if selected_file.starts_with(&folder_prefix) {
            self.preview_error = Some("Cannot preview folders".to_string());
            self.ui.show_preview = true;
            return Ok(());
        }

        let name = selected_file
            .strip_prefix(&file_prefix)
            .unwrap_or(&selected_file);

        // Check file type
        let file_type = PreviewFileType::from_extension(name);
        if !file_type.is_supported() {
            self.preview_error = Some(
                "Unsupported file type. Preview supports: CSV, TSV, JSON, Parquet, and text files"
                    .to_string(),
            );
            self.preview_file_type = Some(file_type);
            self.ui.show_preview = true;
            return Ok(());
        }

        self.preview_file_type = Some(file_type.clone());
        self.ui.is_loading_preview = true;
        self.ui.show_preview = true;
        self.preview_error = None;
        self.preview_data = None;
        self.preview_scroll = (0, 0);
        self.preview_selected_row = 0;

        // Fetch file content (up to MAX_PREVIEW_BYTES)
        let object_store = self
            .browsing()
            .ok_or_else(|| color_eyre::eyre::eyre!("No container selected"))?
            .object_store
            .clone();

        let blob_path = if current_path.is_empty() {
            name.to_string()
        } else if current_path.ends_with('/') {
            format!("{}{}", current_path, name)
        } else {
            format!("{}/{}", current_path, name)
        };

        let object_path = ObjectPath::from(blob_path.as_str());

        // For Parquet files, we need to fetch the footer from the end of the file
        if file_type == PreviewFileType::Parquet {
            // First, get the file size
            let head_result = object_store.head(&object_path).await;
            match head_result {
                Ok(meta) => {
                    let file_size = meta.size;
                    // Fetch the last MAX_PARQUET_PREVIEW_BYTES bytes (footer is at the end)
                    let start = file_size.saturating_sub(MAX_PARQUET_PREVIEW_BYTES as u64);
                    let get_result = object_store.get_range(&object_path, start..file_size).await;

                    self.ui.is_loading_preview = false;

                    match get_result {
                        Ok(bytes) => match parse_parquet_schema(&bytes, Some(file_size)) {
                            Ok(data) => {
                                self.preview_data = Some(data);
                            }
                            Err(e) => {
                                self.preview_error = Some(e);
                            }
                        },
                        Err(e) => {
                            self.preview_error = Some(format!("Failed to fetch file: {e}"));
                        }
                    }
                }
                Err(e) => {
                    self.ui.is_loading_preview = false;
                    self.preview_error = Some(format!("Failed to get file info: {e}"));
                }
            }
            return Ok(());
        }

        // For other file types, fetch from the beginning
        let get_result = object_store
            .get_range(&object_path, 0..(MAX_PREVIEW_BYTES as u64))
            .await;

        self.ui.is_loading_preview = false;

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
        self.ui.show_preview = false;
        self.preview_data = None;
        self.preview_file_type = None;
        self.preview_error = None;
        self.preview_scroll = (0, 0);
        self.preview_selected_row = 0;
        self.ui.is_loading_preview = false;
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
            Some(PreviewData::Text(text)) => text.content.lines().count().saturating_sub(1),
            Some(PreviewData::ParquetSchema(schema)) => {
                // Metadata lines + schema fields + some padding
                (7 + schema.fields.len()).saturating_sub(1)
            }
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

#[cfg(test)]
mod tests {
    use super::{App, AsyncOp, BrowsingState, EntryKind, Modal, Search, Session, SortCriteria};
    use crate::event::EventHandler;
    use crate::terminal_icons::detect_terminal_icons;

    fn test_app() -> App {
        App {
            running: true,
            events: EventHandler::new(),
            session: Session::Selecting,
            storage_account: "test-account".to_string(),
            access_key: "test-key".to_string(),
            containers: Vec::new(),
            selected_container_index: 0,
            async_op: AsyncOp::None,
            error_message: None,
            success_message: None,
            search: Search::Inactive,
            icons: detect_terminal_icons(),
            modal: Modal::None,
            ui: UiToggles {
                show_preview: false,
                is_loading_preview: false,
            },
            sort_criteria: SortCriteria::Name,
            preview_data: None,
            preview_file_type: None,
            preview_scroll: (0, 0),
            preview_error: None,
            preview_selected_row: 0,
        }
    }

    #[test]
    fn open_clone_dialog_sets_modal_data() {
        let mut app = test_app();
        app.session = Session::Browsing(BrowsingState {
            object_store: std::sync::Arc::new(object_store::memory::InMemory::new()),
            current_path: String::new(),
            files: vec![format!("{} file.txt", app.icons.file)],
            file_items: Vec::new(),
            selected_index: 0,
        });

        app.open_clone_dialog();

        match app.modal {
            Modal::Clone {
                input,
                original_path,
                is_folder,
            } => {
                assert_eq!(input, "file.txt");
                assert_eq!(original_path, "file.txt");
                assert!(!is_folder);
            }
            _ => panic!("Expected clone dialog modal"),
        }
    }

    #[test]
    fn open_delete_dialog_sets_modal_data_for_folder() {
        let mut app = test_app();
        app.session = Session::Browsing(BrowsingState {
            object_store: std::sync::Arc::new(object_store::memory::InMemory::new()),
            current_path: String::new(),
            files: vec![format!("{} logs", app.icons.folder)],
            file_items: Vec::new(),
            selected_index: 0,
        });

        app.open_delete_dialog();

        match app.modal {
            Modal::DeleteConfirm {
                input,
                target_path,
                target_name,
                is_folder,
            } => {
                assert_eq!(input, "");
                assert_eq!(target_path, "logs/");
                assert_eq!(target_name, "logs");
                assert!(is_folder);
            }
            _ => panic!("Expected delete confirm modal"),
        }
    }

    #[test]
    fn show_download_picker_sets_modal() {
        let mut app = test_app();
        app.session = Session::Browsing(BrowsingState {
            object_store: std::sync::Arc::new(object_store::memory::InMemory::new()),
            current_path: String::new(),
            files: vec![format!("{} report.csv", app.icons.file)],
            file_items: Vec::new(),
            selected_index: 0,
        });

        app.show_download_picker();

        match app.modal {
            Modal::DownloadPicker { destination } => {
                assert!(destination.is_none());
            }
            _ => panic!("Expected download picker modal"),
        }
    }

    #[test]
    fn async_op_helpers_reflect_state() {
        let mut app = test_app();

        app.async_op = AsyncOp::LoadingContainers;
        assert!(app.is_loading_containers());
        assert!(!app.is_loading_files());

        app.async_op = AsyncOp::LoadingFiles;
        assert!(app.is_loading_files());
        assert!(!app.is_loading_containers());

        app.async_op = AsyncOp::Downloading(super::DownloadProgress {
            current_file: "a".to_string(),
            files_completed: 0,
            total_files: 1,
            bytes_downloaded: 0,
            total_bytes: None,
            error_message: None,
        });
        assert!(app.is_downloading());

        app.async_op = AsyncOp::Cloning(super::CloneProgress {
            current_file: "a".to_string(),
            files_completed: 0,
            total_files: 1,
            error_message: None,
        });
        assert!(app.is_cloning());

        app.async_op = AsyncOp::Deleting(super::DeleteProgress {
            current_file: "a".to_string(),
            files_completed: 0,
            total_files: 1,
            error_message: None,
        });
        assert!(app.is_deleting());
    }

    #[test]
    fn enter_and_exit_container_search_restores_list() {
        let mut app = test_app();
        app.containers = vec![
            super::ContainerInfo {
                name: "alpha".to_string(),
            },
            super::ContainerInfo {
                name: "beta".to_string(),
            },
        ];

        app.enter_container_search_mode();

        match &app.search {
            Search::Containers { query, all_containers } => {
                assert!(query.is_empty());
                assert_eq!(all_containers.len(), 2);
            }
            _ => panic!("Expected container search state"),
        }

        app.exit_container_search_mode();
        assert!(matches!(app.search, Search::Inactive));
        assert_eq!(app.containers.len(), 2);
    }

    #[test]
    fn file_search_filters_and_exit_restores() {
        let mut app = test_app();
        app.session = Session::Browsing(BrowsingState {
            object_store: std::sync::Arc::new(object_store::memory::InMemory::new()),
            current_path: String::new(),
            files: Vec::new(),
            file_items: Vec::new(),
            selected_index: 0,
        });
        let file_items = vec![
            super::FileItem {
                display_name: "file_a".to_string(),
                actual_name: "file_a".to_string(),
                kind: super::EntryKind::File,
                size: None,
                last_modified: None,
                created: None,
            },
            super::FileItem {
                display_name: "file_b".to_string(),
                actual_name: "file_b".to_string(),
                kind: super::EntryKind::File,
                size: None,
                last_modified: None,
                created: None,
            },
        ];
        if let Session::Browsing(state) = &mut app.session {
            state.file_items = file_items.clone();
            state.files = state
                .file_items
                .iter()
                .map(|item| item.display_name.clone())
                .collect();
        }

        app.enter_search_mode();
        if let Search::Files { query, .. } = &mut app.search {
            query.push('a');
        }
        app.apply_file_search("a");

        if let Session::Browsing(state) = &app.session {
            assert_eq!(state.files, vec!["file_a".to_string()]);
        }

        app.exit_search_mode();
        if let Session::Browsing(state) = &app.session {
            assert_eq!(state.files.len(), 2);
        }
        assert!(matches!(app.search, Search::Inactive));
    }

    #[test]
    fn entry_kind_folder_detection() {
        assert!(matches!(EntryKind::Folder, EntryKind::Folder));
        assert!(!matches!(EntryKind::Folder, EntryKind::File));
        assert!(matches!(EntryKind::File, EntryKind::File));
        assert!(!matches!(EntryKind::File, EntryKind::Folder));
    }
}
