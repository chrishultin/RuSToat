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

## Chunk 2: Stoat REST client and login screen (with 2FA)

### Changes Made
- `Cargo.toml`: added `revolt-models =0.16.2` (`default-features = false, features = ["serde", "partials"]`), `revolt-result =0.16.2` (`default-features = false, features = ["serde"]`), `reqwest 0.13` (`json`, default rustls TLS), `serde`, `serde_json` and `thiserror`. Dev dependency: `wiremock 0.6`.
- `src/api/mod.rs`: re-exports `revolt_models::v0` as `api::models`, so all wire types are the backend's own.
- `src/api/client.rs`:
  - `StoatClient` is cloneable and carries the base URL plus an optional token. `from_env()` honours `STOAT_API_URL`, `with_token()` returns a copy with a token, and a trailing `/` in the base URL is trimmed.
  - Methods:
    - `root_config()`: `GET /`
    - `login(email, password)`: `POST /auth/session/login` with `DataLogin::Email`
    - `login_mfa(ticket, MFAResponse)`: the same endpoint with `DataLogin::MFA`
    - `fetch_self()`: `GET /users/@me` with `x-session-token`
  - Every login sends `friendly_name: "Stoat GPUI POC"`.
  - `RootConfig` is a local two-field struct (`version` read from the `revolt` key, and `ws`), because `revolt-models` has no type for `GET /`.
- `src/api/error.rs`: `ApiError` has these variants:
  - `Api(revolt_result::Error)`
  - `Unauthorized`, for `InvalidSession`/`NotAuthenticated` and for any 401 whose body can't be parsed
  - `RateLimited { retry_after_ms }`, for HTTP 429
  - `Network`
  - `Unexpected { status, detail }`

  The login-relevant `ErrorType`s map to readable text (`InvalidCredentials`, `InvalidToken`, `UnverifiedAccount`, `LockedOut`, `DisallowedMFAMethod`, `CaptchaFailed`). Others fall back to "Stoat returned an error: <Variant>" through `IntoStaticStr`.
- `src/ui/login.rs`: `LoginView`, built from `gpui-component` `InputState`/`Input` and `Button`.
  - **Credentials step:** an email field, and a masked password field with a show/hide toggle. Enter in email moves focus to password. Enter in password, or clicking "Log in", submits.
  - **MFA step:** shown when the server returns `ResponseLogin::MFA`. It has a code field and, if both methods are allowed, a "Use a recovery code instead" toggle. Spaces are stripped from TOTP codes. "Back" returns to the credentials step.
  - **While a request is in flight:** inputs are disabled and the button shows a spinner.
  - **Error handling:** errors appear inline in red. `Disabled` accounts and MFA with only unsupported methods get specific messages.
  - Requests run through `spawn_io`, and results come back via `cx.spawn_in`. A successful login emits `LoginEvent::LoggedIn(Session)`.
- `src/ui/app_root.rs`: `AppRoot` routes between `Screen::Login` and `Screen::SignedIn`.
  - After login it calls `fetch_self()` with the new token and shows "Logged in as Display Name (username#1234)" plus the session name. `SignedIn` is boxed to satisfy clippy's `large_enum_variant`.
  - The session token is kept in memory only (Keychain comes in chunk 3).
- `src/ui/mod.rs`: registers `login`.
- `tests/fixtures/*.json`: API-shaped `root`, `login_success`, `login_mfa`, `login_disabled`, `error_invalid_credentials` and `users_me`.
- `tests/contract_models.rs`: 6 contract tests that parse the fixtures into `revolt-models`/`revolt-result` types.
- `tests/api_login.rs`: 9 `wiremock` tests covering:
  - login success, checking the `friendly_name` that gets sent
  - MFA followed by TOTP
  - a recovery code
  - `InvalidCredentials` producing the readable message
  - `fetch_self` sending the token
  - a revoked token producing `Unauthorized`
  - `root_config`
  - connection refused producing `Network`
  - a trailing slash in the base URL

  Plus 1 `#[ignore]` live test that hits the real `GET /` and asserts the server is still on 0.16.x (a drift check for the pinned crates).

### Divergences
- **Minor: login errors come from `authifier`, not `revolt-result`.** Stoat's auth routes use `authifier` (Apache-2.0), whose `Error` can be serialized but not deserialized. `revolt_result::ErrorType` contains the same auth variants, and the live OpenAPI's single `Error` schema lists them too, so login errors parse into `revolt_result::Error` and no extra crate is needed.
- **Minor: added `ApiError::RateLimited`** for HTTP 429 (`{"retry_after": ms}`), which is likely to come up during manual testing.
- **Minor: added an `#[ignore]` live drift test.**
- **Gotcha for later chunks:** in any module that does `use gpui_kit::*`, a test submodule must not `use super::*`. The glob brings in GPUI's `test` proc macro, which shadows Rust's `#[test]` and fails with "recursion limit reached while expanding `#[test]`". Import the items under test explicitly. Recorded in the plan's Test Plan.

### Verification Performed
- `cargo build`: success.
- `cargo test`: 26 passed, 0 failed, 1 ignored:
  - 11 unit tests (6 error-mapping, 3 runtime, 2 user label)
  - 9 `wiremock` tests
  - 6 contract tests
- `cargo test --test api_login -- --ignored`: the live drift test passed against `https://api.stoat.chat`.
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Launched `./target/debug/stoat-gpui` with `RUST_LOG=stoat_gpui=debug`. It logged `using Stoat API at https://api.stoat.chat` and `Stoat window opened`, with an on-screen window owned by the process and no panics.

### Manual Validation
The user logged in with their real Stoat account in the running app and confirmed that it works. They didn't say separately which paths they tried (2FA, wrong password, recovery code). Those paths are covered by the `wiremock` tests.
