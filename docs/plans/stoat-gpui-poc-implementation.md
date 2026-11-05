# Implementation Record: Stoat Chat GPUI Client (POC)

_Plan: docs/plans/stoat-gpui-poc.md_
_Started: 2026-11-04_

## Chunk 1: Project scaffold and window shell

### Changes Made
- `Cargo.toml`: new `stoat-gpui` package (edition 2024). Dependencies:
  - `gpui-kit = "=0.9.0"`, which bundles `gpui-pre 0.3.11` and `gpui-component 0.9.0`
  - `tokio` (rt-multi-thread, macros, time, sync, net)
  - `futures`, `anyhow`, `log`, `env_logger`
- `rust-toolchain.toml`: pins Rust `1.99.1` with clippy and rustfmt for this project only. The user's global default (1.93.1) is untouched.
- `src/lib.rs`: library crate exposing `runtime` and `ui`, so integration tests in `tests/` can import them.
- `src/main.rs`: thin binary that:
  - starts `env_logger` (default filter `stoat_gpui=info`) and the IO runtime
  - runs `gpui_kit::application()` and calls `gpui_kit::init`
  - registers a `stoat::Quit` action bound to `cmd-q`, with a "Stoat" app menu containing "Quit Stoat"
  - opens a centered 1100×720 window titled "Stoat" through `gpui_kit::open_window`, which wraps the view in `gpui_base::Root`
- `src/runtime.rs`: a process-wide Tokio runtime (2 workers, `stoat-io` threads) behind a `OnceLock`. `spawn_io(fut) -> IoTask<T>` runs a future on that runtime and returns a handle that any executor can await (a `futures::channel::oneshot` underneath). The handle resolves to `anyhow::Result<T>`, which is `Err` if the task panicked or was cancelled. **Dropping an `IoTask` aborts the Tokio task**, so abandoned requests (for example after switching channels) are cancelled. Three unit tests.
- `src/ui/mod.rs`, `src/ui/app_root.rs`: an `AppRoot` view with a centered "Stoat" title and the subtitle "GPUI proof of concept · not signed in". It follows macOS light/dark mode through `Theme::sync_system_appearance` and `observe_window_appearance`.
- `.gitignore`: `/target`, `.DS_Store`.

### Divergences
- **Minor: Rust toolchain pin.** `gpui-pre 0.3.11` calls `std::hint::cold_path`, which Rust 1.93.1 treats as unstable (E0658). I added `rust-toolchain.toml` pinning 1.99.1, the current stable, instead of updating the user's global toolchain. Rustup installed 1.99.1 alongside the existing toolchains.
- **Minor: lib + bin split.** I added `src/lib.rs` because clippy `-D warnings` rejected `spawn_io` as dead code in a binary-only crate. Chunk 2's planned `tests/api_login.rs` integration tests need a library target anyway.
- **Minor: `Menu` has a `disabled` field** in this GPUI version (`disabled: false`).
- **Significant, user-directed: reuse the Stoat backend's Rust crates.** Mid-chunk, the user asked to use the existing Stoat backend Rust code wherever possible. I investigated and amended the plan; nothing in chunk 1 itself changed.
  - **Reuse:**
    - `revolt-models =0.16.2` (MIT, `default-features = false, features = ["serde", "partials"]`) for all wire types
    - `revolt-result =0.16.2` (MIT) for error bodies
    - `revolt-permissions` (MIT) only if needed
  - **Don't reuse `revolt-database`.** It's AGPL-3.0 with MongoDB, Redis, RabbitMQ and SMTP dependencies, and it's where `EventV1` lives. The client defines its own small `ServerEvent` enum with `revolt-models` payloads instead.
  - A scratch build (outside the project) confirmed `revolt-models` compiles as a client dependency and parses `ResponseLogin` (MFA and Success), `Channel::TextChannel` and `PartialMessage`. It also serializes `DataLogin::MFA` correctly.
  - Look-ahead updated:
    - the plan's Approach section and Assumptions 6 and 11
    - the files and risks for chunks 2, 4, 5 and 6
    - the Test Plan, which now has contract tests for the pinned models

### Verification Performed
- `cargo build`: success. The first full build of GPUI and its dependencies took several minutes; incremental builds take about 4 s.
- `cargo test`: 3 passed, 0 failed:
  - `delivers_tokio_result_to_a_non_tokio_executor`
  - `panicking_task_resolves_to_error`
  - `dropping_the_handle_aborts_the_task`
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: clean (after `cargo fmt`).
- `./target/debug/stoat-gpui`:
  - logged `INFO stoat_gpui] Stoat window opened`
  - the process stayed running
  - `CGWindowListCopyWindowInfo` reported an on-screen, layer-0 window owned by the process (window id 140792)
  - a pixel screenshot wasn't possible (`screencapture` refused because Screen Recording permission isn't granted), so the visual check is left to the user

### Manual Validation
The window was left open for the user to inspect, and they gave the go-ahead to commit without reporting any issues.

### Notes
- After this chunk, the project moved to its permanent home, `~/Git/Personal/RuSToat`. Build, tests, clippy and fmt were re-verified there before committing.
