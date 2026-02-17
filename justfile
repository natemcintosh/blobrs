set dotenv-required := true

default:
    @just --list

build:
    cargo build --release

run:
    @just check-env
    cargo run --release

test:
    cargo test

check:
    cargo clippy -- -D warnings

fmt:
    cargo fmt

clean:
    cargo clean

test-all: fmt check test

[private]
check-env:
    @if [ -z "${AZURE_STORAGE_ACCOUNT:-}" ]; then \
        echo "AZURE_STORAGE_ACCOUNT environment variable not set"; \
        exit 1; \
    fi
    @if [ -z "${AZURE_STORAGE_ACCESS_KEY:-}" ]; then \
        echo "AZURE_STORAGE_ACCESS_KEY environment variable not set"; \
        exit 1; \
    fi
