# justfile for blobrs - Azure Blob Storage TUI Browser
set dotenv-required := true

# Default recipe to list available commands
default:
    @just --list

# Build the project
build:
    @echo "🔨 Building blobrs..."
    cargo build

# Build in release mode
build-release:
    @echo "🔨 Building blobrs (release)..."
    cargo build --release

# Run the application
run:
    @echo "🚀 Running blobrs..."
    @just check-env
    cargo run

# Run in release mode
run-release:
    @echo "🚀 Running blobrs (release)..."
    @just check-env
    cargo run --release

# Check code formatting
fmt-check:
    @echo "📝 Checking code formatting..."
    cargo fmt -- --check

# Format code
fmt:
    @echo "📝 Formatting code..."
    cargo fmt

# Run clippy lints
clippy:
    @echo "🔍 Running clippy..."
    cargo clippy -- -D warnings

# Run all tests
test:
    @echo "🧪 Running tests..."
    cargo test

# Clean build artifacts
clean:
    @echo "🧹 Cleaning build artifacts..."
    cargo clean

# Check if the project compiles
check:
    @echo "✅ Checking compilation..."
    cargo check

# Run full test suite
test-all: fmt-check clippy check build
    @echo "🎉 All checks passed!"

# Check environment variables
check-env:
    @if [ -z "${AZURE_STORAGE_ACCOUNT:-}" ]; then \
        echo "❌ AZURE_STORAGE_ACCOUNT environment variable not set"; \
        echo "   Please set your Azure Storage Account name"; \
        exit 1; \
    fi
    @if [ -z "${AZURE_STORAGE_ACCESS_KEY:-}" ]; then \
        echo "❌ AZURE_STORAGE_ACCESS_KEY environment variable not set"; \
        echo "   Please set your Azure Storage Access Key"; \
        exit 1; \
    fi
    @echo "✅ Environment variables configured"

# Setup development environment
setup:
    @echo "🔧 Setting up development environment..."
    @if [ ! -f ".env" ]; then \
        echo "📋 Creating .env from template..."; \
        cp .env.example .env; \
        echo "✏️  Please edit .env with your Azure credentials"; \
    else \
        echo "✅ .env file already exists"; \
    fi
    @echo "🔨 Installing dependencies..."
    cargo fetch
    @echo "✅ Setup complete!"

# Show environment status
env-status:
    @echo "🌍 Environment Status:"
    @echo -n "AZURE_STORAGE_ACCOUNT: "; if [ -n "${AZURE_STORAGE_ACCOUNT:-}" ]; then echo "✅ Set"; else echo "❌ Not set"; fi
    @echo -n "AZURE_STORAGE_ACCESS_KEY: "; if [ -n "${AZURE_STORAGE_ACCESS_KEY:-}" ]; then echo "✅ Set (hidden)"; else echo "❌ Not set"; fi

# Install development dependencies
install:
    @echo "📦 Installing just (if needed)..."
    @if ! command -v just >/dev/null 2>&1; then \
        echo "Installing just..."; \
        cargo install just; \
    else \
        echo "✅ just already installed"; \
    fi

# Watch for changes and rebuild
watch:
    @echo "👀 Watching for changes..."
    cargo watch -x check -x test -x run

# Generate documentation
docs:
    @echo "📚 Generating documentation..."
    cargo doc --open

# Update dependencies
update:
    @echo "📦 Updating dependencies..."
    cargo update

# Show project info
info:
    @echo "📋 Project Information:"
    @echo "Name: blobrs"
    @echo "Description: Azure Blob Storage TUI Browser"
    @echo "Language: Rust"
    @echo "Framework: Ratatui"
    @echo ""
    @echo "🔑 Required Environment Variables:"
    @echo "  - AZURE_STORAGE_ACCOUNT (your storage account name)"
    @echo "  - AZURE_STORAGE_ACCESS_KEY (your storage account access key)"
    @echo ""
    @echo "🔍 How to find your Azure Storage Access Key:"
    @echo "  1. Go to Azure Portal (portal.azure.com)"
    @echo "  2. Navigate to your Storage Account"
    @echo "  3. In left sidebar: Security + networking → Access keys"
    @echo "  4. Click 'Show' next to key1 or key2"
    @echo "  5. Copy the 'Key' value (not connection string)"
    @echo ""
    @echo "🎭 Icon Configuration (Optional):"
    @echo "  - BLOBRS_ICONS=unicode (📁 📄 🔄 emojis - default for modern terminals)"
    @echo "  - BLOBRS_ICONS=ascii ([DIR] [FILE] [LOADING] - basic terminals)"
    @echo "  - BLOBRS_ICONS=minimal (D F * - legacy terminals)"
    @echo ""
    @echo "🎮 Navigation:"
    @echo "  - ↑/↓ or k/j: Navigate up/down"
    @echo "  - →/l/Enter: Enter folder or select container"
    @echo "  - ←/h/Esc: Go up one level or back"
    @echo "  - r/F5: Refresh"
    @echo "  - /: Search/filter blobs"
    @echo "  - i: Show blob/folder information"
    @echo "  - d: Download selected file or folder"
    @echo "  - q/Ctrl+C: Quit"
    @echo ""
    @echo "🔍 Search Mode:"
    @echo "  - Type to filter results"
    @echo "  - Enter: Confirm search and exit search mode"
    @echo "  - Esc: Cancel search and restore full list"
    @echo "  - Ctrl+↑/↓: Navigate while searching"

# Create a new release
release VERSION:
    @echo "🏷️  Creating release {{VERSION}}..."
    @echo "� Checking git status..."
    @if [ -n "$(git status --porcelain --ignore-submodules)" ]; then \
        echo "❌ Working directory is not clean. Please commit or stash changes first."; \
        exit 1; \
    fi
    @echo "✅ Working directory is clean"
    @echo "�📝 Updating version in Cargo.toml..."
    sed -i 's/^version = ".*"/version = "{{VERSION}}"/' Cargo.toml
    @echo "✅ Updated Cargo.toml to version {{VERSION}}"
    @just test-all
    @echo "📦 Committing version update..."
    git add Cargo.toml Cargo.lock
    git commit -m "Bump version to {{VERSION}}"
    @echo "🏷️  Creating git tag..."
    git tag -a "v{{VERSION}}" -m "Release v{{VERSION}}"
    @echo "✅ Tagged release v{{VERSION}}"
    @echo "🚀 Next steps:"
    @echo "   git push origin main"
    @echo "   git push origin v{{VERSION}}"

# Test release process without making changes
release-dry-run VERSION:
    @echo "🧪 Dry run: Creating release {{VERSION}}..."
    @echo " Would update version in Cargo.toml to {{VERSION}}"
    @echo "Preview of Cargo.toml change:"
    @sed 's/^version = ".*"/version = "{{VERSION}}"/' Cargo.toml | head -10
    @echo ""
    @echo "🧪 Would run: just test-all"
    @echo "📦 Would commit version update"
    @echo "🏷️  Would create git tag v{{VERSION}}"
    @echo "✅ Dry run complete - no changes made"

# Test icon detection in current terminal
test-icons:
    @echo "🎭 Testing icon detection in your terminal..."
    @echo "TERM: ${TERM:-not set}"
    @echo "TERM_PROGRAM: ${TERM_PROGRAM:-not set}"
    @echo "LANG: ${LANG:-not set}"
    @echo ""
    @echo "🌟 Unicode/Emoji test: 📁 📄 🔄 ❌ ✅ 📭 🔍"
    @echo "🔤 ASCII test: [DIR] [FILE] [LOADING] [ERROR] [OK] [EMPTY] [SEARCH]"
    @echo "⚡ Minimal test: D F * ! + - ?"
    @echo ""
    @echo "💡 To force a specific icon set, set BLOBRS_ICONS before running:"
    @echo "   BLOBRS_ICONS=unicode just run"
    @echo "   BLOBRS_ICONS=ascii just run"
    @echo "   BLOBRS_ICONS=minimal just run"
