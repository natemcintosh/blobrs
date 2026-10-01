# Blobrs 0.5.0

This release adds local file uploads and persistent container favorites, and switches storage authentication to your Azure CLI login.

## New features

- **Upload local files:** Press `u` while browsing a container or folder to open the upload prompt and choose a local file. The prompt shows the destination. Uploads keep the local filename and refuse to overwrite existing blobs.
- **Favorite containers:** Press `f` in the container list to pin or unpin a container. Favorites appear first, persist between sessions, and are stored separately for each storage account. Finish searching with Enter before toggling a favorite.
- **Azure CLI authentication:** Use your existing `az login` session, including managed identity login via `az login --identity`, instead of configuring storage account keys.

## Fixes and improvements

- Preserve encoded blob paths when accessing objects.
- Fix Linux/Wayland file chooser compatibility by using the desktop portal backend.
- Update dependencies, including Arrow/Parquet 58, object_store 0.13, and Ratatui 0.30, and replace yanked dependency versions.
- Expand property and regression tests and simplify terminal event handling.
- Document standalone installation, environment setup, and required Azure data permissions.

## Upgrading and setup

Authentication has changed: sign in with `az login` (or `az login --identity`) and set `AZURE_STORAGE_ACCOUNT` to the storage account name. Storage keys and a container environment variable are no longer needed. The binary does not load `.env` files; `just run` loads the repository's `.env` for development.

Your identity needs **Storage Blob Data Reader** for browsing, previews, and downloads, or **Storage Blob Data Contributor** for uploads, cloning, and deletion. Assign the role at the storage account scope or above so Blobrs can list containers.

Uploads read the selected file into memory and block the UI until complete. Linux upload/download file choosers require a working desktop portal.

The Cargo package version is now aligned with GitHub releases at **0.5.0**; earlier GitHub releases through 0.4.1 still carried Cargo version 0.3.0.

**Full changelog:** https://github.com/natemcintosh/blobrs/compare/v0.4.1...v0.5.0
