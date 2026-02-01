# TODO: Refactor boolean-heavy structs to use Rust’s type system

This checklist turns the refactoring ideas into concrete, incremental steps. The goal is to reduce “boolean soup” by encoding invariants in enums (sum types), newtypes, and (optionally) typestate.

---

## 0) Prep: baseline + invariants inventory

1. **Create a short list of invariants (things that should never be possible).**
   - Examples:
     - “Downloading but `download_progress` is `None`”
     - “Two modals open at once”
     - “Browsing blobs without an initialized `object_store`”
     - “Folder metadata fields set on a file item”
2. **Add/confirm tests or manual verification steps** you can run after each stage:
   - `cargo test`
   - Run the app and smoke-test key flows: container selection, browse, preview, download, clone, delete, search, sort.
3. **Do refactors in small commits** and keep behavior identical as long as possible.

---

## 1) Replace “async/busy” boolean clusters with a single `AsyncOp` enum (DONE)

### Goal
Eliminate parallel flags like “is_downloading/is_cloning/is_deleting/is_loading” and ensure progress data is always present when the operation is active.

### Steps
1. **Identify all fields representing async/busy state** (e.g., `is_loading`, `is_downloading`, `download_progress`, `is_cloning`, `clone_progress`, `is_deleting`, `delete_progress`).
2. **Define an enum** (name example: `AsyncOp`) with variants for each operation:
   - `None`
   - `Loading` (if it has no extra data)
   - `Downloading(DownloadProgress)`
   - `Cloning(CloneProgress)`
   - `Deleting(DeleteProgress)`
3. **Replace the old fields** with a single field:
   - `pub async_op: AsyncOp`
4. **Add helper methods** on `App` to make call sites clean:
   - `fn is_busy(&self) -> bool`
   - `fn set_downloading(&mut self, progress: DownloadProgress)`
   - `fn clear_async_op(&mut self)`
5. **Update all code paths** that set or read the old booleans:
   - Convert `if self.is_downloading { ... }` to `match self.async_op { AsyncOp::Downloading(_) => ..., _ => ... }`
   - Convert “set bool + set Option progress” into “set enum variant with embedded progress”
6. **Update UI rendering** that depends on these flags to match on the enum.
7. **Verify invariant enforcement**:
   - Confirm there is no longer a representable state “downloading without progress”.

---

## 2) Replace multiple “show_*” booleans with a single `Modal` enum (DONE)

### Goal
Make “only one modal can be open at a time” a compile-time-checked property, and keep modal-specific data inside the modal variant.

### Steps
1. **List all modal/popup booleans and their associated fields**:
   - e.g., `show_delete_dialog` + `delete_input/delete_target_*`
   - `show_clone_dialog` + `clone_input/clone_original_path/clone_is_folder`
   - `show_download_picker` + `download_destination`
   - `show_sort_popup` + (anything else needed)
   - `show_blob_info_popup` + `current_blob_info`
2. **Define a single enum** `Modal` with variants holding their required data:
   - `None`
   - `BlobInfo(BlobInfo)` (or only what the UI needs)
   - `DownloadPicker { destination: Option<PathBuf> }`
   - `SortPicker { /* current selection, etc */ }`
   - `Clone { input: String, original_path: String, is_folder: bool }`
   - `DeleteConfirm { input: String, target_path: String, target_name: String, is_folder: bool }`
3. **Replace all `show_*` fields + related `Option<T>` fields** with:
   - `pub modal: Modal`
4. **Create constructor helpers** on `Modal` or `App`:
   - `fn open_delete_confirm(&mut self, ...)`
   - `fn close_modal(&mut self)`
5. **Update key-handling**:
   - Where you currently do `if self.show_delete_dialog { ... }`, switch to `match self.modal { Modal::DeleteConfirm { .. } => ..., _ => ... }`.
6. **Update rendering**:
   - Render modals by matching `self.modal`.
7. **Verify invariant enforcement**:
   - Confirm it is no longer possible to have two `show_*` flags true simultaneously.

---

## 3) Replace “search_mode + query + all_* lists” with a typed `Search` state (DONE)

### Goal
Eliminate invalid combinations like `search_mode=false` while `search_query` is non-empty, and reduce “all_files/all_file_items” duplication pitfalls.

### Steps
1. **Identify all search-related fields**:
   - Container search: `container_search_mode`, `container_search_query`, `all_containers`
   - File search: `search_mode`, `search_query`, `all_files`, `all_file_items`
2. **Define a single enum**:
   - `Search::Inactive`
   - `Search::Containers { query: String }`
   - `Search::Files { query: String }`
3. **Replace the booleans + query strings** with:
   - `pub search: Search`
4. **Make filtering a pure function** where possible:
   - `fn filtered_containers(&self) -> &[ContainerInfo]` or `Vec<ContainerInfo>`
   - `fn filtered_files(&self) -> Vec<FileItem>`
5. **Update event handling**:
   - Enter search → set `self.search = Search::Containers { query: String::new() }` or `Files`
   - Exit search → set `Inactive`
6. **Remove/limit “cached filtered copies”** if feasible.
   - If you keep cached lists, ensure they’re updated only from the `Search` state transitions, not from scattered conditionals.
7. **Verify invariant enforcement**:
   - Make sure there’s exactly one authoritative place to know whether you’re searching and what the query is.

---

## 4) Encode “requires object_store” using session/typestate (DONE)

### Goal
Ensure the code cannot call browsing operations unless the Azure client (`object_store`) is initialized.

### Option A (lighter): `enum Session`
1. **Create `enum Session`**:
   - `Selecting { /* container list, selection index, etc */ }`
   - `Browsing { object_store: Arc<dyn ObjectStore>, current_path: String, /* browsing data */ }`
2. **Move fields** that only make sense during browsing into the `Browsing` variant.
3. **Replace `App.state` and `App.object_store: Option<_>`** with:
   - `pub session: Session`
4. **Update functions**:
   - Functions like `refresh_files`, `load_preview`, etc. should `match &mut self.session` and only run in `Browsing`.
5. **Verify**:
   - There is no `Option<Arc<dyn ObjectStore>>` left; the type ensures presence.

### Option B (stronger): typestate `App<Mode>`
1. **Define**:
   - `struct App<Mode> { common: CommonFields, mode: Mode }`
   - `struct Selecting { ... }`
   - `struct Browsing { object_store: Arc<dyn ObjectStore>, ... }`
2. **Refactor constructors/transitions**:
   - `App<Selecting>::select_container(self) -> App<Browsing>`
3. **Update call sites**:
   - Make browsing functions require `&mut App<Browsing>`.
4. **Verify**:
   - It’s impossible to compile code that browses without selecting.

---

## 5) Replace “is_folder: bool” with an enum (or split structs) (DONE)

### Goal
Prevent invalid metadata combinations like “file with folder-only fields” or vice versa.

### Steps
1. **Locate all `is_folder: bool` usage** (e.g., `BlobInfo`, `FileItem`, and any others).
2. **Decide representation**:
   - Minimal: `enum EntryKind { File, Folder }`
   - Better: `enum Entry { File(FileMeta), Folder(FolderMeta) }`
3. **Refactor structs**:
   - For `BlobInfo`, split into:
     - `BlobInfo::File { name, size, last_modified, etag }`
     - `BlobInfo::Folder { name, blob_count, total_size }`
4. **Update constructors**:
   - Anywhere you currently create a `BlobInfo { is_folder: true, ... }`, create the folder variant.
5. **Update consumers**:
   - Replace `if blob.is_folder { ... } else { ... }` with `match blob { BlobInfo::Folder{..} => ..., BlobInfo::File{..} => ... }`
6. **Verify**:
   - Ensure folder-only data cannot exist on file variants.

---

## 6) If booleans are truly independent toggles, group them (or use bitflags) (DONE)

### Goal
Avoid dozens of boolean fields while still representing real, independent toggles.

### Steps
1. **Audit each boolean**:
   - If it is mutually exclusive with others → it belongs in an `enum` (not here).
   - If it’s truly independent (feature toggles / settings) → group them.
2. **Create `struct UiToggles`** (or similar):
   - `struct UiToggles { show_preview: bool, ... }`
3. **Move toggles into that struct**:
   - Replace scattered `pub show_preview: bool` with `pub ui: UiToggles`
4. **Optional**: If you need compact/fast set operations, replace with `bitflags`.
5. **Verify**:
   - Code readability improves (fewer top-level fields), without losing meaning.

---

## 7) Refactor strategy: do it safely and incrementally

1. **Start with the most obviously invalid combinations** (usually modals and async operations).
2. **Introduce enums while keeping adapters**:
   - Temporarily keep old getters like `fn is_downloading(&self) -> bool { matches!(self.async_op, AsyncOp::Downloading(_)) }`
3. **Convert one feature end-to-end**:
   - Example: convert delete dialog to a `Modal::DeleteConfirm` variant and remove old fields.
4. **Remove dead fields** only after all references are migrated.
5. **Run `cargo fmt`, `cargo clippy`, `cargo test`** after each step.
6. **Add tiny regression tests** for invariants when possible (even if UI-driven, unit-test the pure state transitions).

---

## Suggested order of operations (recommended)

1. **Modal enum refactor** (big win, reduces many booleans fast)
2. **AsyncOp enum refactor** (prevents inconsistent progress state)
3. **Search state refactor** (clean separation of modes + queries)
4. **`is_folder` refactor** (domain correctness)
5. **Session/typestate refactor** (largest structural change; do after simpler wins)
6. **Group remaining independent toggles** (final cleanup)
