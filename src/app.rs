use crate::{
    favorites::Favorites,
    preview::{
        MAX_PARQUET_PREVIEW_BYTES, MAX_PARQUET_TABLE_PREVIEW_BYTES, MAX_PREVIEW_BYTES,
        ParquetSchemaPreview, PreviewData, PreviewFileType, TablePreview, parse_parquet_schema,
        parse_parquet_table, parse_preview,
    },
    task::{Reporter, TaskMessage, Tasks},
    terminal_icons::{IconSet, detect_terminal_icons},
};
use arboard::Clipboard;
use chrono::Utc;
use futures::{Stream, StreamExt, TryStreamExt};
use object_store::{
    ObjectStore, ObjectStoreExt,
    azure::{AzureCredential, AzureCredentialProvider, MicrosoftAzureBuilder},
    path::Path as ObjectPath,
};
use ratatui::{
    DefaultTerminal, Terminal,
    backend::Backend,
    crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
};
use regex::Regex;
use reqwest;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

/// How often the spinner advances while an operation runs.
const SPINNER_TICK: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortCriteria {
    Name,
    DateModified,
    DateCreated,
    Size,
}

impl SortCriteria {
    /// Blob storage keeps no metadata for folders, so every criterion except name
    /// needs a recursive scan of each folder's blobs.
    fn needs_folder_stats(self) -> bool {
        !matches!(self, Self::Name)
    }
}

#[derive(Debug, Clone)]
pub struct BrowsingState {
    pub object_store: Arc<dyn ObjectStore>,
    directory: LoadedDirectory,
    pub selected_index: usize,
}

/// A prefix and its successfully fetched entries are installed together.
#[derive(Debug, Clone)]
pub(crate) struct LoadedDirectory {
    current_path: String,
    files: Vec<String>,
    file_items: Vec<FileItem>,
    /// Whether folder items carry their newest blob time and total blob size.
    has_folder_stats: bool,
}

impl BrowsingState {
    pub(crate) fn current_path(&self) -> &str {
        &self.directory.current_path
    }

    pub(crate) fn files(&self) -> &[String] {
        &self.directory.files
    }

    pub(crate) fn file_items(&self) -> &[FileItem] {
        &self.directory.file_items
    }
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

/// Blob storage keeps no metadata for folders, so this is aggregated from their blobs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FolderStats {
    size: u64,
    last_modified: chrono::DateTime<chrono::Utc>,
}

/// What a finished directory listing is for, which decides how it is installed.
pub(crate) enum LoadPurpose {
    /// Carries the message prefix to use if the listing fails.
    Navigate(&'static str),
    Refresh,
    /// Show what the operation left behind, keeping its outcome message.
    AfterOperation(Operation),
    OpenContainer(Arc<dyn ObjectStore>),
}

/// An operation on blobs whose outcome is reported as a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operation {
    Clone,
    Delete,
    Download,
    Upload,
}

impl Operation {
    fn failure(self) -> &'static str {
        match self {
            Self::Clone => "Clone failed",
            Self::Delete => "Delete failed",
            Self::Download => "Download failed",
            Self::Upload => "Upload failed",
        }
    }

    fn refresh_failure(self) -> &'static str {
        match self {
            Self::Clone => "Refresh failed after clone",
            Self::Delete => "Refresh failed after delete",
            Self::Download => "Refresh failed after download",
            Self::Upload => "Refresh failed after upload",
        }
    }

    /// Whether the container may differ afterward, even if the operation failed
    /// or was canceled partway.
    fn changes_container(self) -> bool {
        !matches!(self, Self::Download)
    }
}

/// The result of a background task, applied to [`App`] on the event loop.
pub(crate) enum TaskEvent {
    DirectoryLoaded {
        purpose: LoadPurpose,
        result: color_eyre::Result<LoadedDirectory>,
    },
    FolderStats {
        /// The prefix that was scanned.
        path: String,
        criteria: SortCriteria,
        result: color_eyre::Result<HashMap<String, FolderStats>>,
    },
    PreviewLoaded(Box<LoadedPreview>),
    BlobInfo(color_eyre::Result<BlobInfo>),
    ContainersLoaded(Result<Vec<ContainerInfo>, String>),
    /// The native dialog closed; `None` means it was dismissed.
    DownloadDestinationPicked(Result<Option<PathBuf>, tokio::task::JoinError>),
    UploadSourcePicked(Result<Option<PathBuf>, tokio::task::JoinError>),
    /// Carries the success message to show.
    OperationFinished {
        operation: Operation,
        result: color_eyre::Result<String>,
    },
}

/// Everything a preview fetch produces, installed together when it finishes.
#[derive(Default)]
pub(crate) struct LoadedPreview {
    data: Option<PreviewData>,
    error: Option<String>,
    parquet_mode: Option<ParquetPreviewMode>,
    parquet_table: Option<TablePreview>,
    parquet_schema: Option<ParquetSchemaPreview>,
}

#[derive(Debug, Clone)]
pub enum Modal {
    None,
    BlobInfo {
        info: BlobInfo,
    },
    DownloadPicker {
        destination: Option<PathBuf>,
    },
    UploadPicker,
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
    pub preview: PreviewPanel,
}

/// A visible preview can accompany browsing or take keyboard focus for scrolling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PreviewPanel {
    Hidden,
    #[default]
    Browsing,
    Focused,
}

impl PreviewPanel {
    #[must_use]
    pub fn is_visible(self) -> bool {
        self != Self::Hidden
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewTarget {
    container: usize,
    path: String,
    name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParquetPreviewMode {
    Table,
    Metadata,
}

/// Application.
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Current application session.
    pub session: Session,
    /// Azure Storage Account name.
    pub storage_account: String,
    /// Shared Azure CLI credentials, including the token refresh cache.
    credentials: AzureCredentialProvider,
    favorites: Favorites,
    favorites_path: Option<PathBuf>,
    /// List of available containers (may be filtered during search).
    pub containers: Vec<ContainerInfo>,
    /// Full list of all containers from Azure (never filtered).
    pub all_containers: Vec<ContainerInfo>,
    /// Currently selected container index.
    pub selected_container_index: usize,
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
    /// Current parquet preview mode, if parquet preview is active.
    pub parquet_preview_mode: Option<ParquetPreviewMode>,
    /// Cached parquet table preview data for fast toggling.
    pub parquet_table_data: Option<TablePreview>,
    /// Cached parquet metadata preview data for fast toggling.
    pub parquet_schema_data: Option<ParquetSchemaPreview>,
    /// The running background operation, if any, and its result channel.
    pub(crate) tasks: Tasks,
    /// Automatic previews are replaceable and never block file-list actions.
    pub(crate) preview_tasks: Tasks,
    preview_target: Option<PreviewTarget>,
    /// Folder scans of the open container by prefix, kept so a date or size sort
    /// does not rescan on every visit. Dropped on refresh and after any change.
    folder_stats_cache: HashMap<String, HashMap<String, FolderStats>>,
    /// Created on first copy and kept for the app's lifetime. On X11 the copying
    /// process must answer paste requests itself, so dropping the clipboard right
    /// after writing loses the text when no clipboard manager is running (e.g.
    /// XFCE over xrdp).
    clipboard: Option<Clipboard>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("running", &self.running)
            .field("session", &self.session)
            .field("storage_account", &self.storage_account)
            .field("containers", &self.containers)
            .field("all_containers", &self.all_containers)
            .field("selected_container_index", &self.selected_container_index)
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
            .field("parquet_preview_mode", &self.parquet_preview_mode)
            .finish_non_exhaustive()
    }
}

impl App {
    /// Constructs a new instance of [`App`] and starts loading its containers.
    ///
    /// Must be called from within a Tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if the Azure client cannot be configured.
    pub fn new(storage_account: String) -> color_eyre::Result<Self> {
        // object_store exposes its CLI provider through a container client. Building
        // this client makes no request; $root need not exist to obtain the provider.
        let credentials = MicrosoftAzureBuilder::new()
            .with_account(&storage_account)
            .with_container_name("$root")
            .with_use_azure_cli(true)
            .build()?
            .credentials()
            .clone();
        let mut app = Self {
            running: true,
            session: Session::Selecting,
            storage_account,
            credentials,
            favorites: Favorites::default(),
            favorites_path: Favorites::path().ok(),
            containers: Vec::new(),
            all_containers: Vec::new(),
            selected_container_index: 0,
            error_message: None,
            success_message: None,
            search: Search::Inactive,
            icons: detect_terminal_icons(),
            modal: Modal::None,
            ui: UiToggles {
                preview: PreviewPanel::default(),
            },
            sort_criteria: SortCriteria::Name,
            preview_data: None,
            preview_file_type: None,
            preview_scroll: (0, 0),
            preview_error: None,
            preview_selected_row: 0,
            parquet_preview_mode: None,
            parquet_table_data: None,
            parquet_schema_data: None,
            tasks: Tasks::new(),
            preview_tasks: Tasks::new(),
            preview_target: None,
            folder_stats_cache: HashMap::new(),
            clipboard: None,
        };

        app.load_containers();

        Ok(app)
    }

    /// Run the application's main loop.
    ///
    /// # Errors
    ///
    /// Returns an error if terminal drawing or event handling fails.
    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> color_eyre::Result<()> {
        self.run_loop(terminal, EventStream::new()).await
    }

    /// Redraw after each terminal event, task message, or spinner tick, so input
    /// stays responsive while an operation runs.
    async fn run_loop<B>(
        &mut self,
        terminal: &mut Terminal<B>,
        mut events: impl Stream<Item = std::io::Result<Event>> + Unpin,
    ) -> color_eyre::Result<()>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
    {
        let mut tick = tokio::time::interval(SPINNER_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        while self.running {
            self.sync_preview();
            terminal.draw(|frame| frame.render_widget(&*self, frame.area()))?;
            tokio::select! {
                event = events.next() => match event {
                    Some(Ok(Event::Key(key_event))) => self.handle_key_event(key_event)?,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(error.into()),
                    None => break,
                },
                Some(message) = self.tasks.recv() => self.apply_task_message(message),
                Some(message) = self.preview_tasks.recv() => {
                    if let Some(TaskEvent::PreviewLoaded(preview)) = self.preview_tasks.accept(message) {
                        self.install_preview(*preview);
                    }
                },
                _ = tick.tick(), if self.tasks.is_running() => self.tasks.advance_spinner(),
            }
        }
        Ok(())
    }

    /// Apply a finished task's result, unless the task was canceled or replaced.
    fn apply_task_message(&mut self, message: TaskMessage) {
        let Some(event) = self.tasks.accept(message) else {
            return;
        };
        match event {
            TaskEvent::DirectoryLoaded { purpose, result } => self.finish_load(purpose, result),
            TaskEvent::FolderStats {
                path,
                criteria,
                result,
            } => self.finish_folder_stats(path, criteria, result),
            TaskEvent::PreviewLoaded(preview) => {
                self.install_preview(*preview);
            }
            TaskEvent::BlobInfo(Ok(info)) => self.modal = Modal::BlobInfo { info },
            TaskEvent::BlobInfo(Err(e)) => {
                self.error_message = Some(format!("Failed to get blob info: {e}"));
            }
            TaskEvent::ContainersLoaded(result) => self.finish_load_containers(result),
            TaskEvent::DownloadDestinationPicked(Ok(Some(path))) => {
                self.modal = Modal::DownloadPicker {
                    destination: Some(path),
                };
                self.start_download();
            }
            // User cancelled the dialog
            TaskEvent::DownloadDestinationPicked(Ok(None)) => self.close_modal(),
            TaskEvent::DownloadDestinationPicked(Err(e)) => {
                self.close_modal();
                self.error_message = Some(format!("Failed to open file dialog: {e}"));
            }
            TaskEvent::UploadSourcePicked(Ok(Some(path))) => self.upload_file(path),
            TaskEvent::UploadSourcePicked(Ok(None)) => {}
            TaskEvent::UploadSourcePicked(Err(e)) => {
                self.error_message = Some(format!("Upload failed: {e}"));
            }
            TaskEvent::OperationFinished { operation, result } => {
                self.finish_operation(operation, result);
            }
        }
    }

    fn install_preview(&mut self, preview: LoadedPreview) {
        self.preview_data = preview.data;
        self.preview_error = preview.error;
        self.parquet_preview_mode = preview.parquet_mode;
        self.parquet_table_data = preview.parquet_table;
        self.parquet_schema_data = preview.parquet_schema;
    }

    fn cancel_activity(&mut self) {
        if let Some(label) = self.tasks.cancel()
            && self.error_message.is_none()
        {
            self.success_message = Some(format!("Canceled: {label}"));
        }
        if self.ui.preview == PreviewPanel::Focused {
            self.close_preview();
        }
    }

    /// Handles the key events and updates the state of [`App`].
    ///
    /// Never waits on the network: operations are started here and their results
    /// arrive through [`Self::apply_task_message`].
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but uses `Result` for API consistency.
    #[allow(clippy::too_many_lines)]
    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        // One operation runs at a time, so only cancel and quit are accepted until it ends.
        if self.tasks.is_running() {
            match key_event.code {
                KeyCode::Esc => self.cancel_activity(),
                KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                    self.cancel_activity();
                }
                KeyCode::Char('q') => self.quit(),
                _ => {}
            }
            return Ok(());
        }

        // Handle delete dialog separately
        if self.is_modal_delete_dialog() {
            return self.handle_delete_dialog_key_event(key_event);
        }

        // Handle clone dialog separately
        if self.is_modal_clone_dialog() {
            return self.handle_clone_dialog_key_event(key_event);
        }

        // Handle search mode separately
        if self.is_searching_containers() && self.is_selecting() {
            return self.handle_container_search_key_event(key_event);
        }

        if self.is_searching_files() && self.is_browsing() {
            return self.handle_search_key_event(key_event);
        }

        // Global keys
        match key_event.code {
            KeyCode::Char('q') => {
                self.quit();
                return Ok(());
            }
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.quit();
                return Ok(());
            }
            _ => {}
        }

        // State-specific key handling
        if matches!(self.modal, Modal::UploadPicker) {
            match key_event.code {
                KeyCode::Esc | KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
                    self.close_modal();
                }
                KeyCode::Enter => {
                    self.close_modal();
                    self.error_message = None;
                    self.success_message = None;
                    // The dialog blocks its thread until it closes.
                    self.tasks.spawn("Waiting for file selection", async {
                        TaskEvent::UploadSourcePicked(
                            tokio::task::spawn_blocking(|| {
                                rfd::FileDialog::new()
                                    .set_title("Select file to upload")
                                    .pick_file()
                            })
                            .await,
                        )
                    });
                }
                _ => {}
            }
            return Ok(());
        }

        if self.is_selecting() {
            match key_event.code {
                KeyCode::Esc => {
                    // Only quit at the top level (container selection)
                    self.quit();
                    return Ok(());
                }
                KeyCode::Char('f') => self.toggle_favorite(),
                KeyCode::Char('/') => {
                    self.enter_container_search_mode();
                }
                KeyCode::Up | KeyCode::Char('k') => self.move_container_up(),
                KeyCode::Down | KeyCode::Char('j') => self.move_container_down(),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    if let Err(e) = self.select_container() {
                        self.error_message = Some(format!("Failed to select container: {e}"));
                    }
                }
                KeyCode::Char('r') | KeyCode::F(5) => {
                    self.load_containers();
                }
                _ => {
                    self.error_message = None;
                    self.success_message = None;
                }
            }
        } else if self.is_browsing() {
            // Only a focused file preview captures navigation and action keys.
            if self.ui.preview == PreviewPanel::Focused {
                match key_event.code {
                    KeyCode::Char('p') | KeyCode::Esc | KeyCode::Backspace => self.close_preview(),
                    KeyCode::Tab => self.toggle_parquet_preview_mode(),
                    KeyCode::Up | KeyCode::Char('k') => self.preview_scroll_up(),
                    KeyCode::Down | KeyCode::Char('j') => self.preview_scroll_down(),
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                        self.preview_scroll_right();
                    }
                    KeyCode::Left | KeyCode::Char('h') => self.preview_scroll_left(),
                    _ => {}
                }
                return Ok(());
            }

            match key_event.code {
                KeyCode::Tab
                    if self.ui.preview == PreviewPanel::Browsing
                        && matches!(self.modal, Modal::None) =>
                {
                    self.toggle_parquet_preview_mode();
                }
                KeyCode::Char('/') => {
                    if !self.is_modal_blob_info() {
                        self.enter_search_mode();
                    }
                }
                KeyCode::Char('r') | KeyCode::F(5) => {
                    if !self.is_modal_blob_info() {
                        self.refresh_files();
                    }
                }
                KeyCode::Char('i') => {
                    if !self.is_modal_blob_info() {
                        self.show_blob_info();
                    }
                }
                KeyCode::Char('p') => {
                    if !self.is_modal_blob_info()
                        && !self.is_modal_download_picker()
                        && !self.is_modal_sort_picker()
                        && !self.is_modal_clone_dialog()
                        && !self.is_modal_delete_dialog()
                    {
                        self.load_preview();
                    }
                }
                KeyCode::Char('u') if matches!(self.modal, Modal::None) => {
                    self.modal = Modal::UploadPicker;
                }
                KeyCode::Char('d') => {
                    if !self.is_modal_blob_info()
                        && !self.is_modal_download_picker()
                        && !self.is_modal_sort_picker()
                    {
                        self.show_download_picker();
                    }
                }
                KeyCode::Char('s') => {
                    if !self.is_modal_blob_info()
                        && !self.is_modal_download_picker()
                        && !self.is_modal_sort_picker()
                    {
                        self.modal = Modal::SortPicker;
                    } else if self.is_modal_sort_picker() {
                        // Handle sort selection
                        self.apply_sort(SortCriteria::Size);
                        self.close_modal();
                    }
                }
                KeyCode::Char('n') => {
                    if self.is_modal_sort_picker() {
                        self.apply_sort(SortCriteria::Name);
                        self.close_modal();
                    }
                }
                KeyCode::Char('m') => {
                    if self.is_modal_sort_picker() {
                        self.apply_sort(SortCriteria::DateModified);
                        self.close_modal();
                    }
                }
                KeyCode::Char('t') => {
                    if self.is_modal_sort_picker() {
                        self.apply_sort(SortCriteria::DateCreated);
                        self.close_modal();
                    }
                }
                KeyCode::Char('y') => {
                    if !self.is_modal_blob_info()
                        && !self.is_modal_download_picker()
                        && !self.is_modal_sort_picker()
                        && !self.is_modal_clone_dialog()
                    {
                        // Copy blob path to clipboard
                        if let Err(e) = self.copy_blob_path_to_clipboard() {
                            self.error_message = Some(format!("Failed to copy to clipboard: {e}"));
                        }
                    }
                }
                KeyCode::Char('c') => {
                    if !self.is_modal_blob_info()
                        && !self.is_modal_download_picker()
                        && !self.is_modal_sort_picker()
                        && !self.is_modal_clone_dialog()
                        && !self.is_modal_delete_dialog()
                    {
                        // Open clone dialog
                        self.open_clone_dialog();
                    }
                }
                KeyCode::Char('x') | KeyCode::Delete => {
                    if !self.is_modal_blob_info()
                        && !self.is_modal_download_picker()
                        && !self.is_modal_sort_picker()
                        && !self.is_modal_clone_dialog()
                        && !self.is_modal_delete_dialog()
                    {
                        // Open delete dialog
                        self.open_delete_dialog();
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if !self.is_modal_blob_info() && !self.is_modal_sort_picker() {
                        self.move_up();
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !self.is_modal_blob_info() && !self.is_modal_sort_picker() {
                        self.move_down();
                    }
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                    if self.is_modal_download_picker() {
                        self.confirm_download();
                    } else if !self.is_modal_blob_info() && !self.is_modal_sort_picker() {
                        self.enter_directory();
                    }
                }
                KeyCode::Left | KeyCode::Char('h') => {
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
                        self.go_up_directory();
                    }
                }
                KeyCode::Esc => {
                    if self.is_modal_download_picker() {
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
                        .is_some_and(|state| !state.directory.current_path.is_empty())
                    {
                        // Go up one directory level if not at container root
                        self.go_up_directory();
                    } else {
                        // At container root, go back to container selection
                        self.session = Session::Selecting;
                        self.containers.clone_from(&self.all_containers);
                        self.selected_container_index = 0;
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
                        self.containers.clone_from(&self.all_containers);
                        self.selected_container_index = 0;
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

    fn is_selecting(&self) -> bool {
        matches!(self.session, Session::Selecting)
    }

    fn is_browsing(&self) -> bool {
        matches!(self.session, Session::Browsing(_))
    }

    pub(crate) fn browsing(&self) -> Option<&BrowsingState> {
        match &self.session {
            Session::Browsing(state) => Some(state),
            Session::Selecting => None,
        }
    }

    fn browsing_mut(&mut self) -> Option<&mut BrowsingState> {
        match &mut self.session {
            Session::Browsing(state) => Some(state),
            Session::Selecting => None,
        }
    }

    pub(crate) fn selected_file_item(&self) -> Option<&FileItem> {
        let state = self.browsing()?;
        state.directory.file_items.get(state.selected_index)
    }

    fn join_blob_path(current_path: &str, name: &str) -> String {
        if current_path.is_empty() {
            name.to_string()
        } else if current_path.ends_with('/') {
            format!("{current_path}{name}")
        } else {
            format!("{current_path}/{name}")
        }
    }

    fn join_folder_path(current_path: &str, name: &str) -> String {
        if current_path.is_empty() {
            format!("{name}/")
        } else if current_path.ends_with('/') {
            format!("{current_path}{name}/")
        } else {
            format!("{current_path}/{name}/")
        }
    }

    fn object_path(path: &str) -> color_eyre::Result<ObjectPath> {
        Ok(ObjectPath::parse(path.trim_end_matches('/'))?)
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

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// List blobs and prefixes with metadata for sorting.
    async fn list_file_items(
        object_store: &dyn ObjectStore,
        icons: &IconSet,
        prefix: &str,
    ) -> color_eyre::Result<Vec<FileItem>> {
        let prefix_path = Self::prefix_path(prefix)?;
        let result = object_store
            .list_with_delimiter(prefix_path.as_ref())
            .await?;
        let mut items = Vec::new();

        // Add "directories" (common prefixes)
        for prefix in result.common_prefixes {
            let name = prefix.as_ref().trim_end_matches('/');
            if let Some(last_part) = name.split('/').next_back() {
                items.push(FileItem {
                    display_name: format!("{folder} {last_part}", folder = icons.folder),
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
                    display_name: format!("{file} {last_part}", file = icons.file),
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

    /// Container roots have no object path.
    fn prefix_path(prefix: &str) -> color_eyre::Result<Option<ObjectPath>> {
        if prefix.is_empty() {
            Ok(None)
        } else {
            Self::object_path(prefix).map(Some)
        }
    }

    /// Aggregate each child folder of `prefix` from the blobs beneath it, at any depth.
    async fn folder_stats(
        object_store: &dyn ObjectStore,
        prefix: Option<&ObjectPath>,
    ) -> color_eyre::Result<HashMap<String, FolderStats>> {
        let mut stats: HashMap<String, FolderStats> = HashMap::new();

        // One recursive listing of the prefix covers every folder, and streaming it
        // avoids holding a large container's full listing in memory.
        let root = ObjectPath::default();
        let mut objects = object_store.list(prefix);
        while let Some(meta) = objects.try_next().await? {
            let Some(mut parts) = meta.location.prefix_match(prefix.unwrap_or(&root)) else {
                continue;
            };
            // Blobs directly under the prefix are files, not folder contents.
            let (Some(folder), Some(_)) = (parts.next(), parts.next()) else {
                continue;
            };
            match stats.get_mut(folder.as_ref()) {
                Some(folder_stats) => {
                    folder_stats.size += meta.size;
                    folder_stats.last_modified = folder_stats.last_modified.max(meta.last_modified);
                }
                None => {
                    stats.insert(
                        folder.as_ref().to_owned(),
                        FolderStats {
                            size: meta.size,
                            last_modified: meta.last_modified,
                        },
                    );
                }
            }
        }
        Ok(stats)
    }

    fn set_folder_stats(items: &mut [FileItem], stats: &HashMap<String, FolderStats>) {
        for item in items
            .iter_mut()
            .filter(|item| item.kind == EntryKind::Folder)
        {
            if let Some(folder_stats) = stats.get(&item.actual_name) {
                item.size = Some(folder_stats.size);
                item.last_modified = Some(folder_stats.last_modified);
            }
        }
    }

    /// Apply sorting to the current file list.
    ///
    /// Date and size sorts need each folder's stats. If they are not cached, the
    /// folders are scanned in the background and the sort is applied when the
    /// scan arrives; it is left unchanged if the scan fails or is canceled.
    pub fn apply_sort(&mut self, criteria: SortCriteria) {
        if criteria.needs_folder_stats()
            && let Some(state) = self.browsing()
            && !state.directory.has_folder_stats
        {
            let path = state.current_path().to_owned();
            let is_folder = |item: &FileItem| item.kind == EntryKind::Folder;
            let has_folders = state.directory.file_items.iter().any(is_folder)
                || matches!(&self.search, Search::Files { all_file_items, .. }
                    if all_file_items.iter().any(is_folder));
            if let Some(stats) = self.folder_stats_cache.get(&path).cloned() {
                self.merge_folder_stats(&stats);
            } else if has_folders {
                let prefix = match Self::prefix_path(&path) {
                    Ok(prefix) => prefix,
                    Err(e) => {
                        self.error_message = Some(format!("Failed to sort: {e}"));
                        return;
                    }
                };
                let object_store = state.object_store.clone();
                self.tasks.spawn("Scanning folders", async move {
                    let result = Self::folder_stats(object_store.as_ref(), prefix.as_ref()).await;
                    TaskEvent::FolderStats {
                        path,
                        criteria,
                        result,
                    }
                });
                return;
            } else {
                self.merge_folder_stats(&HashMap::new());
            }
        }
        self.sort_by(criteria);
    }

    fn finish_folder_stats(
        &mut self,
        path: String,
        criteria: SortCriteria,
        result: color_eyre::Result<HashMap<String, FolderStats>>,
    ) {
        match result {
            Ok(stats) => {
                self.merge_folder_stats(&stats);
                self.folder_stats_cache.insert(path, stats);
                self.sort_by(criteria);
            }
            Err(e) => self.error_message = Some(format!("Failed to sort: {e}")),
        }
    }

    /// Merge stats into the current items rather than relisting, which would drop
    /// a confirmed search filter.
    fn merge_folder_stats(&mut self, stats: &HashMap<String, FolderStats>) {
        if let Some(state) = self.browsing_mut() {
            Self::set_folder_stats(&mut state.directory.file_items, stats);
            state.directory.has_folder_stats = true;
        }
        if let Search::Files { all_file_items, .. } = &mut self.search {
            Self::set_folder_stats(all_file_items, stats);
        }
    }

    /// A listing arrives without folder stats so it can be shown at once; fetch
    /// them now if the current sort depends on them.
    fn sort_new_listing(&mut self) {
        if self.sort_criteria.needs_folder_stats() {
            self.apply_sort(self.sort_criteria);
        }
    }

    fn sort_by(&mut self, criteria: SortCriteria) {
        self.sort_criteria = criteria;

        if let Some(state) = self.browsing_mut()
            && !state.directory.file_items.is_empty()
        {
            Self::sort_file_items_static(&mut state.directory.file_items, criteria);
            // Update the display list
            state.directory.files = state
                .directory
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
    pub fn refresh_files(&mut self) {
        let Some(state) = self.browsing() else {
            return;
        };
        let object_store = state.object_store.clone();
        let path = state.directory.current_path.clone();
        self.folder_stats_cache.remove(&path);
        self.start_load(
            object_store,
            path,
            "Refreshing".to_owned(),
            LoadPurpose::Refresh,
        );
    }

    fn install_refreshed(&mut self, directory: LoadedDirectory) {
        if let Search::Files {
            all_files,
            all_file_items,
            ..
        } = &mut self.search
        {
            all_files.clone_from(&directory.files);
            all_file_items.clone_from(&directory.file_items);
        }
        if let Some(state) = self.browsing_mut() {
            state.directory = directory;
            state.selected_index = 0;
        }
        if let Search::Files { query, .. } = &self.search {
            let query = query.clone();
            self.apply_file_search(&query);
        }
    }

    async fn load_directory(
        object_store: &dyn ObjectStore,
        icons: &IconSet,
        sort_criteria: SortCriteria,
        current_path: String,
    ) -> color_eyre::Result<LoadedDirectory> {
        let mut file_items = Self::list_file_items(object_store, icons, &current_path).await?;
        Self::sort_file_items_static(&mut file_items, sort_criteria);
        let files = file_items
            .iter()
            .map(|item| item.display_name.clone())
            .collect();
        Ok(LoadedDirectory {
            current_path,
            files,
            file_items,
            has_folder_stats: false,
        })
    }

    /// List `path` in the background; nothing changes until the listing arrives.
    fn start_load(
        &mut self,
        object_store: Arc<dyn ObjectStore>,
        path: String,
        label: String,
        purpose: LoadPurpose,
    ) {
        let icons = self.icons.clone();
        let sort_criteria = self.sort_criteria;
        self.tasks.spawn(label, async move {
            let result =
                Self::load_directory(object_store.as_ref(), &icons, sort_criteria, path).await;
            TaskEvent::DirectoryLoaded { purpose, result }
        });
    }

    fn finish_load(&mut self, purpose: LoadPurpose, result: color_eyre::Result<LoadedDirectory>) {
        // Do not change the path, selection, or search snapshot unless listing succeeded.
        let directory = match result {
            Ok(directory) => directory,
            Err(e) => {
                let failure = match purpose {
                    LoadPurpose::Navigate(failure) => failure,
                    LoadPurpose::Refresh => "Refresh failed",
                    LoadPurpose::OpenContainer(_) => "Failed to select container",
                    // The operation's own failure is the more useful message.
                    LoadPurpose::AfterOperation(_) if self.error_message.is_some() => return,
                    LoadPurpose::AfterOperation(operation) => operation.refresh_failure(),
                };
                self.error_message = Some(format!("{failure}: {e}"));
                return;
            }
        };
        match purpose {
            LoadPurpose::Refresh => {
                self.install_refreshed(directory);
                self.error_message = None;
                self.success_message = None;
            }
            LoadPurpose::AfterOperation(_) => self.install_refreshed(directory),
            LoadPurpose::Navigate(_) => {
                if let Some(state) = self.browsing_mut() {
                    state.directory = directory;
                    state.selected_index = 0;
                }
                self.search = Search::Inactive;
                self.error_message = None;
                self.success_message = None;
            }
            LoadPurpose::OpenContainer(object_store) => {
                self.session = Session::Browsing(BrowsingState {
                    object_store,
                    directory,
                    selected_index: 0,
                });
                self.folder_stats_cache.clear();
                self.search = Search::Inactive;
                self.error_message = None;
                self.success_message = None;
            }
        }
        self.preview_target = None;
        self.preview_tasks.cancel();
        self.clear_preview_data();
        self.sort_new_listing();
    }

    fn navigate_to(&mut self, path: String, failure: &'static str) {
        let Some(state) = self.browsing() else {
            return;
        };
        let object_store = state.object_store.clone();
        let label = format!("Loading /{}", path.trim_end_matches('/'));
        self.start_load(object_store, path, label, LoadPurpose::Navigate(failure));
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
            && state.selected_index < state.directory.files.len().saturating_sub(1)
        {
            state.selected_index += 1;
        }
    }

    /// Enter a directory if the selected item is a folder.
    pub fn enter_directory(&mut self) {
        let (selected_item, current_path) = match self.browsing() {
            Some(state) => {
                let Some(item) = self.selected_file_item() else {
                    return;
                };
                (item.clone(), state.directory.current_path.clone())
            }
            None => return,
        };

        if selected_item.actual_name.is_empty() {
            return;
        }
        if selected_item.kind == EntryKind::Folder {
            let new_path = Self::join_folder_path(&current_path, &selected_item.actual_name);
            self.navigate_to(new_path, "Enter directory failed");
        }
    }

    /// Go up one directory level.
    pub fn go_up_directory(&mut self) {
        let current_path = match self.browsing() {
            Some(state) => state.directory.current_path.clone(),
            None => return,
        };

        if current_path.is_empty() {
            return; // Already at root
        }

        // Remove trailing slash and go up one level
        let trimmed = current_path.trim_end_matches('/');
        let new_path = match trimmed.rfind('/') {
            Some(last_slash) => format!("{}/", &trimmed[..last_slash]),
            None => String::new(),
        };
        self.navigate_to(new_path, "Go up failed");
    }

    /// Enter search mode.
    pub fn enter_search_mode(&mut self) {
        let (files, file_items) = match self.browsing() {
            Some(state) => (
                state.directory.files.clone(),
                state.directory.file_items.clone(),
            ),
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
            state.directory.files = all_files;
            state.directory.file_items = all_file_items;
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
        let Search::Files { query, .. } = &mut self.search else {
            return Ok(());
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
    /// This function currently does not return errors but uses `Result` for API consistency.
    pub fn handle_clone_dialog_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        let Modal::Clone {
            input,
            original_path,
            is_folder: _is_folder,
        } = &mut self.modal
        else {
            return Ok(());
        };

        match key_event.code {
            KeyCode::Esc => {
                self.close_modal();
            }
            KeyCode::Enter => {
                // Only allow confirm if name is different from original
                if input != original_path && !input.is_empty() {
                    self.execute_clone();
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
        let (selected_item, current_path) = match self.browsing() {
            Some(state) => {
                let Some(item) = self.selected_file_item() else {
                    return;
                };
                (item.clone(), state.directory.current_path.clone())
            }
            None => return,
        };

        let (item_name, is_folder) = if selected_item.kind == EntryKind::Folder {
            (selected_item.actual_name, true)
        } else {
            (selected_item.actual_name, false)
        };

        let full_path = if is_folder {
            Self::join_folder_path(&current_path, &item_name)
        } else {
            Self::join_blob_path(&current_path, &item_name)
        };

        self.modal = Modal::Clone {
            input: full_path.clone(),
            original_path: full_path,
            is_folder,
        };
    }

    /// Start the clone described by the open dialog.
    pub fn execute_clone(&mut self) {
        let (mut new_path, original_path, is_folder) = match &self.modal {
            Modal::Clone {
                input,
                original_path,
                is_folder,
            } => (input.clone(), original_path.clone(), *is_folder),
            _ => return,
        };
        let Some(state) = self.browsing() else {
            return;
        };
        let object_store = state.object_store.clone();

        // Ensure folder paths end with /
        if is_folder && !new_path.ends_with('/') {
            new_path.push('/');
        }

        self.success_message = None;
        self.error_message = None;

        let label = format!("Cloning {}", original_path.trim_end_matches('/'));
        self.tasks
            .spawn_stoppable(label, move |reporter, cancel| async move {
                let result = if is_folder {
                    Self::clone_folder(
                        object_store.as_ref(),
                        &original_path,
                        &new_path,
                        &cancel,
                        &reporter,
                    )
                    .await
                } else {
                    Self::clone_blob(object_store.as_ref(), &original_path, &new_path).await
                };
                let orig = original_path.trim_end_matches('/');
                let new = new_path.trim_end_matches('/');
                TaskEvent::OperationFinished {
                    operation: Operation::Clone,
                    result: result.map(|()| format!("Successfully cloned {orig} to {new}")),
                }
            });
    }

    /// Show an operation's outcome, then relist if it may have changed the container.
    fn finish_operation(&mut self, operation: Operation, result: color_eyre::Result<String>) {
        match result {
            Ok(message) => self.success_message = Some(message),
            Err(e) => self.error_message = Some(format!("{}: {e}", operation.failure())),
        }
        // Even a failed or canceled batch may have changed some objects.
        if operation.changes_container()
            && let Some(state) = self.browsing()
        {
            let object_store = state.object_store.clone();
            let path = state.directory.current_path.clone();
            self.folder_stats_cache.clear();
            self.start_load(
                object_store,
                path,
                "Refreshing".to_owned(),
                LoadPurpose::AfterOperation(operation),
            );
        }
    }

    /// Clone a single blob.
    async fn clone_blob(
        object_store: &dyn ObjectStore,
        source: &str,
        destination: &str,
    ) -> color_eyre::Result<()> {
        let source_path = Self::object_path(source)?;
        let dest_path = Self::object_path(destination)?;

        // Use copy operation (server-side copy)
        object_store.copy(&source_path, &dest_path).await?;

        Ok(())
    }

    /// Clone all blobs in a folder (prefix).
    async fn clone_folder(
        object_store: &dyn ObjectStore,
        source: &str,
        destination: &str,
        cancel: &CancellationToken,
        reporter: &Reporter,
    ) -> color_eyre::Result<()> {
        let source_path = Self::object_path(source)?;

        crate::bulk::run(
            object_store.list(Some(&source_path)),
            cancel,
            |completed, total| reporter.progress(completed, total),
            |meta| async move {
                let file_path = meta.location.as_ref();
                let relative_path = file_path.strip_prefix(source).ok_or_else(|| {
                    color_eyre::eyre::eyre!("Listed object is outside source folder: {file_path}")
                })?;
                let dest_object_path = Self::object_path(&format!("{destination}{relative_path}"))?;
                object_store.copy(&meta.location, &dest_object_path).await?;
                Ok(())
            },
        )
        .await?;

        Ok(())
    }

    /// Handle key events when in delete dialog mode.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but uses `Result` for API consistency.
    pub fn handle_delete_dialog_key_event(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<()> {
        let Modal::DeleteConfirm {
            input, target_name, ..
        } = &mut self.modal
        else {
            return Ok(());
        };

        match key_event.code {
            KeyCode::Esc => {
                self.close_modal();
            }
            KeyCode::Enter => {
                // Only allow confirm if the typed name matches the target name
                if input == target_name {
                    self.execute_delete();
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
        let (selected_item, current_path) = match self.browsing() {
            Some(state) => {
                let Some(item) = self.selected_file_item() else {
                    return;
                };
                (item.clone(), state.directory.current_path.clone())
            }
            None => return,
        };

        let (item_name, is_folder) = if selected_item.kind == EntryKind::Folder {
            (selected_item.actual_name, true)
        } else {
            (selected_item.actual_name, false)
        };

        let full_path = if is_folder {
            Self::join_folder_path(&current_path, &item_name)
        } else {
            Self::join_blob_path(&current_path, &item_name)
        };

        self.modal = Modal::DeleteConfirm {
            input: String::new(),
            target_path: full_path,
            target_name: item_name,
            is_folder,
        };
    }

    /// Start the delete described by the open dialog.
    pub fn execute_delete(&mut self) {
        let (target_path, is_folder) = match &self.modal {
            Modal::DeleteConfirm {
                target_path,
                is_folder,
                ..
            } => (target_path.clone(), *is_folder),
            _ => return,
        };
        let Some(state) = self.browsing() else {
            return;
        };
        let object_store = state.object_store.clone();

        self.success_message = None;
        self.error_message = None;

        let label = format!("Deleting {}", target_path.trim_end_matches('/'));
        self.tasks
            .spawn_stoppable(label, move |reporter, cancel| async move {
                let result = if is_folder {
                    Self::delete_folder(object_store.as_ref(), &target_path, &cancel, &reporter)
                        .await
                } else {
                    Self::delete_blob(object_store.as_ref(), &target_path).await
                };
                let name = target_path.trim_end_matches('/');
                TaskEvent::OperationFinished {
                    operation: Operation::Delete,
                    result: result.map(|()| format!("Successfully deleted {name}")),
                }
            });
    }

    /// Delete a single blob.
    async fn delete_blob(object_store: &dyn ObjectStore, path: &str) -> color_eyre::Result<()> {
        let object_path = Self::object_path(path)?;

        object_store.delete(&object_path).await?;

        Ok(())
    }

    /// Delete all blobs in a folder (prefix).
    async fn delete_folder(
        object_store: &dyn ObjectStore,
        prefix: &str,
        cancel: &CancellationToken,
        reporter: &Reporter,
    ) -> color_eyre::Result<()> {
        let prefix_path = Self::object_path(prefix)?;

        crate::bulk::run(
            object_store.list(Some(&prefix_path)),
            cancel,
            |completed, total| reporter.progress(completed, total),
            |meta| async move {
                object_store.delete(&meta.location).await?;
                Ok(())
            },
        )
        .await?;

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
                state.directory.files = all_files;
                state.directory.file_items = all_file_items;
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

                state.directory.file_items.clone_from(&filtered_items);
                state.directory.files = filtered_items
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

    #[must_use]
    pub fn is_favorite(&self, container: &str) -> bool {
        self.favorites.contains(&self.storage_account, container)
    }

    fn reload_favorites(&mut self) {
        let result = self
            .favorites_path
            .as_deref()
            .ok_or_else(|| std::io::Error::other("Cannot determine user data directory"))
            .and_then(Favorites::load);
        match result {
            Ok(favorites) => self.favorites = favorites,
            Err(error) => self.error_message = Some(format!("Cannot load favorites: {error}")),
        }
    }

    fn sort_containers(&mut self) {
        let selected = self
            .containers
            .get(self.selected_container_index)
            .map(|c| c.name.clone());
        let favorites = &self.favorites;
        let account = &self.storage_account;
        let key = |c: &ContainerInfo| (!favorites.contains(account, &c.name), c.name.clone());
        self.all_containers.sort_by_key(key);
        self.containers.sort_by_key(key);
        if let Search::Containers { all_containers, .. } = &mut self.search {
            all_containers.sort_by_key(key);
        }
        self.selected_container_index = selected
            .and_then(|name| self.containers.iter().position(|c| c.name == name))
            .unwrap_or(0);
    }

    fn toggle_favorite(&mut self) {
        let Some(container) = self.containers.get(self.selected_container_index) else {
            return;
        };
        let result = self
            .favorites_path
            .as_deref()
            .ok_or_else(|| std::io::Error::other("Cannot determine user data directory"))
            .and_then(|path| Favorites::toggle(path, &self.storage_account, &container.name));
        self.success_message = None;
        match result {
            Ok(favorites) => {
                self.favorites = favorites;
                self.error_message = None;
                self.sort_containers();
            }
            Err(error) => self.error_message = Some(format!("Cannot save favorites: {error}")),
        }
    }

    /// Load the list of containers from Azure Storage.
    fn load_containers(&mut self) {
        self.error_message = None;
        self.success_message = None;

        let account = self.storage_account.clone();
        let credentials = self.credentials.clone();
        self.tasks.spawn("Loading containers", async move {
            TaskEvent::ContainersLoaded(Self::list_containers(&account, &credentials).await)
        });
    }

    fn finish_load_containers(&mut self, result: Result<Vec<ContainerInfo>, String>) {
        match result {
            Ok(mut containers) => {
                self.reload_favorites();
                containers.sort_by_key(|c| (!self.is_favorite(&c.name), c.name.clone()));
                // Always store the full list
                self.all_containers.clone_from(&containers);

                let search_query = match &self.search {
                    Search::Containers { query, .. } => Some(query.clone()),
                    _ => None,
                };

                if let Search::Containers { all_containers, .. } = &mut self.search {
                    all_containers.clone_from(&containers);
                }

                if let Some(query) = search_query {
                    if query.is_empty() {
                        self.containers = containers;
                        self.selected_container_index = 0;
                    } else {
                        self.apply_container_search(&query);
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
    }

    /// List all containers in the storage account with pagination support.
    async fn list_containers(
        account_name: &str,
        credentials: &AzureCredentialProvider,
    ) -> Result<Vec<ContainerInfo>, String> {
        let client = reqwest::Client::new();

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

            // Resolve credentials for each page so an expired token is refreshed.
            let response = Self::container_list_request(credentials, &client, &url)
                .await?
                .header("x-ms-date", &date)
                .header("x-ms-version", "2020-08-04")
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

    /// Authorize account-level requests with the same provider as blob operations.
    async fn container_list_request(
        credentials: &AzureCredentialProvider,
        client: &reqwest::Client,
        url: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        let credential = credentials.get_credential().await.map_err(|e| {
            format!(
                "Azure CLI authentication failed. Sign in with `az login --identity` \
                 or `az login`, then refresh: {e}"
            )
        })?;
        match credential.as_ref() {
            AzureCredential::BearerToken(token) => Ok(client.get(url).bearer_auth(token)),
            _ => Err("Azure CLI authentication requires a bearer token".to_string()),
        }
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

    /// Select a container and list its root in the background.
    fn select_container(&mut self) -> color_eyre::Result<()> {
        let Some(selected_container) = self.containers.get(self.selected_container_index) else {
            return Ok(());
        };

        let azure_client = MicrosoftAzureBuilder::new()
            .with_account(&self.storage_account)
            .with_container_name(&selected_container.name)
            .with_credentials(self.credentials.clone())
            .build()?;
        let label = format!("Opening {}", selected_container.name);
        self.open_container(Arc::new(azure_client), label);
        Ok(())
    }

    /// Browse `object_store` once its root listing succeeds.
    fn open_container(&mut self, object_store: Arc<dyn ObjectStore>, label: String) {
        let purpose = LoadPurpose::OpenContainer(object_store.clone());
        self.start_load(object_store, String::new(), label, purpose);
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
        // Always start search from the full container list
        self.containers = self.all_containers.clone();
        self.selected_container_index = 0;
        self.search = Search::Containers {
            query: String::new(),
            all_containers: self.all_containers.clone(),
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
        let Search::Containers { query, .. } = &mut self.search else {
            return Ok(());
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

    /// Fetch information about the selected blob or folder, then show it.
    pub fn show_blob_info(&mut self) {
        let Some(state) = self.browsing() else {
            return;
        };
        let Some(selected_item) = self.selected_file_item() else {
            return;
        };
        let object_store = state.object_store.clone();
        let name = selected_item.actual_name.clone();
        let current_path = &state.directory.current_path;
        if selected_item.kind == EntryKind::Folder {
            let folder_path = Self::join_folder_path(current_path, &name);
            self.tasks.spawn(format!("Measuring {name}"), async move {
                TaskEvent::BlobInfo(
                    Self::folder_info(object_store.as_ref(), &folder_path, &name).await,
                )
            });
        } else {
            let blob_path = Self::join_blob_path(current_path, &name);
            self.tasks
                .spawn(format!("Fetching info for {name}"), async move {
                    TaskEvent::BlobInfo(
                        Self::blob_info(object_store.as_ref(), &blob_path, &name).await,
                    )
                });
        }
    }

    /// Get information about a folder (blob count and total size).
    async fn folder_info(
        object_store: &dyn ObjectStore,
        folder_path: &str,
        folder_name: &str,
    ) -> color_eyre::Result<BlobInfo> {
        let object_path = Self::object_path(folder_path)?;

        // List all objects in this folder (recursively)
        let mut blob_count = 0;
        let mut total_size: u64 = 0; // Explicitly type as u64

        let stream = object_store.list(Some(&object_path));
        let objects = crate::bulk::snapshot(stream).await?;

        for meta in objects {
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
    async fn blob_info(
        object_store: &dyn ObjectStore,
        blob_path: &str,
        blob_name: &str,
    ) -> color_eyre::Result<BlobInfo> {
        let object_path = Self::object_path(blob_path)?;

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
            Err(e) => Err(color_eyre::eyre::eyre!("Failed to get blob metadata: {e}",)),
        }
    }

    /// Copy the full blob path to clipboard.
    ///
    /// # Errors
    ///
    /// Returns an error if clipboard access fails.
    pub fn copy_blob_path_to_clipboard(&mut self) -> color_eyre::Result<()> {
        let (selected_item, current_path) = match self.browsing() {
            Some(state) => {
                let Some(item) = self.selected_file_item() else {
                    return Ok(());
                };
                (item.clone(), state.directory.current_path.clone())
            }
            None => return Ok(()),
        };

        if selected_item.actual_name.is_empty() {
            return Ok(());
        }
        let is_folder = selected_item.kind == EntryKind::Folder;
        let full_path = if is_folder {
            Self::join_folder_path(&current_path, &selected_item.actual_name)
        } else {
            Self::join_blob_path(&current_path, &selected_item.actual_name)
        };

        let clipboard = match self.clipboard.take() {
            Some(clipboard) => clipboard,
            None => Clipboard::new()?,
        };
        self.clipboard
            .insert(clipboard)
            .set_text(full_path.clone())?;

        // Set success message
        let item_type = if is_folder { "folder" } else { "file" };
        self.success_message = Some(format!("Copied {item_type} path to clipboard: {full_path}"));
        self.error_message = None; // Clear any existing error message

        Ok(())
    }

    /// Upload a local file into the current prefix without replacing an existing blob.
    fn upload_file(&mut self, source: PathBuf) {
        let Some(state) = self.browsing() else {
            return;
        };
        let object_store = state.object_store.clone();
        let current_path = state.directory.current_path.clone();
        let label = format!("Uploading {}", source.display());
        self.tasks
            .spawn_stoppable(label, move |_reporter, cancel| async move {
                // The put is atomic, so abandoning it leaves the blob whole or absent.
                let result = tokio::select! {
                    biased;
                    () = cancel.cancelled() => Err(color_eyre::eyre::eyre!("canceled")),
                    result = Self::put_file(object_store.as_ref(), &current_path, &source) => result,
                };
                TaskEvent::OperationFinished {
                    operation: Operation::Upload,
                    result: result
                        .map(|destination| format!("Successfully uploaded {destination}")),
                }
            });
    }

    /// Create the blob for `source` under `current_path`, returning its path.
    async fn put_file(
        store: &dyn ObjectStore,
        current_path: &str,
        source: &Path,
    ) -> color_eyre::Result<ObjectPath> {
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| color_eyre::eyre::eyre!("File name must be valid UTF-8"))?;
        let destination = Self::join_blob_path(current_path, name);
        let destination = Self::object_path(&destination)?;
        let mut file = tokio::fs::File::open(source).await?;
        if !file.metadata().await?.is_file() {
            color_eyre::eyre::bail!("Select a regular file to upload");
        }
        let mut contents = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut file, &mut contents).await?;
        store
            .put_opts(
                &destination,
                contents.into(),
                object_store::PutMode::Create.into(),
            )
            .await?;
        Ok(destination)
    }

    /// Show the download destination picker.
    pub fn show_download_picker(&mut self) {
        if let Some(state) = self.browsing() {
            if state.directory.files.is_empty() {
                return;
            }
        } else {
            return;
        }

        self.modal = Modal::DownloadPicker { destination: None };
    }

    /// Start downloading the selected file or folder to the picked destination.
    pub fn start_download(&mut self) {
        let destination = match &self.modal {
            Modal::DownloadPicker { destination } => destination.clone(),
            _ => None,
        };

        let Some(state) = self.browsing() else {
            return;
        };
        let Some(selected_item) = self.selected_file_item() else {
            return;
        };

        let Some(destination) = destination else {
            return;
        };
        let is_folder = selected_item.kind == EntryKind::Folder;
        let name = selected_item.actual_name.clone();
        let object_store = state.object_store.clone();
        let current_path = state.directory.current_path.clone();

        self.close_modal();
        self.success_message = None;
        self.error_message = None;

        let label = format!("Downloading {name}");
        if is_folder {
            self.tasks
                .spawn_stoppable(label, move |reporter, cancel| async move {
                    let result = Self::download_folder(
                        object_store.as_ref(),
                        &current_path,
                        &name,
                        &destination,
                        &cancel,
                        &reporter,
                    )
                    .await;
                    TaskEvent::OperationFinished {
                        operation: Operation::Download,
                        result: result.map(|completed| {
                            format!("Successfully downloaded {completed} objects from {name}")
                        }),
                    }
                });
        } else {
            self.tasks.spawn(label, async move {
                let result =
                    Self::download_file(object_store.as_ref(), &current_path, &name, &destination)
                        .await;
                TaskEvent::OperationFinished {
                    operation: Operation::Download,
                    result: result.map(|()| format!("Successfully downloaded {name}")),
                }
            });
        }
    }

    /// Download a single file.
    async fn download_file(
        object_store: &dyn ObjectStore,
        current_path: &str,
        file_name: &str,
        destination: &Path,
    ) -> color_eyre::Result<()> {
        let blob_path = Self::join_blob_path(current_path, file_name);

        let object_path = Self::object_path(blob_path.as_str())?;

        Self::download_object(object_store, &object_path, &destination.join(file_name)).await
    }

    /// Stream one blob to `destination` without holding it in memory.
    ///
    /// The data goes to a `.part` file beside the destination that is renamed
    /// only once complete, so a failed or canceled download leaves no
    /// truncated file behind.
    async fn download_object(
        object_store: &dyn ObjectStore,
        location: &ObjectPath,
        destination: &Path,
    ) -> color_eyre::Result<()> {
        /// Removes the partial file; after a completed download it is already
        /// renamed away and there is nothing to remove.
        struct Partial(PathBuf);
        impl Drop for Partial {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }

        // Ensure parent directory exists
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut partial_name = destination.as_os_str().to_owned();
        partial_name.push(".part");
        let partial = Partial(PathBuf::from(partial_name));

        let mut chunks = object_store.get(location).await?.into_stream();
        let mut file = tokio::fs::File::create(&partial.0).await?;
        while let Some(chunk) = chunks.try_next().await? {
            file.write_all(&chunk).await?;
        }
        // Tokio writes in the background; wait for the last one before renaming.
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&partial.0, destination).await?;
        Ok(())
    }

    /// Download all files in a folder, returning how many there were.
    async fn download_folder(
        object_store: &dyn ObjectStore,
        current_path: &str,
        folder_name: &str,
        destination: &Path,
        cancel: &CancellationToken,
        reporter: &Reporter,
    ) -> color_eyre::Result<usize> {
        let folder_path = Self::join_folder_path(current_path, folder_name);

        let object_path = Self::object_path(folder_path.as_str())?;

        let folder_destination = destination.join(folder_name);
        let completed = crate::bulk::run(
            object_store.list(Some(&object_path)),
            cancel,
            |completed, total| reporter.progress(completed, total),
            |meta| {
                let folder_path = &folder_path;
                let folder_destination = &folder_destination;
                async move {
                    let file_path = meta.location.as_ref();
                    let relative_path = file_path.strip_prefix(folder_path).ok_or_else(|| {
                        color_eyre::eyre::eyre!(
                            "Listed object is outside source folder: {file_path}"
                        )
                    })?;
                    let file_destination = folder_destination.join(relative_path);
                    Self::download_object(object_store, &meta.location, &file_destination).await
                }
            },
        )
        .await?;

        Ok(completed)
    }

    /// Ask for a destination folder once the download picker is confirmed.
    pub fn confirm_download(&mut self) {
        if self.is_modal_download_picker() {
            // The dialog blocks its thread until it closes.
            self.tasks.spawn("Waiting for folder selection", async {
                TaskEvent::DownloadDestinationPicked(
                    tokio::task::spawn_blocking(|| rfd::FileDialog::new().pick_folder()).await,
                )
            });
        }
    }

    // ========================================
    // Preview Panel Methods
    // ========================================

    /// Keep the default pane attached to the selected entry without taking focus.
    fn sync_preview(&mut self) {
        if self.ui.preview != PreviewPanel::Browsing || self.tasks.is_running() {
            return;
        }
        let target = self.browsing().and_then(|state| {
            self.selected_file_item().map(|item| PreviewTarget {
                container: self.selected_container_index,
                path: state.current_path().to_owned(),
                name: item.actual_name.clone(),
            })
        });
        if target == self.preview_target {
            return;
        }
        self.preview_tasks.cancel();
        self.clear_preview_data();
        self.preview_target = target;
        if self.preview_target.is_some() {
            self.start_preview(PreviewPanel::Browsing);
        }
    }

    /// Open the preview panel and fetch the selected file's preview.
    pub fn load_preview(&mut self) {
        self.preview_tasks.cancel();
        self.clear_preview_data();
        self.start_preview(PreviewPanel::Focused);
    }

    fn start_preview(&mut self, panel: PreviewPanel) {
        let (selected_item, current_path, object_store) = match self.browsing() {
            Some(state) => {
                let Some(item) = self.selected_file_item() else {
                    return;
                };
                (
                    item.clone(),
                    state.directory.current_path.clone(),
                    state.object_store.clone(),
                )
            }
            None => return,
        };

        if selected_item.actual_name.is_empty() {
            return;
        }

        // Check if it's a folder
        if selected_item.kind == EntryKind::Folder {
            self.ui.preview = PreviewPanel::Browsing;
            return;
        }

        let name = selected_item.actual_name;

        // Check file type
        let file_type = PreviewFileType::from_extension(&name);
        if !file_type.is_supported() {
            self.preview_error = Some(
                "Unsupported file type. Preview supports: CSV, TSV, JSON, Parquet, and text files"
                    .to_string(),
            );
            self.preview_file_type = Some(file_type);
            self.ui.preview = panel;
            return;
        }

        let object_path = match Self::object_path(&Self::join_blob_path(&current_path, &name)) {
            Ok(object_path) => object_path,
            Err(e) => {
                self.error_message = Some(format!("Preview failed: {e}"));
                return;
            }
        };

        self.preview_file_type = Some(file_type.clone());
        self.ui.preview = panel;
        self.preview_error = None;
        self.preview_data = None;
        self.preview_scroll = (0, 0);
        self.preview_selected_row = 0;
        self.parquet_preview_mode = None;
        self.parquet_table_data = None;
        self.parquet_schema_data = None;

        let tasks = if panel == PreviewPanel::Focused {
            &mut self.tasks
        } else {
            &mut self.preview_tasks
        };
        tasks.spawn(format!("Loading preview of {name}"), async move {
            TaskEvent::PreviewLoaded(Box::new(
                Self::fetch_preview(object_store.as_ref(), &object_path, &file_type).await,
            ))
        });
    }

    /// Fetch and parse a preview (up to `MAX_PREVIEW_BYTES` for non-Parquet files).
    #[allow(clippy::too_many_lines)]
    async fn fetch_preview(
        object_store: &dyn ObjectStore,
        object_path: &ObjectPath,
        file_type: &PreviewFileType,
    ) -> LoadedPreview {
        let mut preview = LoadedPreview::default();

        // For Parquet files, support both table and metadata views.
        if *file_type == PreviewFileType::Parquet {
            let head_result = object_store.head(object_path).await;
            match head_result {
                Ok(meta) => {
                    let file_size = meta.size;
                    if file_size <= MAX_PARQUET_TABLE_PREVIEW_BYTES as u64 {
                        let get_result = object_store.get_range(object_path, 0..file_size).await;

                        match get_result {
                            Ok(bytes) => {
                                let table_result = parse_parquet_table(&bytes);
                                let schema_result = parse_parquet_schema(&bytes, Some(file_size));
                                let mut table_error: Option<String> = None;

                                match table_result {
                                    Ok(PreviewData::Table(table)) => {
                                        preview.parquet_table = Some(table.clone());
                                        preview.data = Some(PreviewData::Table(table));
                                        preview.parquet_mode = Some(ParquetPreviewMode::Table);
                                    }
                                    Ok(_) => {
                                        table_error = Some(
                                            "Unexpected Parquet table preview format".to_string(),
                                        );
                                    }
                                    Err(e) => {
                                        table_error = Some(e);
                                    }
                                }

                                match schema_result {
                                    Ok(PreviewData::ParquetSchema(mut schema)) => {
                                        if preview.data.is_none() && table_error.is_some() {
                                            schema.note.clone_from(&table_error);
                                        }
                                        preview.parquet_schema = Some(schema);
                                    }
                                    Ok(_) => {
                                        preview.error = Some(
                                            "Unexpected Parquet metadata preview format"
                                                .to_string(),
                                        );
                                    }
                                    Err(e) => {
                                        preview.error = Some(e);
                                    }
                                }

                                if preview.data.is_none() {
                                    if let Some(schema) = &preview.parquet_schema {
                                        preview.data =
                                            Some(PreviewData::ParquetSchema(schema.clone()));
                                        preview.parquet_mode = Some(ParquetPreviewMode::Metadata);
                                        preview.error = None;
                                    } else if preview.error.is_none() {
                                        preview.error = table_error;
                                    }
                                }
                            }
                            Err(e) => {
                                preview.error = Some(format!("Failed to fetch file: {e}"));
                            }
                        }
                    } else {
                        // Large parquet files: read only footer metadata.
                        let start = file_size.saturating_sub(MAX_PARQUET_PREVIEW_BYTES as u64);
                        let get_result =
                            object_store.get_range(object_path, start..file_size).await;

                        match get_result {
                            Ok(bytes) => match parse_parquet_schema(&bytes, Some(file_size)) {
                                Ok(PreviewData::ParquetSchema(mut schema)) => {
                                    schema.note = Some(
                                        "Table preview unavailable for files larger than 16 MB"
                                            .to_string(),
                                    );
                                    preview.parquet_schema = Some(schema.clone());
                                    preview.data = Some(PreviewData::ParquetSchema(schema));
                                    preview.parquet_mode = Some(ParquetPreviewMode::Metadata);
                                }
                                Ok(_) => {
                                    preview.error = Some(
                                        "Unexpected Parquet metadata preview format".to_string(),
                                    );
                                }
                                Err(e) => {
                                    preview.error = Some(e);
                                }
                            },
                            Err(e) => {
                                preview.error = Some(format!("Failed to fetch file: {e}"));
                            }
                        }
                    }
                }
                Err(e) => {
                    preview.error = Some(format!("Failed to get file info: {e}"));
                }
            }
            return preview;
        }

        // For other file types, fetch from the beginning
        let get_result = object_store
            .get_range(object_path, 0..(MAX_PREVIEW_BYTES as u64))
            .await;

        match get_result {
            Ok(bytes) => {
                // Parse the data
                match parse_preview(&bytes, file_type) {
                    Ok(data) => {
                        preview.data = Some(data);
                    }
                    Err(e) => {
                        preview.error = Some(e);
                    }
                }
            }
            Err(e) => {
                preview.error = Some(format!("Failed to fetch file: {e}"));
            }
        }

        preview
    }

    /// Close the preview panel.
    pub fn close_preview(&mut self) {
        self.ui.preview = PreviewPanel::Hidden;
        self.preview_tasks.cancel();
        self.preview_target = None;
        self.clear_preview_data();
    }

    fn clear_preview_data(&mut self) {
        self.preview_data = None;
        self.preview_file_type = None;
        self.preview_error = None;
        self.preview_scroll = (0, 0);
        self.preview_selected_row = 0;
        self.parquet_preview_mode = None;
        self.parquet_table_data = None;
        self.parquet_schema_data = None;
    }

    /// Toggle between parquet table and metadata preview modes.
    pub fn toggle_parquet_preview_mode(&mut self) {
        if self.preview_file_type != Some(PreviewFileType::Parquet) {
            return;
        }

        match self.parquet_preview_mode {
            Some(ParquetPreviewMode::Table) => {
                if let Some(schema) = &self.parquet_schema_data {
                    self.preview_data = Some(PreviewData::ParquetSchema(schema.clone()));
                    self.parquet_preview_mode = Some(ParquetPreviewMode::Metadata);
                    self.preview_scroll = (0, 0);
                    self.preview_selected_row = 0;
                }
            }
            Some(ParquetPreviewMode::Metadata) => {
                if let Some(table) = &self.parquet_table_data {
                    self.preview_data = Some(PreviewData::Table(table.clone()));
                    self.parquet_preview_mode = Some(ParquetPreviewMode::Table);
                    self.preview_scroll = (0, 0);
                    self.preview_selected_row = 0;
                }
            }
            None => {}
        }
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
        if let Some(PreviewData::Table(table)) = &self.preview_data {
            let num_cols = table
                .headers
                .len()
                .max(table.rows.first().map_or(0, Vec::len));
            if num_cols > 0 {
                self.preview_scroll.1 = (self.preview_scroll.1 + 1).min(num_cols.saturating_sub(1));
            }
        } else {
            self.preview_scroll.1 += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        App, BrowsingState, EntryKind, KeyCode, KeyEvent, KeyModifiers, LoadedDirectory, Modal,
        ParquetPreviewMode, Search, Session, SortCriteria, UiToggles,
    };
    use crate::preview::{ParquetSchemaPreview, PreviewData, PreviewFileType, TablePreview};
    use crate::terminal_icons::detect_terminal_icons;
    use chrono::{TimeZone, Utc};
    use futures::{StreamExt, stream::BoxStream};
    use object_store::{ObjectStoreExt, path::Path as ObjectPath};
    use proptest::prelude::*;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn arb_entry_kind() -> impl Strategy<Value = super::EntryKind> {
        prop_oneof![Just(super::EntryKind::File), Just(super::EntryKind::Folder)]
    }

    fn arb_sort_criteria() -> impl Strategy<Value = SortCriteria> {
        prop_oneof![
            Just(SortCriteria::Name),
            Just(SortCriteria::DateModified),
            Just(SortCriteria::DateCreated),
            Just(SortCriteria::Size),
        ]
    }

    fn arb_timestamp() -> impl Strategy<Value = Option<chrono::DateTime<chrono::Utc>>> {
        prop::option::of((-2_208_988_800_i64..4_102_444_800_i64).prop_map(|secs| {
            Utc.timestamp_opt(secs, 0)
                .single()
                .expect("generated timestamp should be valid")
        }))
    }

    fn arb_file_item() -> impl Strategy<Value = super::FileItem> {
        (
            "[A-Za-z0-9_./-]{1,16}",
            arb_entry_kind(),
            prop::option::of(0_u64..10_000),
            arb_timestamp(),
            arb_timestamp(),
        )
            .prop_map(|(actual_name, kind, size, last_modified, created)| {
                let icon = match kind {
                    super::EntryKind::File => "[FILE]",
                    super::EntryKind::Folder => "[DIR]",
                };
                super::FileItem {
                    display_name: format!("{icon} {actual_name}"),
                    actual_name,
                    kind,
                    size,
                    last_modified,
                    created,
                }
            })
    }

    fn test_app() -> App {
        App {
            running: true,
            session: Session::Selecting,
            storage_account: "test-account".to_string(),
            favorites: crate::favorites::Favorites::default(),
            favorites_path: None,
            credentials: Arc::new(object_store::StaticCredentialProvider::new(
                object_store::azure::AzureCredential::BearerToken("test-token".to_string()),
            )),
            containers: Vec::new(),
            all_containers: Vec::new(),
            selected_container_index: 0,
            error_message: None,
            success_message: None,
            search: Search::Inactive,
            icons: detect_terminal_icons(),
            modal: Modal::None,
            ui: UiToggles {
                preview: super::PreviewPanel::default(),
            },
            sort_criteria: SortCriteria::Name,
            preview_data: None,
            preview_file_type: None,
            preview_scroll: (0, 0),
            preview_error: None,
            preview_selected_row: 0,
            parquet_preview_mode: None,
            parquet_table_data: None,
            parquet_schema_data: None,
            tasks: super::Tasks::new(),
            preview_tasks: super::Tasks::new(),
            preview_target: None,
            folder_stats_cache: std::collections::HashMap::new(),
            clipboard: None,
        }
    }

    /// Apply task results until no operation is running.
    async fn settle(app: &mut App) {
        while app.tasks.is_running() {
            let message =
                tokio::time::timeout(std::time::Duration::from_secs(10), app.tasks.recv())
                    .await
                    .expect("operation did not finish")
                    .expect("task channel closed");
            app.apply_task_message(message);
        }
    }

    #[tokio::test]
    async fn favorite_toggle_preserves_selection_search_and_saved_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app();
        app.favorites_path = Some(dir.path().join("favorites.json"));
        app.containers = ["alpha", "reports", "zebra"]
            .into_iter()
            .map(|name| super::ContainerInfo {
                name: name.to_owned(),
            })
            .collect();
        app.all_containers = app.containers.clone();
        app.selected_container_index = 1;
        app.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.containers[0].name, "reports");
        assert_eq!(app.selected_container_index, 0);
        assert!(app.is_favorite("reports"));
        app.enter_container_search_mode();
        app.handle_container_search_key_event(KeyEvent::new(
            KeyCode::Char('f'),
            KeyModifiers::NONE,
        ))
        .unwrap();
        assert_eq!(app.container_search_query(), Some("f"));
        assert!(app.is_favorite("reports"));
        app.exit_container_search_mode();
        assert_eq!(app.containers[0].name, "reports");
        app.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.containers[app.selected_container_index].name, "reports");
        assert_eq!(app.containers[0].name, "alpha");
        assert!(
            !crate::favorites::Favorites::load(app.favorites_path.as_deref().unwrap())
                .unwrap()
                .contains("test-account", "reports")
        );
        std::fs::write(app.favorites_path.as_deref().unwrap(), "invalid").unwrap();
        app.toggle_favorite();
        assert!(
            app.error_message
                .as_deref()
                .unwrap()
                .contains("Cannot save favorites")
        );
        assert!(!app.is_favorite("reports"));
    }

    /// Run in tmux to exercise rendering and input without Azure credentials.
    #[tokio::test]
    #[ignore = "interactive terminal required"]
    async fn favorites_interactive_preview() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app();
        app.favorites_path = Some(dir.path().join("favorites.json"));
        app.containers = ["alpha", "backups", "reports"]
            .into_iter()
            .map(|name| super::ContainerInfo {
                name: name.to_owned(),
            })
            .collect();
        app.all_containers = app.containers.clone();
        let mut terminal = ratatui::init();
        let result = app.run(&mut terminal).await;
        ratatui::restore();
        result.unwrap();
    }

    fn upload_test_app(prefix: &str) -> App {
        let mut app = test_app();
        app.session = Session::Browsing(BrowsingState {
            object_store: Arc::new(object_store::memory::InMemory::new()),
            directory: LoadedDirectory {
                current_path: prefix.to_owned(),
                files: Vec::new(),
                file_items: Vec::new(),
                has_folder_stats: false,
            },
            selected_index: 0,
        });
        app
    }

    #[tokio::test]
    async fn upload_preserves_contents_and_names_and_refreshes_listing() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("résumé 100%.bin");
        for (prefix, contents) in [("", b"\0\xffbinary".as_slice()), ("nested/folder/", b"")] {
            std::fs::write(&source, contents).unwrap();
            let mut app = upload_test_app(prefix);
            app.upload_file(source.clone());
            settle(&mut app).await;
            assert!(app.error_message.is_none());
            let state = app.browsing().unwrap();
            let path = ObjectPath::parse(format!("{prefix}résumé 100%.bin")).unwrap();
            let uploaded = state
                .object_store
                .get(&path)
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap();
            assert_eq!(uploaded.as_ref(), contents);
            assert_eq!(state.directory.file_items.len(), 1);
            assert_eq!(state.directory.file_items[0].actual_name, "résumé 100%.bin");
            assert!(
                app.success_message
                    .as_ref()
                    .unwrap()
                    .contains(path.as_ref())
            );
        }
    }

    #[tokio::test]
    async fn upload_does_not_replace_existing_blob() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("existing.txt");
        std::fs::write(&source, b"replacement").unwrap();
        let mut app = upload_test_app("");
        let store = app.browsing().unwrap().object_store.clone();
        let destination = ObjectPath::from("existing.txt");
        store.put(&destination, "original".into()).await.unwrap();
        app.upload_file(source);
        settle(&mut app).await;
        assert!(app.error_message.unwrap().contains("Upload failed"));
        assert_eq!(
            store
                .get(&destination)
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap()
                .as_ref(),
            b"original"
        );
        assert!(app.success_message.is_none());
    }

    #[tokio::test]
    async fn upload_rejects_missing_files_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = upload_test_app("");
        for source in [dir.path().join("missing"), dir.path().to_owned()] {
            app.upload_file(source);
            settle(&mut app).await;
            assert!(app.error_message.take().unwrap().contains("Upload failed"));
        }
        let objects = app
            .browsing()
            .unwrap()
            .object_store
            .list_with_delimiter(None)
            .await
            .unwrap()
            .objects;
        assert_eq!(objects, Vec::<object_store::ObjectMeta>::new());
        assert!(app.success_message.is_none());
    }

    #[tokio::test]
    async fn upload_picker_works_in_empty_container_and_is_modal() {
        let mut app = upload_test_app("");
        app.handle_key_event(KeyEvent::from(KeyCode::Char('u')))
            .unwrap();
        assert!(matches!(app.modal, Modal::UploadPicker));
        for key in ['c', 'x', 'd', '/', 'r'] {
            app.handle_key_event(KeyEvent::from(KeyCode::Char(key)))
                .unwrap();
            assert!(matches!(app.modal, Modal::UploadPicker));
        }
        app.handle_key_event(KeyEvent::from(KeyCode::Esc)).unwrap();
        assert!(matches!(app.modal, Modal::None));
        assert!(app.browsing().unwrap().directory.file_items.is_empty());
        app.modal = Modal::SortPicker;
        app.handle_key_event(KeyEvent::from(KeyCode::Char('u')))
            .unwrap();
        assert!(matches!(app.modal, Modal::SortPicker));
        let mut app = test_app();
        app.handle_key_event(KeyEvent::from(KeyCode::Char('u')))
            .unwrap();
        assert!(matches!(app.modal, Modal::None));
    }

    #[tokio::test]
    async fn container_requests_use_bearer_auth_without_sending_a_query_token() {
        let app = test_app();
        let url = "https://test-account.blob.core.windows.net/?comp=list&marker=next";
        let request = App::container_list_request(&app.credentials, &reqwest::Client::new(), url)
            .await
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(request.url().as_str(), url);
        let authorization = &request.headers()[reqwest::header::AUTHORIZATION];
        assert_eq!(authorization, "Bearer test-token");
        assert!(authorization.is_sensitive());
    }

    #[tokio::test]
    async fn container_requests_reject_non_bearer_credentials() {
        let credentials: object_store::azure::AzureCredentialProvider =
            Arc::new(object_store::StaticCredentialProvider::new(
                object_store::azure::AzureCredential::SASToken(Vec::new()),
            ));

        let error = App::container_list_request(
            &credentials,
            &reqwest::Client::new(),
            "https://test-account.blob.core.windows.net/?comp=list",
        )
        .await
        .unwrap_err();

        assert_eq!(error, "Azure CLI authentication requires a bearer token");
    }

    #[tokio::test]
    async fn quit_key_stops_app() {
        let mut app = test_app();

        app.handle_key_event(KeyEvent::from(KeyCode::Char('q')))
            .unwrap();

        assert!(!app.running);
    }

    #[test]
    fn open_clone_dialog_sets_modal_data() {
        let mut app = test_app();
        app.session = Session::Browsing(BrowsingState {
            object_store: std::sync::Arc::new(object_store::memory::InMemory::new()),
            directory: LoadedDirectory {
                current_path: String::new(),
                files: vec![format!("{file} file.txt", file = app.icons.file)],
                file_items: vec![super::FileItem {
                    display_name: format!("{file} file.txt", file = app.icons.file),
                    actual_name: "file.txt".to_string(),
                    kind: super::EntryKind::File,
                    size: None,
                    last_modified: None,
                    created: None,
                }],
                has_folder_stats: false,
            },
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
            directory: LoadedDirectory {
                current_path: String::new(),
                files: vec![format!("{folder} logs", folder = app.icons.folder)],
                file_items: vec![super::FileItem {
                    display_name: format!("{folder} logs", folder = app.icons.folder),
                    actual_name: "logs".to_string(),
                    kind: super::EntryKind::Folder,
                    size: None,
                    last_modified: None,
                    created: None,
                }],
                has_folder_stats: false,
            },
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
            directory: LoadedDirectory {
                current_path: String::new(),
                files: vec![format!("{file} report.csv", file = app.icons.file)],
                file_items: Vec::new(),
                has_folder_stats: false,
            },
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
        app.all_containers = app.containers.clone();

        app.enter_container_search_mode();

        match &app.search {
            Search::Containers {
                query,
                all_containers,
            } => {
                assert_eq!(query, "");
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
            directory: LoadedDirectory {
                current_path: String::new(),
                files: Vec::new(),
                file_items: Vec::new(),
                has_folder_stats: false,
            },
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
            state.directory.file_items = file_items.clone();
            state.directory.files = state
                .directory
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
            assert_eq!(state.directory.files, vec!["file_a".to_string()]);
        }

        app.exit_search_mode();
        if let Session::Browsing(state) = &app.session {
            assert_eq!(state.directory.files.len(), 2);
        }
        assert!(matches!(app.search, Search::Inactive));
    }

    // Replacing an ancestor directory with a file forces real listing failures,
    // without Azure credentials, network access, or permission-dependent fixtures.
    async fn navigation_test_app() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("online/store");
        std::fs::create_dir_all(root.join("parent/child")).unwrap();
        std::fs::write(root.join("parent/child/data.txt"), "payload").unwrap();
        std::fs::write(root.join("parent/a.txt"), "payload").unwrap();
        let store = object_store::local::LocalFileSystem::new_with_prefix(root).unwrap();
        let mut app = test_app();
        let directory =
            App::load_directory(&store, &app.icons, app.sort_criteria, "parent/".to_owned())
                .await
                .unwrap();
        app.session = Session::Browsing(BrowsingState {
            object_store: Arc::new(store),
            directory,
            selected_index: 0,
        });
        (dir, app)
    }

    fn break_navigation_store(dir: &std::path::Path) {
        std::fs::rename(dir.join("online"), dir.join("offline")).unwrap();
        std::fs::write(dir.join("online"), "listing must fail").unwrap();
    }

    #[tokio::test]
    async fn folder_listing_failures_are_never_successful() {
        let (dir, mut app) = navigation_test_app().await;
        break_navigation_store(dir.path());
        let destination = tempfile::tempdir().unwrap();
        let starts: [fn(&mut App, &std::path::Path); 4] = [
            |app, _| {
                app.modal = Modal::Clone {
                    input: "copy/".to_owned(),
                    original_path: "parent/".to_owned(),
                    is_folder: true,
                };
                app.execute_clone();
            },
            |app, _| {
                app.modal = Modal::DeleteConfirm {
                    input: "parent".to_owned(),
                    target_path: "parent/".to_owned(),
                    target_name: "parent".to_owned(),
                    is_folder: true,
                };
                app.execute_delete();
            },
            |app, destination| {
                app.modal = Modal::DownloadPicker {
                    destination: Some(destination.to_owned()),
                };
                app.start_download();
            },
            |app, _| app.show_blob_info(),
        ];
        for start in starts {
            app.close_modal();
            start(&mut app, destination.path());
            settle(&mut app).await;
            assert!(app.success_message.is_none());
            assert!(
                app.error_message
                    .take()
                    .unwrap()
                    .contains("Folder listing failed")
            );
        }
        assert!(!destination.path().join("child").exists());
    }

    #[tokio::test]
    async fn folder_clone_copy_failure_reaches_dialog_error() {
        let (dir, mut app) = navigation_test_app().await;
        std::fs::write(dir.path().join("online/store/blocked"), "not a directory").unwrap();
        app.modal = Modal::Clone {
            input: "blocked/".to_owned(),
            original_path: "parent/".to_owned(),
            is_folder: true,
        };
        app.success_message = Some("Previous success".to_owned());
        app.handle_clone_dialog_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(app.modal, Modal::None));
        settle(&mut app).await;
        assert!(app.success_message.is_none());
        assert!(
            app.error_message
                .as_deref()
                .unwrap()
                .contains("Clone failed: Failed: 0/2 objects completed")
        );
        assert!(matches!(app.modal, Modal::None));
    }

    #[tokio::test]
    async fn folder_delete_listing_failure_reaches_dialog_error() {
        let (dir, mut app) = navigation_test_app().await;
        break_navigation_store(dir.path());
        app.modal = Modal::DeleteConfirm {
            input: "parent".to_owned(),
            target_path: "parent/".to_owned(),
            target_name: "parent".to_owned(),
            is_folder: true,
        };
        app.success_message = Some("Previous success".to_owned());
        app.handle_delete_dialog_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(app.modal, Modal::None));
        settle(&mut app).await;
        assert!(app.success_message.is_none());
        assert!(
            app.error_message
                .as_deref()
                .unwrap()
                .contains("Delete failed: Folder listing failed")
        );
        assert!(matches!(app.modal, Modal::None));
    }

    #[tokio::test]
    async fn overlapping_folder_clone_uses_original_snapshot() {
        let mut app = upload_test_app("");
        let store = app.browsing().unwrap().object_store.clone();
        store
            .put(&ObjectPath::from("source/a"), "a".into())
            .await
            .unwrap();
        store
            .put(&ObjectPath::from("source/copy/b"), "b".into())
            .await
            .unwrap();
        app.modal = Modal::Clone {
            input: "source/copy/".to_owned(),
            original_path: "source/".to_owned(),
            is_folder: true,
        };
        app.execute_clone();
        settle(&mut app).await;
        let objects = crate::bulk::snapshot(store.list(None)).await.unwrap();
        assert_eq!(objects.len(), 4);
        for path in [
            "source/a",
            "source/copy/a",
            "source/copy/b",
            "source/copy/copy/b",
        ] {
            store.head(&ObjectPath::from(path)).await.unwrap();
        }
        assert!(
            app.success_message
                .as_deref()
                .unwrap()
                .contains("Successfully cloned")
        );
        let super::BlobInfo::Folder {
            blob_count,
            total_size,
            ..
        } = App::folder_info(store.as_ref(), "source/", "source")
            .await
            .unwrap()
        else {
            panic!("expected folder statistics");
        };
        assert_eq!((blob_count, total_size), (4, 4));
        app.modal = Modal::DeleteConfirm {
            input: "source".to_owned(),
            target_path: "source/".to_owned(),
            target_name: "source".to_owned(),
            is_folder: true,
        };
        app.execute_delete();
        settle(&mut app).await;
        let remaining = crate::bulk::snapshot(store.list(None)).await.unwrap();
        assert_eq!(remaining, Vec::<object_store::ObjectMeta>::new());
        assert!(
            app.success_message
                .as_deref()
                .unwrap()
                .contains("Successfully deleted")
        );
    }

    async fn partial_download_app(destination: &std::path::Path) -> App {
        let mut app = upload_test_app("");
        let store = app.browsing().unwrap().object_store.clone();
        for path in ["folder/a", "folder/b", "folder/c"] {
            store
                .put(&ObjectPath::from(path), "payload".into())
                .await
                .unwrap();
        }
        app.refresh_files();
        settle(&mut app).await;
        // A directory at the second file's destination injects a write failure.
        std::fs::create_dir_all(destination.join("folder/b")).unwrap();
        app.modal = Modal::DownloadPicker {
            destination: Some(destination.to_owned()),
        };
        app.success_message = Some("Previous success".to_owned());
        app.start_download();
        settle(&mut app).await;
        app
    }

    #[tokio::test]
    async fn folder_download_reports_partial_write_failure() {
        let destination = tempfile::tempdir().unwrap();
        let app = partial_download_app(destination.path()).await;
        assert!(app.success_message.is_none());
        let error = app.error_message.unwrap();
        assert!(error.contains("Partially completed: 1/3 objects completed"));
        assert!(error.contains("folder/b"));
        assert_eq!(
            std::fs::read(destination.path().join("folder/a")).unwrap(),
            b"payload"
        );
        assert!(!destination.path().join("folder/c").exists());
        // The failed object's partial file is cleaned up, and none is left
        // behind for the one that succeeded.
        let leftovers: Vec<_> = std::fs::read_dir(destination.path().join("folder"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn bulk_error_renders_wrapped_cause() {
        use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
        for width in [70, 110] {
            for browsing in [false, true] {
                let mut app = if browsing {
                    upload_test_app("")
                } else {
                    test_app()
                };
                app.error_message = Some("Download failed: Partially completed: 1/3 objects completed; stopped at folder/b: injected failure".to_owned());
                let area = Rect::new(0, 0, width, 32);
                let mut buffer = Buffer::empty(area);
                (&app).render(area, &mut buffer);
                let text: String = buffer
                    .content
                    .iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect();
                assert!(text.contains("injected failure"));
            }
        }
    }

    /// Exercise the actual partial-download error with terminal input in tmux.
    #[tokio::test]
    #[ignore = "interactive terminal required"]
    async fn bulk_failure_interactive_preview() {
        let destination = tempfile::tempdir().unwrap();
        let app = partial_download_app(destination.path()).await;
        let mut terminal = ratatui::init();
        let result = app.run(&mut terminal).await;
        ratatui::restore();
        result.unwrap();
    }

    fn assert_delete_target(app: &mut App, expected: &str) {
        app.open_delete_dialog();
        let Modal::DeleteConfirm { target_path, .. } = &app.modal else {
            panic!("expected a delete confirmation");
        };
        assert_eq!(target_path, expected);
        app.close_modal();
    }

    #[tokio::test]
    async fn failed_navigation_preserves_directory_and_action_targets() {
        for go_up in [false, true] {
            let (dir, mut app) = navigation_test_app().await;
            if go_up {
                app.enter_directory();
                settle(&mut app).await;
            }
            let before = app.browsing().unwrap().clone();
            break_navigation_store(dir.path());
            if go_up {
                app.go_up_directory();
            } else {
                app.enter_directory();
            }
            settle(&mut app).await;
            assert!(app.error_message.take().unwrap().contains("failed"));
            let state = app.browsing().unwrap();
            assert_eq!(state.current_path(), before.current_path());
            assert_eq!(state.files(), before.files());
            assert_eq!(state.selected_index, before.selected_index);
            assert_delete_target(
                &mut app,
                if go_up {
                    "parent/child/data.txt"
                } else {
                    "parent/child/"
                },
            );

            // The actual keyboard handler must surface the error in the UI.
            app.handle_key_event(KeyEvent::from(if go_up {
                KeyCode::Left
            } else {
                KeyCode::Enter
            }))
            .unwrap();
            settle(&mut app).await;
            assert!(app.error_message.as_deref().unwrap().contains("failed"));
            assert_eq!(
                app.browsing().unwrap().current_path(),
                before.current_path()
            );
        }
    }

    #[tokio::test]
    async fn failed_refresh_and_navigation_preserve_search_and_selection() {
        let (dir, mut app) = navigation_test_app().await;
        app.enter_search_mode();
        app.browsing_mut().unwrap().selected_index = 1;
        let before = app.browsing().unwrap().clone();
        break_navigation_store(dir.path());
        app.refresh_files();
        settle(&mut app).await;
        assert!(app.error_message.take().unwrap().contains("Refresh failed"));
        app.go_up_directory();
        settle(&mut app).await;
        assert!(app.error_message.take().unwrap().contains("Go up failed"));
        let state = app.browsing().unwrap();
        assert_eq!(state.current_path(), before.current_path());
        assert_eq!(state.files(), before.files());
        assert_eq!(state.selected_index, 1);
        let Search::Files {
            query,
            all_files,
            all_file_items,
        } = &app.search
        else {
            panic!("failed navigation must preserve search");
        };
        assert_eq!(query, "");
        assert_eq!(all_files, before.files());
        assert_eq!(
            all_file_items
                .iter()
                .map(|item| &item.actual_name)
                .collect::<Vec<_>>(),
            before
                .directory
                .file_items
                .iter()
                .map(|item| &item.actual_name)
                .collect::<Vec<_>>()
        );
        assert_delete_target(&mut app, "parent/a.txt");
    }

    #[tokio::test]
    async fn successful_navigation_replaces_directory_and_resets_search() {
        let (_dir, mut app) = navigation_test_app().await;
        app.enter_search_mode();
        app.enter_directory();
        settle(&mut app).await;
        assert!(matches!(app.search, Search::Inactive));
        assert_eq!(app.browsing().unwrap().current_path(), "parent/child/");
        assert_delete_target(&mut app, "parent/child/data.txt");
        app.go_up_directory();
        settle(&mut app).await;
        assert_eq!(app.browsing().unwrap().current_path(), "parent/");
        assert_eq!(app.browsing().unwrap().selected_index, 0);
        assert_eq!(app.browsing().unwrap().directory.file_items.len(), 2);
        assert_delete_target(&mut app, "parent/child/");
        app.go_up_directory();
        settle(&mut app).await;
        assert_eq!(app.browsing().unwrap().current_path(), "");
        assert_delete_target(&mut app, "parent/");
    }

    /// Interactive reproduction of a listing failure with a populated browser.
    #[tokio::test]
    #[ignore = "interactive terminal required"]
    async fn navigation_failure_interactive_preview() {
        let (dir, app) = navigation_test_app().await;
        break_navigation_store(dir.path());
        let mut terminal = ratatui::init();
        let result = app.run(&mut terminal).await;
        ratatui::restore();
        result.unwrap();
    }

    /// Exercise the default split pane and selection previews in tmux.
    #[tokio::test]
    #[ignore = "interactive terminal required"]
    async fn browser_interactive_preview() {
        let (_dir, mut app) = navigation_test_app().await;
        let store = app.browsing().unwrap().object_store.clone();
        for (name, data) in [
            ("empty.txt", Vec::new()),
            ("résumé-with-a-very-long-name.txt", vec![b'x'; 1536]),
            ("data.json", br#"[{"name":"example","count":42}]"#.to_vec()),
            ("table.parquet", parquet_preview_bytes()),
        ] {
            store
                .put(&ObjectPath::from(format!("parent/{name}")), data.into())
                .await
                .unwrap();
        }
        app.refresh_files();
        settle(&mut app).await;
        let mut terminal = ratatui::init();
        let result = app.run(&mut terminal).await;
        ratatui::restore();
        result.unwrap();
    }

    fn parquet_preview_bytes() -> Vec<u8> {
        use arrow::{array::Int32Array, record_batch::RecordBatch};
        let batch = RecordBatch::try_from_iter([(
            "value",
            Arc::new(Int32Array::from(vec![42])) as arrow::array::ArrayRef,
        )])
        .unwrap();
        let mut bytes = Vec::new();
        let mut writer =
            parquet::arrow::ArrowWriter::try_new(&mut bytes, batch.schema(), None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
        bytes
    }

    #[tokio::test]
    async fn tab_switches_automatic_parquet_preview_without_taking_list_focus() {
        let (_dir, mut app) = navigation_test_app().await;
        let store = app.browsing().unwrap().object_store.clone();
        store
            .put(
                &ObjectPath::from("parent/table.parquet"),
                parquet_preview_bytes().into(),
            )
            .await
            .unwrap();
        app.refresh_files();
        settle(&mut app).await;
        app.browsing_mut().unwrap().selected_index = 2;
        app.sync_preview();
        let message = app.preview_tasks.recv().await.unwrap();
        if let Some(super::TaskEvent::PreviewLoaded(preview)) = app.preview_tasks.accept(message) {
            app.install_preview(*preview);
        }
        assert_eq!(app.parquet_preview_mode, Some(ParquetPreviewMode::Table));
        assert!(rendered(&app).contains("Table/metadata"));
        for expected in [ParquetPreviewMode::Metadata, ParquetPreviewMode::Table] {
            app.handle_key_event(KeyEvent::from(KeyCode::Tab)).unwrap();
            assert_eq!(app.parquet_preview_mode, Some(expected));
            app.sync_preview();
            assert_eq!(
                app.parquet_preview_mode,
                Some(expected),
                "redraw preserves mode"
            );
            assert!(!app.tasks.is_running());
            assert!(!app.preview_tasks.is_running());
            assert_eq!(app.ui.preview, super::PreviewPanel::Browsing);
        }
        app.modal = Modal::SortPicker;
        app.handle_key_event(KeyEvent::from(KeyCode::Tab)).unwrap();
        assert_eq!(app.parquet_preview_mode, Some(ParquetPreviewMode::Table));
        assert!(!rendered(&app).contains("Table/metadata"));
        app.close_modal();
        app.handle_key_event(KeyEvent::from(KeyCode::Char('k')))
            .unwrap();
        app.sync_preview();
        assert_eq!(app.browsing().unwrap().selected_index, 1);
        assert_eq!(app.parquet_preview_mode, None);
        assert!(!rendered(&app).contains("Table/metadata"));
    }

    #[tokio::test]
    async fn default_folder_preview_keeps_enter_and_info_available() {
        let (_dir, mut app) = navigation_test_app().await;
        app.sync_preview();
        assert_eq!(app.ui.preview, super::PreviewPanel::Browsing);
        assert!(!app.preview_tasks.is_running());
        let text = rendered(&app);
        for expected in [
            "child",
            "Virtual folder",
            "Enter folder",
            "i  Show folder info",
        ] {
            assert!(text.contains(expected), "missing {expected:?}");
        }
        app.handle_key_event(KeyEvent::from(KeyCode::Char('i')))
            .unwrap();
        settle(&mut app).await;
        assert!(matches!(
            app.modal,
            Modal::BlobInfo {
                info: super::BlobInfo::Folder { .. }
            }
        ));
        app.handle_key_event(KeyEvent::from(KeyCode::Esc)).unwrap();
        app.handle_key_event(KeyEvent::from(KeyCode::Enter))
            .unwrap();
        settle(&mut app).await;
        assert_eq!(app.browsing().unwrap().current_path(), "parent/child/");
        app.sync_preview();
        let message = app.preview_tasks.recv().await.unwrap();
        if let Some(super::TaskEvent::PreviewLoaded(preview)) = app.preview_tasks.accept(message) {
            app.install_preview(*preview);
        }
        assert!(rendered(&app).contains("payload"));
    }

    #[tokio::test]
    async fn automatic_preview_never_blocks_navigation_and_drops_stale_results() {
        let (gate, mut app) = gated_app().await;
        app.move_down();
        app.sync_preview();
        assert!(app.preview_tasks.is_running());
        assert!(!app.tasks.is_running());
        assert!(rendered(&app).contains("Loading preview"));

        // Finish the fetch but leave its result queued while the selection changes.
        gate.add_permits(10);
        let stale =
            tokio::time::timeout(std::time::Duration::from_secs(2), app.preview_tasks.recv())
                .await
                .unwrap()
                .unwrap();
        app.handle_key_event(KeyEvent::from(KeyCode::Char('k')))
            .unwrap();
        app.sync_preview();
        assert_eq!(app.browsing().unwrap().selected_index, 0);
        assert!(app.preview_tasks.accept(stale).is_none());
        assert!(app.preview_data.is_none());
        assert!(app.preview_error.is_none());
        let text = rendered(&app);
        assert!(text.contains("Virtual folder"));
        assert!(!text.contains("Loading preview"));
    }

    #[tokio::test]
    async fn file_sizes_stay_right_aligned_when_names_are_clipped() {
        use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
        let (_dir, mut app) = navigation_test_app().await;
        let store = app.browsing().unwrap().object_store.clone();
        store
            .put(
                &ObjectPath::from("parent/résumé-with-a-very-long-name.txt"),
                vec![b'x'; 1536].into(),
            )
            .await
            .unwrap();
        store
            .put(
                &ObjectPath::from("parent/empty.txt"),
                bytes::Bytes::new().into(),
            )
            .await
            .unwrap();
        app.refresh_files();
        settle(&mut app).await;

        for show_preview in [true, false] {
            app.ui.preview = if show_preview {
                super::PreviewPanel::Browsing
            } else {
                super::PreviewPanel::Hidden
            };
            for width in [60, 100, 140] {
                let mut buffer = Buffer::empty(Rect::new(0, 0, width, 30));
                app.render(buffer.area, &mut buffer);
                let list_width = if show_preview {
                    width * 40 / 100
                } else {
                    width
                };
                for (row, expected) in [(1, ""), (2, "7 B"), (3, "0 B"), (4, "1.5 KB")] {
                    let size: String = (list_width - 1 - u16::try_from(expected.len()).unwrap()
                        ..list_width - 1)
                        .map(|x| buffer[(x, row)].symbol())
                        .collect();
                    assert_eq!(size, expected, "width={width}, preview={show_preview}");
                }
                assert_eq!(
                    buffer[(list_width - 2, 1)].symbol(),
                    " ",
                    "folder has no size"
                );
            }
        }
    }

    #[tokio::test]
    async fn entering_percent_encoded_folder_lists_children() -> color_eyre::Result<()> {
        let store = object_store::memory::InMemory::new();
        let parent_path =
            "ducklake-data/main/model_output/reference_date=2026-06-24/output_type=sample";
        let folder_name = "target=inc%20covid%20ed%20visits";
        let blob_name = "part-000.parquet";
        let blob_path = format!("{parent_path}/{folder_name}/{blob_name}");

        store
            .put(&ObjectPath::parse(&blob_path)?, "payload".into())
            .await?;

        let mut app = test_app();
        app.session = Session::Browsing(BrowsingState {
            object_store: Arc::new(store),
            directory: LoadedDirectory {
                current_path: parent_path.to_string(),
                files: Vec::new(),
                file_items: Vec::new(),
                has_folder_stats: false,
            },
            selected_index: 0,
        });

        app.refresh_files();
        settle(&mut app).await;

        let state = app.browsing().expect("expected browsing session");
        assert_eq!(state.directory.file_items.len(), 1);
        assert_eq!(state.directory.file_items[0].actual_name, folder_name);
        assert_eq!(state.directory.file_items[0].kind, EntryKind::Folder);

        app.enter_directory();
        settle(&mut app).await;

        let state = app.browsing().expect("expected browsing session");
        assert_eq!(
            state.directory.current_path,
            format!("{parent_path}/{folder_name}/")
        );
        assert_eq!(state.directory.file_items.len(), 1);
        assert_eq!(state.directory.file_items[0].actual_name, blob_name);
        assert_eq!(state.directory.file_items[0].kind, EntryKind::File);

        Ok(())
    }

    #[test]
    fn entry_kind_folder_detection() {
        assert!(matches!(EntryKind::Folder, EntryKind::Folder));
        assert!(!matches!(EntryKind::Folder, EntryKind::File));
        assert!(matches!(EntryKind::File, EntryKind::File));
        assert!(!matches!(EntryKind::File, EntryKind::Folder));
    }

    #[test]
    fn parquet_tab_toggle_switches_views_and_resets_scroll() {
        let mut app = test_app();
        app.ui.preview = super::PreviewPanel::Focused;
        app.preview_file_type = Some(PreviewFileType::Parquet);
        app.parquet_preview_mode = Some(ParquetPreviewMode::Table);
        app.parquet_table_data = Some(TablePreview {
            headers: vec!["a".to_string()],
            column_types: Some(vec!["Int32".to_string()]),
            rows: vec![vec!["1".to_string()]],
            total_rows: 1,
            truncated: false,
            file_type: PreviewFileType::Parquet,
        });
        app.parquet_schema_data = Some(ParquetSchemaPreview {
            fields: vec!["a: Int32".to_string()],
            num_row_groups: 1,
            num_rows: 1,
            file_size: Some(10),
            created_by: None,
            note: None,
        });
        app.preview_data = app.parquet_table_data.clone().map(PreviewData::Table);
        app.preview_scroll = (5, 3);
        app.preview_selected_row = 4;

        app.toggle_parquet_preview_mode();

        assert_eq!(app.parquet_preview_mode, Some(ParquetPreviewMode::Metadata));
        assert_eq!(app.preview_scroll, (0, 0));
        assert_eq!(app.preview_selected_row, 0);
        assert!(matches!(
            app.preview_data,
            Some(PreviewData::ParquetSchema(_))
        ));

        app.preview_scroll = (2, 1);
        app.preview_selected_row = 2;
        app.toggle_parquet_preview_mode();

        assert_eq!(app.parquet_preview_mode, Some(ParquetPreviewMode::Table));
        assert_eq!(app.preview_scroll, (0, 0));
        assert_eq!(app.preview_selected_row, 0);
        assert!(matches!(app.preview_data, Some(PreviewData::Table(_))));
    }

    #[test]
    fn parquet_tab_toggle_is_noop_for_non_parquet() {
        let mut app = test_app();
        app.preview_file_type = Some(PreviewFileType::Csv);
        app.parquet_preview_mode = Some(ParquetPreviewMode::Table);
        app.toggle_parquet_preview_mode();
        assert_eq!(app.parquet_preview_mode, Some(ParquetPreviewMode::Table));
    }

    #[test]
    fn preview_scroll_right_is_bounded_for_tables() {
        let mut app = test_app();
        app.preview_data = Some(PreviewData::Table(TablePreview {
            headers: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            column_types: None,
            rows: vec![vec!["1".to_string(), "2".to_string(), "3".to_string()]],
            total_rows: 1,
            truncated: false,
            file_type: PreviewFileType::Csv,
        }));

        for _ in 0..10 {
            app.preview_scroll_right();
        }

        assert_eq!(app.preview_scroll.1, 2);
    }

    #[tokio::test]
    async fn open_preview_captures_keys_until_closed() {
        let (_dir, mut app) = navigation_test_app().await;
        app.browsing_mut().unwrap().selected_index = 1;
        let press = |code| KeyEvent::new(code, KeyModifiers::NONE);

        app.handle_key_event(press(KeyCode::Char('p'))).unwrap();
        settle(&mut app).await;
        assert!(app.ui.preview.is_visible());
        assert!(app.preview_data.is_some());

        // None of the file-list actions may reach the list hidden behind the preview.
        for key in ['r', '/', 'd', 'x', 'c', 's', 'u', 'i', 'y'] {
            app.handle_key_event(press(KeyCode::Char(key))).unwrap();
            assert!(app.ui.preview.is_visible(), "`{key}` closed the preview");
            assert!(
                matches!(app.modal, Modal::None),
                "`{key}` opened a modal over the preview"
            );
            assert!(
                matches!(app.search, Search::Inactive),
                "`{key}` started a search"
            );
            assert_eq!(app.browsing().unwrap().selected_index, 1);
            assert_eq!(app.success_message, None);
        }

        // Backspace closes the preview rather than leaving the container with it open.
        app.handle_key_event(press(KeyCode::Backspace)).unwrap();
        assert!(!app.ui.preview.is_visible());
        assert_eq!(app.browsing().unwrap().current_path(), "parent/");
    }

    // Folders a < b < c by name, b > c > a by newest blob (b's is nested and
    // newer than its sibling), and c > b > a by total size. The root file is
    // newest and largest but still follows the folders.
    async fn folder_sort_test_app(base: &str) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        for (path, size, year) in [
            ("a/x", 1, 2020),
            ("b/deep/y", 1, 2024),
            ("b/z", 1, 2019),
            ("c/w", 50, 2022),
            ("f.txt", 1000, 2025),
        ] {
            let path = dir.path().join(base).join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, vec![0_u8; size]).unwrap();
            let modified = Utc.with_ymd_and_hms(year, 1, 1, 0, 0, 0).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(modified.into())
                .unwrap();
        }
        let store = object_store::local::LocalFileSystem::new_with_prefix(dir.path()).unwrap();
        let mut app = test_app();
        let directory = App::load_directory(&store, &app.icons, app.sort_criteria, base.to_owned())
            .await
            .unwrap();
        app.session = Session::Browsing(BrowsingState {
            object_store: Arc::new(store),
            directory,
            selected_index: 0,
        });
        (dir, app)
    }

    fn item_names(app: &App) -> Vec<&str> {
        let state = app.browsing().expect("expected browsing session");
        state
            .directory
            .file_items
            .iter()
            .map(|item| item.actual_name.as_str())
            .collect()
    }

    #[tokio::test]
    async fn folders_sort_by_newest_blob_and_total_size() -> color_eyre::Result<()> {
        for base in ["", "nested/"] {
            let (_dir, mut app) = folder_sort_test_app(base).await;

            for (criteria, expected) in [
                (SortCriteria::DateModified, ["b", "c", "a", "f.txt"]),
                (SortCriteria::DateCreated, ["b", "c", "a", "f.txt"]),
                (SortCriteria::Size, ["c", "b", "a", "f.txt"]),
                (SortCriteria::Name, ["a", "b", "c", "f.txt"]),
            ] {
                app.apply_sort(criteria);
                settle(&mut app).await;
                assert_eq!(item_names(&app), expected, "{criteria:?} under {base:?}");
            }

            // Listing while date-sorted, as when navigating, also loads folder stats.
            app.apply_sort(SortCriteria::DateModified);
            app.refresh_files();
            settle(&mut app).await;
            assert_eq!(item_names(&app), ["b", "c", "a", "f.txt"], "{base:?}");
        }
        Ok(())
    }

    #[tokio::test]
    async fn date_sort_keeps_confirmed_search_filter() -> color_eyre::Result<()> {
        let (_dir, mut app) = folder_sort_test_app("").await;
        for code in [KeyCode::Char('/'), KeyCode::Char('b'), KeyCode::Enter] {
            app.handle_key_event(KeyEvent::from(code))?;
        }

        app.apply_sort(SortCriteria::DateModified);
        settle(&mut app).await;

        assert_eq!(item_names(&app), ["b"]);
        let state = app.browsing().expect("expected browsing session");
        assert!(state.directory.file_items[0].last_modified.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn failed_folder_scan_keeps_previous_sort() {
        let (dir, mut app) = navigation_test_app().await;
        break_navigation_store(dir.path());

        app.apply_sort(SortCriteria::DateModified);
        settle(&mut app).await;
        assert!(app.error_message.unwrap().contains("Failed to sort"));
        assert_eq!(app.sort_criteria, SortCriteria::Name);
    }

    /// Each store operation waits for a permit, so a test decides how far a task gets.
    #[derive(Debug)]
    struct GatedStore {
        inner: object_store::memory::InMemory,
        gate: Arc<tokio::sync::Semaphore>,
    }

    impl GatedStore {
        async fn pass(&self) {
            self.gate.acquire().await.unwrap().forget();
        }
    }

    impl std::fmt::Display for GatedStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "GatedStore({})", self.inner)
        }
    }

    #[async_trait::async_trait]
    impl object_store::ObjectStore for GatedStore {
        async fn put_opts(
            &self,
            location: &ObjectPath,
            payload: object_store::PutPayload,
            opts: object_store::PutOptions,
        ) -> object_store::Result<object_store::PutResult> {
            self.pass().await;
            self.inner.put_opts(location, payload, opts).await
        }

        async fn put_multipart_opts(
            &self,
            location: &ObjectPath,
            opts: object_store::PutMultipartOptions,
        ) -> object_store::Result<Box<dyn object_store::MultipartUpload>> {
            self.pass().await;
            self.inner.put_multipart_opts(location, opts).await
        }

        async fn get_opts(
            &self,
            location: &ObjectPath,
            options: object_store::GetOptions,
        ) -> object_store::Result<object_store::GetResult> {
            self.pass().await;
            self.inner.get_opts(location, options).await
        }

        fn delete_stream(
            &self,
            locations: BoxStream<'static, object_store::Result<ObjectPath>>,
        ) -> BoxStream<'static, object_store::Result<ObjectPath>> {
            let gate = self.gate.clone();
            let gated = locations.then(move |location| {
                let gate = gate.clone();
                async move {
                    gate.acquire().await.unwrap().forget();
                    location
                }
            });
            self.inner.delete_stream(gated.boxed())
        }

        fn list(
            &self,
            prefix: Option<&ObjectPath>,
        ) -> BoxStream<'static, object_store::Result<object_store::ObjectMeta>> {
            let gate = self.gate.clone();
            let listing = self.inner.list(prefix);
            futures::stream::once(async move {
                gate.acquire().await.unwrap().forget();
                listing
            })
            .flatten()
            .boxed()
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&ObjectPath>,
        ) -> object_store::Result<object_store::ListResult> {
            self.pass().await;
            self.inner.list_with_delimiter(prefix).await
        }

        async fn copy_opts(
            &self,
            from: &ObjectPath,
            to: &ObjectPath,
            options: object_store::CopyOptions,
        ) -> object_store::Result<()> {
            self.pass().await;
            self.inner.copy_opts(from, to, options).await
        }
    }

    /// Browse `parent/` of a store whose operations block until the gate gets permits.
    async fn gated_app() -> (Arc<tokio::sync::Semaphore>, App) {
        let inner = object_store::memory::InMemory::new();
        for path in ["parent/child/data.txt", "parent/a.txt"] {
            inner
                .put(&ObjectPath::from(path), "payload".into())
                .await
                .unwrap();
        }
        let mut app = test_app();
        let directory =
            App::load_directory(&inner, &app.icons, app.sort_criteria, "parent/".to_owned())
                .await
                .unwrap();
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        app.session = Session::Browsing(BrowsingState {
            object_store: Arc::new(GatedStore {
                inner,
                gate: gate.clone(),
            }),
            directory,
            selected_index: 0,
        });
        (gate, app)
    }

    /// Every operation blocks forever, to exercise the spinner, input limits,
    /// and cancel in tmux without Azure credentials.
    #[tokio::test]
    #[ignore = "interactive terminal required"]
    async fn blocked_operation_interactive_preview() {
        let (_gate, app) = gated_app().await;
        let mut terminal = ratatui::init();
        let result = app.run(&mut terminal).await;
        ratatui::restore();
        result.unwrap();
    }

    #[tokio::test]
    async fn canceled_directory_load_leaves_state_unchanged() {
        for cancel in [
            KeyEvent::from(KeyCode::Esc),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let (gate, mut app) = gated_app().await;
            app.enter_search_mode();
            let before = app.browsing().unwrap().clone();

            app.enter_directory();
            assert!(app.tasks.is_running());
            app.handle_key_event(cancel).unwrap();

            assert!(!app.tasks.is_running());
            assert!(app.running, "canceling must not quit");
            assert!(app.success_message.as_deref().unwrap().contains("Canceled"));
            // Even if the listing would now succeed, its result is never applied.
            gate.add_permits(10);
            tokio::task::yield_now().await;
            let state = app.browsing().unwrap();
            assert_eq!(state.current_path(), before.current_path());
            assert_eq!(state.files(), before.files());
            assert_eq!(state.selected_index, before.selected_index);
            assert!(matches!(app.search, Search::Files { .. }));
        }
    }

    #[tokio::test]
    async fn only_cancel_and_quit_are_handled_while_running() {
        let (_gate, mut app) = gated_app().await;
        app.enter_directory();
        for key in ['j', 'x', 'c', 'd', 'u', 's', 'i', 'p', 'r', '/', 'l', 'h'] {
            app.handle_key_event(KeyEvent::from(KeyCode::Char(key)))
                .unwrap();
            assert!(app.tasks.is_running(), "`{key}` stopped the operation");
            assert!(matches!(app.modal, Modal::None), "`{key}` opened a modal");
            assert!(matches!(app.search, Search::Inactive));
            assert_eq!(app.browsing().unwrap().selected_index, 0);
            assert_eq!(app.ui.preview, super::PreviewPanel::Browsing);
        }
        app.handle_key_event(KeyEvent::from(KeyCode::Char('q')))
            .unwrap();
        assert!(!app.running);
    }

    #[tokio::test]
    async fn stale_task_result_is_ignored() {
        let (gate, mut app) = gated_app().await;
        gate.add_permits(1);
        app.enter_directory();
        // The first listing finishes, but is canceled before its result is applied.
        let stale = app.tasks.recv().await.unwrap();
        app.tasks.cancel();
        app.refresh_files();

        app.apply_task_message(stale);

        assert!(app.tasks.is_running(), "a stale result ended the operation");
        assert_eq!(app.browsing().unwrap().current_path(), "parent/");
        gate.add_permits(1);
        settle(&mut app).await;
        assert_eq!(app.browsing().unwrap().current_path(), "parent/");
        assert_eq!(app.browsing().unwrap().directory.file_items.len(), 2);
    }

    fn rendered(app: &App) -> String {
        use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
        let area = Rect::new(0, 0, 100, 30);
        let mut buffer = Buffer::empty(area);
        app.render(area, &mut buffer);
        buffer
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    #[tokio::test]
    async fn running_operation_is_shown_in_the_footer() {
        let (_gate, mut app) = gated_app().await;
        assert!(!rendered(&app).contains("Esc/Ctrl-C"));
        app.enter_directory();
        let text = rendered(&app);
        assert!(text.contains("Loading /parent/child"));
        assert!(text.contains("Esc/Ctrl-C"));
        assert!(text.contains("Cancel"));
        assert!(!text.contains("Download"));
    }

    #[tokio::test]
    async fn event_loop_handles_input_while_an_operation_is_blocked() {
        let (_gate, mut app) = gated_app().await;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        // The listing never gets a permit, so the loop must not wait for it.
        let keys = [KeyCode::Enter, KeyCode::Char('j'), KeyCode::Esc]
            .map(|code| Ok(ratatui::crossterm::event::Event::Key(KeyEvent::from(code))));

        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            app.run_loop(&mut terminal, futures::stream::iter(keys)),
        )
        .await
        .expect("event loop blocked on the running operation")
        .unwrap();

        assert!(!app.tasks.is_running());
        assert_eq!(app.browsing().unwrap().current_path(), "parent/");
        assert_eq!(app.browsing().unwrap().selected_index, 0);
        assert!(app.success_message.as_deref().unwrap().contains("Canceled"));
    }

    #[tokio::test]
    async fn preview_load_shows_progress_and_cancel_closes_the_panel() {
        let (gate, mut app) = gated_app().await;
        app.browsing_mut().unwrap().selected_index = 1;

        app.load_preview();
        assert!(rendered(&app).contains("Loading preview"));
        app.handle_key_event(KeyEvent::from(KeyCode::Esc)).unwrap();
        assert!(!app.ui.preview.is_visible());
        assert!(!app.tasks.is_running());

        gate.add_permits(10);
        app.load_preview();
        settle(&mut app).await;
        assert!(app.ui.preview.is_visible());
        assert!(matches!(app.preview_data, Some(PreviewData::Text(_))));
    }

    #[tokio::test]
    async fn blob_info_arrives_as_a_modal_or_an_error() {
        let (gate, mut app) = gated_app().await;
        gate.add_permits(10);
        app.show_blob_info();
        settle(&mut app).await;
        assert!(matches!(
            app.modal,
            Modal::BlobInfo {
                info: super::BlobInfo::Folder {
                    blob_count: 1,
                    total_size: 7,
                    ..
                }
            }
        ));

        app.close_modal();
        app.browsing_mut().unwrap().selected_index = 1;
        app.show_blob_info();
        settle(&mut app).await;
        assert!(matches!(
            app.modal,
            Modal::BlobInfo {
                info: super::BlobInfo::File { size: 7, .. }
            }
        ));

        let (dir, mut app) = navigation_test_app().await;
        break_navigation_store(dir.path());
        app.show_blob_info();
        settle(&mut app).await;
        assert!(matches!(app.modal, Modal::None));
        assert!(
            app.error_message
                .unwrap()
                .contains("Failed to get blob info: Folder listing failed")
        );
    }

    #[tokio::test]
    async fn canceled_folder_clone_reports_progress_and_refreshes() {
        let (gate, mut app) = gated_app().await;
        app.modal = Modal::Clone {
            input: "parent/copy/".to_owned(),
            original_path: "parent/".to_owned(),
            is_folder: true,
        };
        app.handle_clone_dialog_key_event(KeyEvent::from(KeyCode::Enter))
            .unwrap();
        // Let the listing through, leaving the first copy blocked.
        gate.add_permits(1);
        while !app.tasks.status().unwrap().contains("(0/2)") {
            let message = app.tasks.recv().await.unwrap();
            app.apply_task_message(message);
        }
        assert!(rendered(&app).contains("Cloning parent (0/2)"));

        app.handle_key_event(KeyEvent::from(KeyCode::Esc)).unwrap();
        // The copy in flight is allowed to finish; nothing after it starts.
        assert!(app.tasks.status().unwrap().contains("stopping"));
        gate.add_permits(100);
        settle(&mut app).await;

        let error = app.error_message.as_deref().unwrap();
        assert!(error.contains("Clone failed: Partially completed: 1/2 objects completed"));
        assert!(error.contains("canceled"));
        assert!(app.success_message.is_none());
        assert_eq!(item_names(&app), ["child", "copy", "a.txt"]);
    }

    #[tokio::test]
    async fn canceled_upload_reports_and_leaves_no_blob() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("new.txt");
        std::fs::write(&source, "payload").unwrap();
        let (gate, mut app) = gated_app().await;
        app.upload_file(source);
        app.handle_key_event(KeyEvent::from(KeyCode::Esc)).unwrap();
        gate.add_permits(100);
        settle(&mut app).await;
        assert!(
            app.error_message
                .take()
                .unwrap()
                .contains("Upload failed: canceled")
        );
        assert_eq!(item_names(&app), ["child", "a.txt"]);
    }

    #[tokio::test]
    async fn single_file_download_reports_success() {
        let destination = tempfile::tempdir().unwrap();
        let (gate, mut app) = gated_app().await;
        gate.add_permits(100);
        app.browsing_mut().unwrap().selected_index = 1;
        app.modal = Modal::DownloadPicker {
            destination: Some(destination.path().to_owned()),
        };
        app.start_download();
        settle(&mut app).await;
        assert!(app.success_message.unwrap().contains("downloaded a.txt"));
        assert_eq!(
            std::fs::read(destination.path().join("a.txt")).unwrap(),
            b"payload"
        );
    }

    #[tokio::test]
    async fn container_listing_result_is_applied_when_it_arrives() {
        let mut app = test_app();
        let names =
            |app: &App| -> Vec<String> { app.containers.iter().map(|c| c.name.clone()).collect() };
        app.finish_load_containers(Ok(["zebra", "alpha"]
            .into_iter()
            .map(|name| super::ContainerInfo {
                name: name.to_owned(),
            })
            .collect()));
        assert_eq!(names(&app), ["alpha", "zebra"]);
        app.finish_load_containers(Err("offline".to_owned()));
        assert_eq!(names(&app), ["alpha", "zebra"]);
        assert!(
            app.error_message
                .unwrap()
                .contains("Failed to list containers: offline")
        );
    }

    #[tokio::test]
    async fn listing_is_shown_before_its_folder_scan_and_scans_are_cached() {
        let (gate, mut app) = gated_app().await;
        let child_size = |app: &App| app.browsing().unwrap().directory.file_items[0].size;

        // One permit: the scan.
        gate.add_permits(1);
        app.apply_sort(SortCriteria::Size);
        settle(&mut app).await;
        assert_eq!(child_size(&app), Some(7));

        // Two permits: one listing each way. The child has no folders to scan,
        // and coming back uses the cached scan.
        gate.add_permits(2);
        app.enter_directory();
        settle(&mut app).await;
        assert_eq!(app.browsing().unwrap().current_path(), "parent/child/");
        app.go_up_directory();
        settle(&mut app).await;
        assert_eq!(child_size(&app), Some(7));

        // Refreshing rescans, but shows the listing while the scan is running.
        app.handle_key_event(KeyEvent::from(KeyCode::Char('r')))
            .unwrap();
        gate.add_permits(1);
        let listing = app.tasks.recv().await.unwrap();
        app.apply_task_message(listing);
        assert_eq!(item_names(&app), ["child", "a.txt"]);
        assert_eq!(child_size(&app), None);
        assert!(app.tasks.status().unwrap().contains("Scanning folders"));

        // Canceling the scan keeps the listing usable.
        app.handle_key_event(KeyEvent::from(KeyCode::Esc)).unwrap();
        assert_eq!(item_names(&app), ["child", "a.txt"]);
        assert_eq!(app.sort_criteria, SortCriteria::Size);

        // Choosing the sort again retries the scan.
        gate.add_permits(1);
        app.apply_sort(SortCriteria::Size);
        settle(&mut app).await;
        assert_eq!(child_size(&app), Some(7));
    }

    #[tokio::test]
    async fn changing_the_container_drops_cached_folder_scans() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("new.txt");
        std::fs::write(&source, "payload").unwrap();
        let (gate, mut app) = gated_app().await;
        gate.add_permits(100);
        app.apply_sort(SortCriteria::Size);
        settle(&mut app).await;
        assert!(app.folder_stats_cache.contains_key("parent/"));

        app.browsing_mut().unwrap().selected_index = 0;
        app.enter_directory();
        settle(&mut app).await;
        app.upload_file(source);
        settle(&mut app).await;
        app.go_up_directory();
        settle(&mut app).await;

        // The upload into the child folder is reflected in the parent's scan.
        assert_eq!(
            app.browsing().unwrap().directory.file_items[0].size,
            Some(14)
        );
    }

    proptest! {
        #[test]
        fn apply_sort_preserves_items_and_folders_stay_first(
            items in prop::collection::vec(arb_file_item(), 0..40),
            criteria in arb_sort_criteria(),
        ) {
            type ItemKey = (
                String,
                String,
                bool,
                Option<u64>,
                Option<chrono::DateTime<chrono::Utc>>,
                Option<chrono::DateTime<chrono::Utc>>,
            );

            let mut sorted = items.clone();
            App::sort_file_items_static(&mut sorted, criteria);

            prop_assert_eq!(sorted.len(), items.len());

            let before = items.iter().fold(BTreeMap::<ItemKey, usize>::new(), |mut acc, item| {
                *acc.entry((
                    item.display_name.clone(),
                    item.actual_name.clone(),
                    matches!(item.kind, EntryKind::Folder),
                    item.size,
                    item.last_modified,
                    item.created,
                )).or_default() += 1_usize;
                acc
            });

            let after = sorted
                .iter()
                .fold(BTreeMap::<ItemKey, usize>::new(), |mut acc, item| {
                *acc.entry((
                    item.display_name.clone(),
                    item.actual_name.clone(),
                    matches!(item.kind, EntryKind::Folder),
                    item.size,
                    item.last_modified,
                    item.created,
                ))
                .or_default() += 1_usize;
                    acc
                });

            prop_assert_eq!(before, after);

            let first_file = sorted.iter().position(|item| item.kind == EntryKind::File);
            if let Some(first_file) = first_file {
                prop_assert!(sorted[first_file..].iter().all(|item| item.kind == EntryKind::File));
            }
        }

        #[test]
        fn apply_file_search_keeps_subset_and_resets_selection(
            items in prop::collection::vec(arb_file_item(), 0..30),
            query in "[A-Za-z0-9_./-]{0,8}",
        ) {
            let mut app = test_app();
            let all_files: Vec<String> = items.iter().map(|item| item.display_name.clone()).collect();

            app.session = Session::Browsing(BrowsingState {
                object_store: std::sync::Arc::new(object_store::memory::InMemory::new()),
                directory: LoadedDirectory {
                    current_path: String::new(),
                    files: all_files.clone(),
                    file_items: items.clone(),
                    has_folder_stats: false,
                },
                selected_index: 5,
            });
            app.search = Search::Files {
                query: query.clone(),
                all_files: all_files.clone(),
                all_file_items: items.clone(),
            };

            app.apply_file_search(&query);

            let Session::Browsing(state) = &app.session else {
                prop_assert!(false, "expected browsing session");
                return Ok(());
            };

            prop_assert_eq!(state.selected_index, 0);
            prop_assert_eq!(
                state.directory.files.clone(),
                state.directory.file_items.iter().map(|item| item.display_name.clone()).collect::<Vec<_>>()
            );

            if query.is_empty() {
                prop_assert_eq!(state.directory.file_items.len(), items.len());
            } else {
                let lowered = query.to_lowercase();
                prop_assert!(state.directory.file_items.iter().all(|item| item.actual_name.to_lowercase().contains(&lowered)));
                prop_assert!(state.directory.file_items.len() <= items.len());
            }
        }

        #[test]
        fn preview_horizontal_scroll_stays_within_table_bounds(
            cols in 0_usize..12,
            moves in 0_usize..32,
        ) {
            let mut app = test_app();
            app.preview_data = Some(PreviewData::Table(TablePreview {
                headers: (0..cols).map(|i| format!("col{i}")).collect(),
                column_types: None,
                rows: if cols == 0 {
                    Vec::new()
                } else {
                    vec![(0..cols).map(|i| format!("v{i}")).collect()]
                },
                total_rows: 1,
                truncated: false,
                file_type: PreviewFileType::Csv,
            }));

            for _ in 0..moves {
                app.preview_scroll_right();
            }

            prop_assert!(app.preview_scroll.1 <= cols.saturating_sub(1));

            for _ in 0..moves {
                app.preview_scroll_left();
            }

            prop_assert_eq!(app.preview_scroll.1, 0);
        }
    }
}
