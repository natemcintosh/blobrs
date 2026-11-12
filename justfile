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

# Run the release build and choose a saved Azure Storage account.
[group('development')]
run:
    cargo run --release

# Run tests with cargo-nextest.
[group('validation')]
test:
    cargo nextest run

# Run Clippy and treat warnings as errors.
[group('validation')]
check:
    cargo clippy -- -D warnings

# Run pedantic Clippy on all targets and features, treating warnings as errors.
[group('validation')]
pedantic:
    cargo clippy --all-targets --all-features --locked -- -D warnings -W clippy::pedantic

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
