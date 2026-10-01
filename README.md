# Blobrs

A terminal UI for browsing Azure Blob Storage containers and blobs.

## Quick start

You'll need Rust (Cargo + rustc), [Azure CLI](https://learn.microsoft.com/en-us/cli/azure/install-azure-cli),
and an Azure Storage account.

```bash
git clone https://github.com/natemcintosh/blobrs.git
cd blobrs
cargo install --path . --locked

az login
export AZURE_STORAGE_ACCOUNT="your_storage_account_name"
blobrs
```

`AZURE_STORAGE_ACCOUNT` is the only required environment variable. See
[Environment variables](#environment-variables) for PowerShell and optional settings.
Once installed, run `blobrs` from any directory. Make sure Cargo's binary directory
(`~/.cargo/bin` or `%USERPROFILE%\.cargo\bin`) is on your `PATH`.

## Features

- Browse containers and virtual folders
- Pin favorite containers
- Search/filter blobs by name
- Preview files and view blob/folder metadata
- Download files and folders
- Upload local files
- Clone and delete blobs

## Environment variables

| Variable | Required | Purpose |
| --- | --- | --- |
| `AZURE_STORAGE_ACCOUNT` | Yes | Storage account name, not a URL or connection string. |
| `BLOBRS_ICONS` | No | Set to `unicode`, `ascii`, or `minimal` to override automatic icon detection. |

For Bash/zsh:

```bash
export AZURE_STORAGE_ACCOUNT="your_storage_account_name"
```

For PowerShell:

```powershell
$env:AZURE_STORAGE_ACCOUNT = "your_storage_account_name"
```

To keep the setting across terminals, add the line to `~/.bashrc`, `~/.zshrc`, or
your PowerShell profile (`$PROFILE`).

Credentials come from your Azure CLI login; no storage key or container variable
is needed. The installed binary and `cargo run` do not load `.env` files.

## Storage permissions

Blobrs uses your current Azure CLI login. The signed-in identity needs
**Storage Blob Data Reader** to browse, preview, and download, or
**Storage Blob Data Contributor** to also upload, clone, and delete blobs.
Assign the role at the storage account scope (or above), since Blobrs starts by
listing containers. An Azure management role such as Contributor alone does not
grant blob data access.

## Usage notes

- Favorites are saved per storage account in `$XDG_DATA_HOME/blobrs/favorites.json`
  (defaulting to `~/.local/share/blobrs/favorites.json` on Linux). Finish searching
  with Enter before pressing `f` to toggle a favorite.
- Uploads use the local filename and never overwrite existing blobs. Uploads
  read the file into memory and block the TUI until complete.
- Upload and download file choosers require a working desktop portal on Linux.

## Development

Run from the repository with `cargo run --release`, or copy `.env.example` to
`.env`, set your account name, and use `just run` to load it automatically.

Install [cargo-nextest](https://nexte.st/docs/installation/) and run the tests:

```bash
cargo install cargo-nextest --locked
just test
```

## License

MIT (see `LICENSE`).
