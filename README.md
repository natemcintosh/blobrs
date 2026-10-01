# Blobrs

A terminal UI for browsing Azure Blob Storage containers and blobs.

## Screenshots

Screenshots will be added here.

## Features

- Browse containers and blobs from your Azure Storage account
- Navigate blob prefixes (virtual folders)
- Search/filter blobs by name
- View blob/folder metadata
- Download files and folders
- Upload local files to the current container or folder with `u`

## Uploading

Open a container, navigate to the destination folder, and press `u`, then Enter
to choose a local file. The file is uploaded using its original filename and the
listing refreshes on success. Upload also works in empty containers. Existing
blobs are never overwritten; rename the local file to upload it under a new name.
Press Esc or cancel the file chooser to cancel. The desktop file chooser requires
a working desktop portal on Linux, as does downloading. Uploads currently read
the selected file into memory, and the TUI waits for the transfer to finish.

## Prerequisites

- Rust (Cargo + rustc)
- [Azure CLI](https://learn.microsoft.com/en-us/cli/azure/install-azure-cli)
- Azure Storage account

## Authentication

Blobrs uses the current Azure CLI login for both container listing and blob
operations. Storage access keys are not required or used. Tokens are cached and
refreshed automatically while the application is running.

On an Azure resource with a managed identity:

```bash
az login --identity
```

For a user-assigned managed identity, select it explicitly:

```bash
az login --identity --client-id <managed-identity-client-id>
```

For an interactive user login:

```bash
az login
```

The signed-in identity needs **Storage Blob Data Reader** to browse, preview, and
download blobs, or **Storage Blob Data Contributor** to also upload, clone, and delete
blobs. Assign the role at the storage account scope (or above), because Blobrs
starts by listing the account's containers. An Azure management role such as
Contributor alone does not grant blob data access.

If authentication fails, check that `az` is on your PATH, sign in again, and
press `r` to refresh. If Azure returns a permission error, check the identity's
storage data role and scope.

## Environment Variables

`AZURE_STORAGE_ACCOUNT` is the only required environment variable. Set it to the
storage account **name**, not a URL or connection string. Credentials come from
your Azure CLI login; no storage key or container variable is needed.

For Bash or zsh, set it in your current terminal:

```bash
export AZURE_STORAGE_ACCOUNT="your_storage_account_name"
```

To keep the setting across terminals, add that `export` line to `~/.bashrc`
(Bash) or `~/.zshrc` (zsh), then open a new terminal or run `source ~/.bashrc`
or `source ~/.zshrc`, respectively.

For PowerShell:

```powershell
$env:AZURE_STORAGE_ACCOUNT = "your_storage_account_name"
```

Add that line to your PowerShell profile (`$PROFILE`) to load it in future
sessions. If the profile does not exist, create it first:

```powershell
New-Item -ItemType File -Path $PROFILE -Force
```

Optional: set `BLOBRS_ICONS` to `unicode`, `ascii`, or `minimal` to override
automatic terminal icon detection, for example `export BLOBRS_ICONS=ascii` in
Bash/zsh or `$env:BLOBRS_ICONS = "ascii"` in PowerShell.

The `blobrs` binary and `cargo run` do not load `.env` files. For development,
`just run` loads the repository's `.env`; copy `.env.example` to `.env` and edit it:

```env
AZURE_STORAGE_ACCOUNT=your_storage_account_name
```

## Install

Install from GitHub with Cargo (no repository checkout required):

```bash
cargo install --git https://github.com/natemcintosh/blobrs.git --locked
```

Make sure Cargo's binary directory is on your `PATH`: by default `~/.cargo/bin`
on Linux/macOS or `%USERPROFILE%\.cargo\bin` on Windows. If you use a custom
`CARGO_HOME` or install root, use its `bin` directory instead.

For Bash/zsh, add `export PATH="$HOME/.cargo/bin:$PATH"` to the same shell
configuration file used above. On Windows, add `%USERPROFILE%\.cargo\bin` to
your user `Path` environment variable through **Edit environment variables for
your account**. Open a new terminal after updating `PATH`.

Alternatively, install from a local checkout:

```bash
git clone https://github.com/natemcintosh/blobrs.git
cd blobrs
cargo install --path . --locked
```

## Run

After installation, setting `AZURE_STORAGE_ACCOUNT`, and signing in with `az
login`, run from **any directory**:

```bash
blobrs
```

You do not need the repository, `just`, or a `.env` file to run the installed
binary. To switch accounts, set `AZURE_STORAGE_ACCOUNT` to the other account name
before launching again.

For development, run from the repository using Cargo:

```bash
cargo run --release
```

Using just:

```bash
just run
```

## Testing

Install [cargo-nextest](https://nexte.st/docs/installation/) and run the tests:

```bash
cargo install cargo-nextest --locked
just test
```

CI also uses nextest on Linux, macOS, and Windows.

## License

MIT (see `LICENSE`).
