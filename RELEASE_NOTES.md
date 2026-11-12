# Blobrs 0.7.0

This release adds saved storage accounts, automatic split-pane previews, and full-file previews with disk-backed Parquet paging. It also makes file metadata and keyboard focus easier to see. These changes are since v0.6.0.

## New features

- **Saved storage accounts:** Start Blobrs without setting `AZURE_STORAGE_ACCOUNT`, add an account with `a`, and choose saved accounts with ↑/↓ and Enter. From the container list, press `a` or Esc to switch accounts. Account names are validated and saved in `$XDG_CONFIG_HOME/blobrs/config.json` (defaulting to `~/.config/blobrs/config.json`); concurrent instances preserve each other's additions.
- **Automatic split-pane previews:** Browsing now opens a preview beside the file list by default. Selecting another file updates the preview in the background without blocking navigation. Press `p` to focus the preview, and use Tab to switch Parquet table/metadata views, including while the file list has focus.
- **Full-file previews:** In a focused preview, press `f` to download the complete CSV, TSV, JSON, text, or Parquet file and browse beyond the automatic row/line limits. Full Parquet previews also support files larger than the automatic 16 MB table-preview limit. The full-preview choice applies only to the current preview and can be canceled.
- **Disk-backed Parquet paging:** Parquet previews cache downloads in a temporary file and decode a small page of rows as you scroll. Upgrading to a full preview reuses that cache, which is deleted when the preview closes.
- **File metadata columns:** The browser shows file sizes and last-modified times in UTC, adapting the date column to the available width. Cached folder summaries show recursive blob counts and newest-blob times after a date/size sort has loaded them.

## Fixes and improvements

- Make the active file-list or preview pane clearer with focused panel borders and context-specific keyboard hints.
- Give each command shortcut and its description a shared box, and bring popups, status messages, and Parquet metadata into the shared UI theme.
- Keep selection previews responsive and discard results from files that are no longer selected.
- Keep saved-account input separate from navigation shortcuts, preserve existing account configuration on invalid data or unsupported versions, and keep favorites scoped to each storage account.
- Update the quick start and development commands for saved-account selection; `AZURE_STORAGE_ACCOUNT` is now optional, and `just run` no longer requires it.

## Upgrading and usage notes

Blobrs supports **Linux only**. Install this release from source with:

```bash
cargo install --git https://github.com/natemcintosh/blobrs.git --tag v0.7.0 --locked --force
```

Continue using `az login` (or `az login --identity`) for credentials. On launch, choose a saved account or press `a` to add one. If `AZURE_STORAGE_ACCOUNT` is set, its account name is added to the saved list automatically. Credentials are not stored in the account configuration.

Automatic previews retain download and display limits. Full previews download the entire file; non-Parquet full previews hold the downloaded file and parsed contents in memory. Folder summaries reuse scans requested by date/size sorting; normal browsing does not trigger recursive scans just to populate metadata columns.

**Full changelog:** https://github.com/natemcintosh/blobrs/compare/v0.6.0...v0.7.0
