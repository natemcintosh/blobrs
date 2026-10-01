set dotenv-load := true

alias b := build
alias r := run
alias t := test-all
alias c := check
alias f := fmt
alias i := install

# List available commands and aliases.
[group('help')]
default:
    @just --list

# Build the application in release mode.
[group('development')]
build:
    cargo build --release

# Reinstall the application from local source using the locked dependencies.
[group('development')]
install:
    cargo install --path . --locked --force

# Run the release build using the configured Azure Storage account.
[group('development')]
run:
    @just check-env
    cargo run --release

# Run tests with cargo-nextest.
[group('validation')]
test:
    cargo nextest run

# Run Clippy and treat warnings as errors.
[group('validation')]
check:
    cargo clippy -- -D warnings

# Format Rust source files.
[group('validation')]
fmt:
    cargo fmt

# Remove Cargo build artifacts.
[group('development')]
clean:
    cargo clean

# Format source files, run Clippy, and run all tests.
[group('validation')]
test-all: fmt check test

# Ensure AZURE_STORAGE_ACCOUNT is set before running the application.
[private]
check-env:
    @if [ -z "${AZURE_STORAGE_ACCOUNT:-}" ]; then \
        echo "AZURE_STORAGE_ACCOUNT environment variable not set"; \
        exit 1; \
    fi
