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

## Chunk 3: Keychain session persistence and logout

### Changes Made
- `Cargo.toml`: added `keyring = "4.2"` (default `v1` mode, which uses Keychain Services on macOS).
- `src/session.rs` (new):
  - `TokenStore` trait with `load`, `save` and `clear`, keyed by **instance**, meaning the API base URL.
    - `KeychainStore`: service `chat.stoat.gpui-poc`, account = API URL. A missing item counts as "none", and clearing a missing item is fine.
    - `MemoryStore`: public, used by the tests.
  - `restore(store, client) -> Restored` loads the token and checks it with `GET /users/@me`:
    - valid: `SignedIn { token, user: Box<User> }`
    - none saved, or rejected (`Unauthorized`): `NeedsLogin`, clearing the rejected token
    - any other error (network, 5xx, 429): `Unverified { token, error }`, keeping the token
  - `remember(store, instance, token)` saves a token.
  - `log_out(store, client)` **clears the local token first**, then calls the server, and returns both results.
  - Store calls run on `spawn_blocking`.
  - An opt-in real-Keychain round-trip test uses the separate service `chat.stoat.gpui-poc.test`.
- `src/api/client.rs`:
  - `logout()` calls `POST /auth/session/logout`. A 204 or an already-rejected token (401) counts as success.
  - `send` is split into `send_raw`, which returns the status and body bytes, and `send`, which parses JSON. This handles 204 responses with no body.
- `src/ui/app_root.rs`: `AppRoot::new(store, …)` is now a state machine:
  - **Restoring** ("Signing in…") goes to **SignedIn**, **Login**, or **Offline**.
  - **Offline** shows the error with **Retry** and **Log out**.
  - **SignedIn** shows "Logged in as …" with a **Log out** button.
  - A fresh login saves the token in a detached task (a save failure is only logged) and then fetches the profile.
  - `pub fn log_out` runs `session::log_out` in a detached task, so changing the screen doesn't cancel the IO, and shows the login screen right away. A failed server logout is logged with a pointer to Settings → Sessions.
- `src/main.rs`:
  - adds the `stoat::LogOut` action and a "Log Out" item (plus a separator) in the Stoat menu
  - registers the action at the app level, calling `AppRoot::log_out` through the window handle so it works wherever focus is
  - passes `Arc<KeychainStore>` into `AppRoot`
- `src/lib.rs`: registers `session`.
- `tests/session_restore.rs` (new): 10 `wiremock` + `MemoryStore` tests:
  - no token: login, with no API call
  - valid token: signed in, token kept
  - 401: token cleared
  - unreachable server: token kept, `Network` error
  - 500: token kept
  - token saved for another instance: ignored
  - `remember` then `restore` round trip
  - logout: sends the token and clears it
  - logout while offline: still clears locally
  - logout with an already-revoked token: success
- `README.md` (new): how to run, the first-build note, the `STOAT_API_URL` override, the per-instance Keychain item, **the expected Keychain prompt after rebuilds**, Log Out behaviour, the reuse of the backend crates, and the test commands, including both opt-in checks.

### Divergences
- **Minor: `keyring` 4.2 instead of 3.x.** 4.x is the current major version. Its `v1` mode keeps the same `Entry` API (`new`, `get_password`, `set_password`, `delete_credential`, `Error::NoEntry`).
- **Minor: Keychain account is the API URL instead of a fixed `session`.** Tokens are tied to an instance, so pointing `STOAT_API_URL` elsewhere never sends one server's token to another. A test covers this.
- **Minor: no keyboard shortcut for Log Out.** The plan called for one, but ⇧⌘Q is macOS's own "Log Out <user>". Log Out is available from the menu and from a button in the window instead.
- **Minor: `log_out` clears locally before calling the server.** That way a slow or offline server call can't delete the token from a quick re-login.
- **Minor: README written now** rather than at the end, since this chunk's Keychain-prompt note needed a home.
- No effect on later chunks: chunk 4 gets the token from `Screen::SignedIn`/`Restored::SignedIn` as planned.

### Verification Performed
- `cargo test`: 37 passed, 0 failed, 2 ignored:
  - 12 unit tests
  - 9 `api_login`
  - 6 contract
  - 10 `session_restore`
- `cargo test --lib -- --ignored`: `keychain_store_round_trips` passed against the real login Keychain. Afterwards `security find-generic-password -s chat.stoat.gpui-poc.test` found nothing left behind.
- `cargo clippy --all-targets -- -D warnings`: clean, after boxing `User` in `Restored::SignedIn` for `large_enum_variant`. `cargo fmt --check`: clean.
- Launched the app. It logged `using Stoat API at https://api.stoat.chat` and `Stoat window opened`. There was no saved session yet, so restore led to the login screen.

### Manual Validation
All passed on 2026-11-04 with the user's real account:
1. **Login:** the user logged in. The app logged `logged in as user …` and `session saved to Keychain`, and `security find-generic-password -s chat.stoat.gpui-poc` confirmed the item existed.
2. **Relaunch:** the app was quit and relaunched (same binary, so no Keychain prompt). It logged `restored saved session for <username>` straight away, without showing the login screen.
3. **Log out:** the user clicked Log out and confirmed they were back at the login screen and the session was gone from Stoat's Sessions. The app logged `logged out` (server logout succeeded), and the Keychain item was gone.
