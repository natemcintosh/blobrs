# Blobrs 0.6.0

This release keeps the TUI responsive during network operations, refreshes its styling and command bar, and improves folder sorting and operation reliability. These changes are since v0.5.0.

## New features

- **Cancelable background operations:** Container and directory listings, folder scans, previews, metadata, uploads, downloads, cloning, deletion, and file dialogs now run in the background. The command bar shows activity and folder-operation progress. While an operation runs, press Esc or Ctrl-C to cancel, or `q` to quit.
- **Refreshed interface:** A consistent color palette, clearer selection styling, and compact panel titles accompany a context-sensitive command bar. Shortcuts wrap together to fit the terminal width and change for searches, previews, dialogs, and running operations.
- **Folder sorting by contents:** Date sorts order folders by their newest blob at any depth; size sorts use the total size of their blobs. Name sorting requires no recursive scan. Directory contents appear before the background scan finishes, and scans are cached while the container is open.
- **Streaming downloads:** Downloads stream to temporary partial files and are renamed only when complete, reducing memory use and avoiding truncated destination files after failure or cancellation.

## Fixes and improvements

- Keep the current directory and listing consistent when navigation fails or is canceled, and ignore results from canceled or replaced operations.
- Report folder-listing failures and partial completion during bulk operations instead of failing silently. Snapshot folder listings before processing, including overlapping folder clones.
- Preserve active search filters when changing sort order, and keep the previous sort if a folder scan fails.
- Capture all keys while a preview is open so preview input cannot trigger actions on the underlying listing.
- Keep the clipboard alive so paths copied with `y` can be pasted on X11 without a clipboard manager.
- Update object_store to 0.14, dirs to 7, and refresh futures, reqwest, bytes, tempfile, and CI dependencies.
- Expand regression and property tests, enforce strict pedantic Clippy checks in CI, restore build caches before compilation, and add `just pedantic` and `just install` development commands.

## Upgrading and usage notes

Blobrs supports **Linux only**; documentation and CI now explicitly reflect this. Install this release from source with:

```bash
cargo install --git https://github.com/natemcintosh/blobrs.git --tag v0.6.0 --locked --force
```

Continue using `az login` (or `az login --identity`) and set `AZURE_STORAGE_ACCOUNT` to your storage account name. Upload and download file choosers require a working desktop portal.

One operation runs at a time; while it runs, only cancellation and quitting are accepted. Cancellation does not roll back completed changes: bulk folder operations stop between objects and report partial completion. Uploads still read the selected file into memory and never overwrite existing blobs. On X11 without a clipboard manager, copied paths remain available only while Blobrs is running.

**Full changelog:** https://github.com/natemcintosh/blobrs/compare/v0.5.0...v0.6.0
