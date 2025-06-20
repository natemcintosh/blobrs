# Bl## Features

- 📦 **Container Selection** - Select from a list of available containers in your storage account
- 🔍 Browse Azure Blob Storage containers and blobs
- 📁 Navigate through blob prefixes (virtual directories)
- ℹ️ **Blob Information** - View detailed metadata about blobs and folder statistics
- ⬇️ **Download Files and Folders** - Download individual files or entire folders with progress tracking
- ⚡ Async operations with loading indicators
- 🎨 Clean, intuitive terminal interface
- ⌨️ Vim-style navigation keys
- 🔍 Search/filter blobs by name (press `/`)
- 🎭 Adaptive icons based on terminal capabilities
- 🌍 Cross-platform support (Windows, macOS, Linux) Blob Storage TUI Browser

A terminal user interface (TUI) application for browsing Azure Blob Storage containers built with Rust and Ratatui.

## Features

- �️ **Container Selection** - Select from a list of available containers in your storage account
- �🔍 Browse Azure Blob Storage containers and blobs
- 📁 Navigate through blob prefixes (virtual directories)
- ℹ️ **Blob Information** - View detailed metadata about blobs and folder statistics
- ⚡ Async operations with loading indicators
- 🎨 Clean, intuitive terminal interface
- ⌨️ Vim-style navigation keys
- 🔍 Search/filter blobs by name (press `/`)
- 🎭 Adaptive icons based on terminal capabilities
- 🌍 Cross-platform support (Windows, macOS, Linux)

## Prerequisites

- Rust 1.70+ (with Cargo)
- Azure Storage Account with Blob Storage enabled
- Storage Account Access Key

## Setup

### 1. Azure Storage Account

You'll need an Azure Storage Account with one or more containers. If you don't have one:

1. Create an Azure Storage Account in the [Azure Portal](https://portal.azure.com)
2. Create one or more containers in your storage account
3. Get your storage account access key from the "Access keys" section

### 2. Environment Variables

Set the following environment variables:

```bash
export AZURE_STORAGE_ACCOUNT="your_storage_account_name"
export AZURE_STORAGE_ACCESS_KEY="your_access_key"
```

Or create a `.env` file in the project root:

```env
AZURE_STORAGE_ACCOUNT=your_storage_account_name
AZURE_STORAGE_ACCESS_KEY=your_access_key
```

**Note:** You no longer need to specify `AZURE_CONTAINER_NAME` as the application will present you with a list of containers to select from.

## Installation

### From Source

```bash
git clone https://github.com/your-username/blobrs.git
cd blobrs
cargo build --release
```

### Using Just (Recommended)

This project includes a [`justfile`](https://github.com/casey/just) for common development tasks:

```bash
# Install just if you don't have it
cargo install just

# See all available commands
just

# Setup development environment
just setup

# Build the project
just build

# Run all checks (format, clippy, build)
just test-all
```

## Usage

### Using Cargo

```bash
# Set environment variables first
export AZURE_STORAGE_ACCOUNT="mystorageaccount"
export AZURE_CONTAINER_NAME="mycontainer"
export AZURE_STORAGE_ACCESS_KEY="your_access_key_here"

# Run the application
cargo run
```

### Using Just

```bash
# Check environment status
just env-status

# Run the application (automatically checks environment)
just run

# Run in release mode
just run-release
```

## Navigation

### Container Selection Mode

When you first start the application, you'll be in container selection mode:

| Key | Action |
|-----|--------|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `→` / `l` / `Enter` | Select container and enter blob browsing mode |
| `r` / `F5` | Refresh container list |
| `q` / `Esc` / `Ctrl+C` | Quit application |

### Blob Browsing Mode

After selecting a container, you can browse blobs:

| Key | Action |
|-----|--------|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `→` / `l` / `Enter` | Enter selected folder |
| `←` / `h` / `Esc` | Go up one level (or to container list if at root) |
| `/` | Search/filter blobs |
| `i` | Show blob/folder information |
| `d` | Download selected file or folder |
| `r` / `F5` | Refresh current view |
| `Backspace` | Return to container selection |
| `q` / `Ctrl+C` | Quit application |

### Search Mode

| Key | Action |
|-----|--------|
| `Type` | Filter results in real-time |
| `Enter` | Confirm search (keep filtered results) |
| `Esc` | Cancel search (restore full list) |
| `Ctrl+↑` / `Ctrl+↓` | Navigate while searching |

### Navigation Hierarchy

The application uses a hierarchical navigation system with the `Esc` key:

- **Container Selection**: `Esc` quits the application
- **Blob Browsing (at container root)**: `Esc` returns to container selection  
- **Blob Browsing (inside folders)**: `Esc` goes up one directory level
- **Search Mode**: `Esc` exits search and returns to normal browsing
- **Information Popup**: `Esc` closes the popup

This provides intuitive "back" behavior - `Esc` always takes you one level up in the navigation hierarchy.

### Blob Information Mode

When viewing blob or folder information (press `i` in blob browsing mode):

| Key | Action |
|-----|--------|
| `Esc` / `←` / `h` | Close information popup |

The information is displayed in a popup window that overlays the blob list, showing:
- **For individual blobs**: Name, size, last modified date, and ETag
- **For folders**: Name, number of contained blobs, and total storage size

### Download Mode

When downloading files or folders (press `d` in blob browsing mode):

| Key | Action |
|-----|--------|
| `Enter` | Select download destination folder |
| `Esc` | Cancel download |

The download process works as follows:
1. Press `d` to start downloading the selected file or folder
2. A file picker will open allowing you to choose the destination folder
3. For single files: The file will be downloaded to the selected destination
4. For folders: All files in the folder will be downloaded, preserving the folder structure
5. A progress popup shows download status including:
   - Current file being downloaded
   - Number of files completed vs total files
   - Total bytes downloaded
   - Any error messages

## Terminal Compatibility

Blobrs automatically detects your terminal's capabilities and adapts its icons accordingly:

### Unicode/Emoji Icons (Modern Terminals)
- **Folders**: 📁 
- **Files**: 📄
- **Loading**: 🔄
- **Errors**: ❌
- **Empty**: 📭
- **Search**: 🔍

**Supported terminals**: Kitty, Alacritty, WezTerm, iTerm2, VS Code, Windows Terminal, and most modern terminals with UTF-8 support.

### ASCII Icons (Basic Terminals)
- **Folders**: [DIR]
- **Files**: [FILE]
- **Loading**: [LOADING]
- **Errors**: [ERROR]
- **Empty**: [EMPTY]
- **Search**: [SEARCH]

### Minimal Icons (Legacy Terminals)
- **Folders**: D
- **Files**: F
- **Loading**: *
- **Errors**: !
- **Empty**: -
- **Search**: ?

### Manual Override

You can force a specific icon set using the `BLOBRS_ICONS` environment variable:

```bash
export BLOBRS_ICONS=unicode  # Force Unicode/emoji icons
export BLOBRS_ICONS=ascii    # Force ASCII icons
export BLOBRS_ICONS=minimal  # Force minimal icons
```

## How It Works

- **Blobs**: Individual files in your container appear with a file icon (📄, [FILE], or F depending on terminal)
- **Prefixes**: Virtual directories (blob name prefixes ending with `/`) appear with a folder icon (📁, [DIR], or D depending on terminal)
- **Navigation**: Uses Azure Blob Storage's hierarchical namespace simulation through prefixes
- **Async Operations**: All Azure API calls are asynchronous with loading indicators
- **Terminal Detection**: Automatically detects terminal capabilities for optimal icon display
- **Search**: Real-time filtering of blobs and directories by name

## Troubleshooting

### "Failed to list blobs" Error

- Verify your Azure credentials are correct
- Check that the container exists and you have read permissions
- Ensure your storage account allows blob access

### Connection Issues

- Check your internet connection
- Verify the storage account name is correct
- Try refreshing with `r` or `F5`

### Environment Variables Not Set

The application will panic if required environment variables are missing:
- `AZURE_STORAGE_ACCOUNT`
- `AZURE_CONTAINER_NAME`  
- `AZURE_STORAGE_ACCESS_KEY`

## Development

### Using Just (Recommended)

```bash
# Setup development environment
just setup

# Run all checks
just test-all

# Format code
just fmt

# Run clippy
just clippy

# Build project
just build

# Watch for changes and rebuild
just watch
```

### Using Cargo

```bash
# Build
cargo build

# Run tests
cargo test

# Format code
cargo fmt

# Run clippy
cargo clippy
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Run `cargo clippy` and fix any warnings
6. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Dependencies

- [Ratatui](https://github.com/ratatui/ratatui) - Terminal UI framework
- [object_store](https://crates.io/crates/object_store) - Cloud storage abstraction
- [tokio](https://tokio.rs/) - Async runtime
- [color-eyre](https://crates.io/crates/color-eyre) - Error handling

## Roadmap

- [ ] Support for other cloud providers (AWS S3, Google Cloud Storage)
- [ ] File upload/download functionality  
- [ ] Blob metadata display
- [ ] Search functionality
- [ ] Configuration file support
- [ ] Multiple container support