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
download blobs, or **Storage Blob Data Contributor** to also clone and delete
blobs. Assign the role at the storage account scope (or above), because Blobrs
starts by listing the account's containers. An Azure management role such as
Contributor alone does not grant blob data access.

If authentication fails, check that `az` is on your PATH, sign in again, and
press `r` to refresh. If Azure returns a permission error, check the identity's
storage data role and scope.

## Environment Variables

Set:

```bash
export AZURE_STORAGE_ACCOUNT="your_storage_account_name"
```

When using `just run`, you can instead use a `.env` file (optional). Direct
`cargo run` requires the variable to be exported in your shell:

```env
AZURE_STORAGE_ACCOUNT=your_storage_account_name
```

## Install

```bash
git clone https://github.com/natemcintosh/blobrs.git
cd blobrs
cargo build --release
```

## Run

Using Cargo:

```bash
cargo run --release
```

Using just:

```bash
just run
```

## License

MIT (see `LICENSE`).
