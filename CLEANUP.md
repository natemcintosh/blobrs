# Cleanup Backlog

This document records maintainability opportunities identified during a repository audit. Items
are ordered by expected impact. Estimates are approximate; the optional cuts together could remove
about 1,500 lines and seven direct dependencies.

## 1. Remove unobservable async progress state (complete)

`AsyncOp` loading and progress states cannot be rendered while work is running because each
operation is awaited inside the input handler. Remove the progress structs, state updates,
predicates, popups, and tests, and continue reporting the final result through the existing success
and error messages. This should preserve current observable behavior while removing substantial
state-management code.

Affected areas: `src/app.rs` and `src/ui.rs`.

## 2. Keep only Parquet metadata previews (optional)

Remove Parquet table previews while retaining the smaller metadata/schema preview. Delete Arrow row
decoding, the table/metadata toggle, cached preview variants, and their tests, then disable unneeded
Parquet features and remove the direct Arrow dependency. This reduces code and build cost but users
would no longer be able to inspect Parquet rows in the TUI.

Affected areas: `src/app.rs`, `src/preview.rs`, `src/ui.rs`, and `Cargo.toml`.

## 3. Replace the event thread with direct terminal reads

Remove the 30 FPS tick thread and event channel, then read Crossterm events directly in the main
loop. Tick handling is empty and all current state changes are driven by terminal input, so periodic
events are unnecessary. This also prevents tests from spawning detached terminal-reader threads.

Affected areas: `src/event.rs`, `src/app.rs`, and `src/main.rs`.

## 4. Simplify terminal icons (optional)

Replace automatic terminal-capability detection with fixed ASCII icons. If manual selection remains
useful, keep only the `BLOBRS_ICONS` override and a small set of constants. Remove the duplicate icon
example and detection tests. The tradeoff is losing automatic Unicode icon selection.

Affected areas: `src/terminal_icons.rs`, `examples/simple_icon_test.rs`, and icon call sites.

## 5. Use one source of truth for file and search data

Remove `BrowsingState.files`, `FileItem.display_name`, `Search::Files::all_files`, and the extra full
container list stored inside search state. Keep structured items as the canonical data and format
their labels only while rendering. This eliminates synchronization code and repeated cloning without
changing behavior.

Affected areas: `src/app.rs` and `src/ui.rs`.

## 6. Download to a fixed directory (optional)

Download to the process working directory and remove the GUI folder picker, its modal state,
`spawn_blocking` call, and the `rfd` dependency. Downloads remain available, but users would no
longer choose a destination for each operation.

Affected areas: `src/app.rs`, `src/ui.rs`, and `Cargo.toml`.

## 7. Dispatch input by application state once

Restructure key handling to dispatch first on modal, search, and session state. Normal browsing keys
should only run after those states have been handled. This replaces repeated combinations of modal
and operation predicates with a smaller, explicit state flow.

Affected area: `src/app.rs`.

## 8. Use Ratatui popup primitives

Replace manual popup-clearing loops with Ratatui's `Clear` widget and replace hand-written centering
math with `Rect::centered`. Apply the same approach to every popup so their layout code stays
consistent.

Affected area: `src/ui.rs`.

## 9. Share JSON and text preview rendering

Extract the common paragraph rendering used by JSON and text previews. The shared code should handle
scrolling, truncation indicators, borders, and wrapping, while callers provide only the title and
content-specific metadata.

Affected area: `src/ui.rs`.

## 10. Trim dependencies and features

Remove unused direct dependencies such as `crossterm`, `dirs`, `serde`, and `url`. Build request URLs
with `reqwest::Url` instead of `urlencoding`, align the direct Reqwest version with `object_store`,
and disable unused default features on `arboard` and `object_store`. Narrow Tokio and other feature
sets to the capabilities used by the application.

Affected areas: `Cargo.toml`, `Cargo.lock`, and URL construction in `src/app.rs`.

## 11. Remove the creation-time sort

Delete `SortCriteria::DateCreated`, `FileItem.created`, its key binding, and the corresponding sort
menu entry. Creation time is never populated, and the option currently performs the same operation
as modification-time sorting, so this removes duplicate behavior rather than a working capability.

Affected areas: `src/app.rs` and `src/ui.rs`.

## 12. Remove dead filters and infallible results

Delete the unused `filter_files` and `filter_containers` methods. Change sorting and search handlers
that cannot fail to return `()` instead of `Result<()>`, then simplify their callers and remove the
documentation claiming API consistency as a reason for the unused error channel.

Affected area: `src/app.rs`.

## 13. Make dotenv optional for development commands

Replace the global `dotenv-required` setting with optional dotenv loading. Keep `check-env` on the
run recipe so Azure credentials are still required when launching the application, while formatting,
building, linting, and testing remain usable without a `.env` file.

Affected area: `justfile`.
