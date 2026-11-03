# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Desktop explorer (Rust, egui/eframe 0.36 + fastframe) for the Dragon City `game_config`. Fan-made. Single binary crate; all domain logic lives in the published `dc-*` crates (`dc-config`, `dc-config-mapper`, `dc-localization`, `dc-statistics`, `dc-core`) — this repo is the UI on top of them.

## Commands

Requires **Rust 1.98+** (fastframe's MSRV; edition 2024). fastframe crates come from git (`crmne/fastframe`, tag `v0.15.5`).

```sh
cargo run --release                          # launch
cargo run --release -- path/to/config.json   # launch and load a file
cargo test                                   # unit tests (fixture/network tests no-op without env vars)
cargo test maps_every_section_of_a_real_config -- --nocapture   # single test

# Real-config tests: runs every mapper and dc-statistics over the file
DC_CONFIG_FIXTURE=/path/config.json cargo test --release -- --nocapture
# Network tests: downloads localization, exercises export/compare
DC_NETWORK_TESTS=1 cargo test --release -- --nocapture
```

Tests gated by those env vars return early (pass silently) when unset — set them to actually exercise the code.

## Architecture

**UI thread never blocks.** `worker.rs` owns a tokio multi-thread runtime. `ExplorerApp` (`app.rs`) sends `Job`s via `Worker::spawn`; results come back as `Event`s on an `mpsc` channel, drained each frame in `ExplorerApp::poll_events`, and the worker calls `ctx.request_repaint()` after each send. CPU-heavy work (parsing, mapping, statistics, file writes) goes through `spawn_blocking`. To add a background operation: add a `Job` variant, handle it in `worker::run`, add an `Event` variant, handle it in `poll_events`.

- **Loading**: three sources (local file / public raw URL / authenticated endpoint) → `dc_config::Config`. `read_game_config` accepts either `{ "game_data": { "config": … } }` or a bare sections object (wrapped automatically). `with_statistics` runs `dc_statistics::generate_statistics` by `mem::take`-ing the sections out and back in — avoid cloning the config, it is large.
- **Document model** (`app.rs`): the loaded config becomes a `Document` of `Section`s, each value in an `Arc<Value>` so jobs can share it cheaply. Explorer state is cached per `DocKey` (`Raw(section)` or `Mapped(section, language)`); mapped results per `(section, language)` as `MapState`.
- **Mapping** (`mapping.rs`): `MAPPED_SECTIONS` + `map_section` dispatch to the `dc-config-mapper` mappers. Adding a mapper means updating both the list and the match.
- **Localization**: downloaded once per `ConfigLanguage` and cached in the worker (`Arc<Mutex<HashMap>>`); the app keeps its own copy for UI lookups. Exports are written key-sorted (BTreeMap) so files diff cleanly; `read_localization_file` accepts the game's array-of-objects format or a flat object.
- **Views**: `explorer.rs` is a reusable virtualized JSON explorer (list or table, filter, breadcrumb, inspector with `tid_*`/`*_key` translations); it reports requests to its owner via an `Action` enum rather than doing I/O itself. `stats_view.rs` and `localization_view.rs` follow the same pattern.
- **Settings** (`settings.rs`): stored in the fastframe-dirs config folder (`DIRS`), written atomically by a `fastframe_file::Writer` background thread. `main` calls `fastframe_file::mark_ui_thread()` — do not do plain synchronous file writes from the UI thread; use the writer or a worker job (`fastframe_file::write` in `spawn_blocking`). The auth token is stored separately in `credentials.json` with private access, and only when `remember_token` is set.

## Conventions

- User-facing strings (labels, error messages) are in **Portuguese (pt-BR)**; code, doc comments and identifiers are in English.
- Errors crossing the worker boundary are `Result<_, String>`.
