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
- Azure Storage account
- Storage account access key

To get your storage account access key:
1. Navigate to your Storage Account in the Azure Portal
1. In the left sidebar, under "Security + networking", click **"Access keys"**
1. You'll see two keys (key1 and key2) - you can use either one
1. Click **"Show"** next to the key you want to use
1. Copy the **"Key"** value (not the connection string)

## Environment Variables

Set:

```bash
export AZURE_STORAGE_ACCOUNT="your_storage_account_name"
export AZURE_STORAGE_ACCESS_KEY="your_access_key"
```

Or use a `.env` file:

```env
AZURE_STORAGE_ACCOUNT=your_storage_account_name
AZURE_STORAGE_ACCESS_KEY=your_access_key
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
