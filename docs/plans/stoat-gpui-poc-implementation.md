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

## Chunk 4: Events websocket and sidebar (servers, channels, DMs)

### Changes Made
- `Cargo.toml`: added `tokio-tungstenite 0.30` (`rustls-tls-native-roots`).
- `src/api/events.rs` (new):
  - **Decoding:** `parse_frame` turns a websocket text frame into our own `ServerEvent` enum: `Authenticated`, `Ready`, `Pong`, `Error(revolt_result::Error)`, `Logout`, or `Unknown(type)`. `Bulk` frames are flattened.
  - **Lenient `Ready`:** `users`, `servers`, `channels` and `members` are parsed item by item into `revolt-models` types. Failures are logged and counted in `Ready::skipped`. `server_order` comes from the `ordering` user setting, which holds `{"servers": [...]}` inside the `[timestamp, json]` tuple.
  - **Outgoing frames:** `authenticate_frame` (the token goes in a message, not the URL) and `ping_frame`.
  - **URL:** `events_url` is `…/?version=1&format=json`, with repeated `ready=` for `users`, `servers`, `channels`, `members` and `user_settings[ordering]`.
  - **Connection:** `connect(client)` spawns the connection loop on the IO runtime and returns an abort-on-drop `Connection` plus an `mpsc` receiver of `ConnectionEvent`s (`Connecting`, `Connected`, `Event`, `Disconnected { reason, retry_in }`, `SessionRejected`). Each attempt re-fetches `GET /` for the ws URL, authenticates, pings every 20 s, and drops the connection if no pong arrives for 65 s. It reconnects with backoff from 1 s to 30 s, and the backoff resets after a successful authentication. `InvalidSession`/`NotAuthenticated` errors and `Logout` stop the loop with `SessionRejected`.
- `src/store.rs` (new): `ChatStore` is plain data with no GPUI dependency.
  - `apply_ready` replaces all state on each `Ready`.
  - `servers()` lists servers in the saved order, then any others in `Ready` order. Each `ServerEntry` has the initials and icon `File`.
  - `server_channels()` returns uncategorized channels first, then categories in order. Channels missing from `Ready` and empty categories are skipped.
  - `home_channels()` returns Saved Notes, then open DMs and groups sorted by `last_message_id` descending (ULIDs sort by time). Closed DMs are hidden. DMs are named after the other participant's display name, or "Unknown user".
  - Lookups: `member()`, `user()`, `channel()`.
  - Helpers: `display_name`, `initials`, `channel_id`.
- `src/ui/chat.rs` (new): `ChatView` owns the store, the connection and the selection, and emits `LogOutRequested` and `SessionRejected`.
  - Fetches the file server address from `GET /` once.
  - **Server rail:** Home button, then servers with their uploaded icon (`img()` with initials as fallback) and a name tooltip. The selected server has a marker on the left edge.
  - **Channel list:** a header, category headings, and rows with Lucide icons (`#`, speaker, `@`, users, bookmark). The selected row is highlighted, and each place remembers its last selected channel.
  - **User panel:** display name, an Online/Connecting/Offline line, and a Log out button.
  - **Main area:** channel header and a placeholder body, plus a warning banner while reconnecting.
- `src/ui/app_root.rs`: the `SignedIn` "Logged in as" placeholder is replaced by `Screen::Chat { view, token }`. A fresh login uses `session.user_id`; a restore uses `user.id`. `SessionRejected` logs out and shows "Your session has ended" on the login screen.
- `src/ui/login.rs`: added `pub fn show_message`.
- `src/assets.rs` (new): `AppAssets` combines gpui-component's default icons with `icon_assets!(ExtraIcons, [AtSign, Bookmark, Hash, House, Users, Volume2, WifiOff])`.
- `src/http.rs` (new): `ReqwestHttpClient` implements GPUI's `HttpClient` on top of reqwest on the IO runtime. Values cross the boundary as strings and bytes, so GPUI's and reqwest's `http` versions don't need to match.
- `src/main.rs`: `.with_assets(AppAssets)` and `.with_http_client(ReqwestHttpClient::new())`.
- `src/api/client.rs`: `RootConfig.features.autumn`, `RootConfig::file_server()`, and `file_url(server, file)` = `{server}/{tag}/{id}`.
- Tests:
  - `tests/fixtures/ready.json` is an anonymized sample covering the awkward cases: an unparseable user and channel type, channels that are listed but missing, an empty category, a closed DM, an unknown DM partner, a voice channel, an icon, and saved ordering.
  - `tests/store_ready.rs`: 8 tests.
  - Unit tests: 7 for events, 1 for initials, 1 for assets, 1 `wiremock` HTTP adapter round trip.
  - Contract tests: added `file_server` and `file_url` cases.
  - `tests/live_events.rs`: an opt-in live handshake test.

### Divergences
- **Bug found in live testing, fixed: websocket 400.** `events_url` produced `wss://events.stoat.chat?…` with no path. tungstenite sends that as `GET ?version=1… HTTP/1.1`, and Stoat rejects it with 400. I confirmed this by probing with curl over HTTP/1.1 (every query-parameter variant got 101) and by reproducing it in an opt-in live handshake test. The fix always adds `/`. The unit test checks both input forms, and `cargo test --test live_events -- --ignored` now passes.
- **Minor: voice channels are listed, not hidden.** Stoat 0.16 has no voice channel type; voice-capable channels are `TextChannel`s with `voice`. Plan Assumption 6 is amended.
- **Minor: assets weren't registered before.** gpui-component icons only render when the app registers an `AssetSource`, and the app didn't, so chunk 2's password-eye toggle and button spinner were blank. Fixed with `AppAssets`.
- **Scope addition at the user's request: server icons in the rail.** The user reported "No icons are showing up" and, asked to clarify, said they meant server pictures. This needed `ReqwestHttpClient`, the file server address from `GET /`, and `ServerEntry.icon`. Plan Assumption 2 is amended, and user avatars remain out of scope.
- **Minor: requests `user_settings[ordering]`** so the rail matches the web app's server order.
- **Minor: the "Logged in as" screen and `user_label` are removed.** The chat view replaces them, and the user's name shows in the sidebar footer.
- **Tried and backed out: offscreen-snapshot debug feature.** I tried a `snapshot` feature using `gpui-kit/test-support` to render the window to PNG. Enabling it pulls in `gpui-fast`, whose exact version pins (`taffy =0.15.0`, `unicode-properties`, …) conflict with the lockfile. Cargo resolves the lockfile with all declared features, so declaring the feature would also break normal builds. I removed it and restored the lockfile (`taffy` back at 0.15.1).
- No effect on later chunks beyond the amended assumptions. Chunk 5's channel header and placeholder body slot into `ChatView::render_main`.

### Verification Performed
- `cargo test`: 54 passed, 0 failed, 3 ignored:
  - 20 unit tests
  - 9 `api_login`
  - 7 contract
  - 10 `session_restore`
  - 8 `store_ready`
- `cargo test --test live_events -- --ignored`: passed after the fix (failed with 400 before).
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live run against the user's account:
  - Logs: `events: authenticated`, then `events: Ready (5 servers, 75 channels, 29 users, 0 skipped)`, then `events: pong` every 20 s.
  - The app fetched 5 server icons from `https://cdn.stoatusercontent.com/icons/<id>`. Each returns `200 image/webp`, 1–5 KB, when fetched directly with curl.
  - No GPUI image or SVG errors at warn level.

### Manual Validation
- The user confirmed the sidebar populates. They reported missing server pictures, which are now implemented.
- After the fixes, the user confirmed "Looks right": server pictures show, and the sidebar matches the web app (server order, channels and categories, DMs).

## Chunk 5: Channel message history view

### Changes Made
- `Cargo.toml`: added `serde_urlencoded 0.7` and `chrono 0.4` (`clock`, `std`).
- `src/api/client.rs`: `fetch_messages(channel, before, limit) -> BulkMessageResponse` calls `GET /channels/{id}/messages`. The query string is serialized from the backend's own `OptionsQueryMessages`: `limit`, optional `before`, `sort=Latest` and `include_users=true`. `sort` must be explicit, because the server defaults to `Relevance`, which is for search. A unit test checks the exact query.
- `src/time.rs` (new):
  - `ulid_millis`: Crockford base32 decoding of the 48-bit time prefix, case-insensitive.
  - `ulid_local_time`, `message_time_label`, `minutes_between`.
  - `format_message_time`: "Today at 14:05", "Yesterday at 09:12", "1 Nov 2026 at 23:59".
- `src/store.rs`:
  - `History { messages (oldest first), exhausted }`.
  - `set_history`: caches the page's `users` and `members`, sorts and dedups by ULID, and sets `exhausted` when the page is shorter than the limit.
  - `author_name`: masquerade name, then webhook name, then server nickname (embedded member or store), then display name, then username, then "Unknown user".
  - `system_text`: name-aware wording such as "Carol joined" or "Ada pinned a message". Anything else falls back to the backend's `String::from(SystemMessage)`.
  - `render_content`: resolves `<@user>` and `<#channel>` mentions and leaves other `<…>` text alone.
  - `timeline(channel) -> Vec<TimelineRow>`: a `StartOfChannel` marker when exhausted, then `MessageRow { author, time, body, attachments, system, edited, continuation }`. `continuation` means the same author as the previous message, within 7 minutes.
- `src/ui/chat.rs`:
  - Per-channel `Pane { rows: Rc<Vec<TimelineRow>>, scroller: Entity<MessageScrollerState> }`, plus a `loading` map of tasks and a `load_errors` map, all keyed by channel.
  - `ensure_current_loaded` runs on `Ready`, on server or channel switch, and on startup.
  - `load_history` keeps each result in its own channel, so a slow response can't overwrite the one on screen.
  - `rebuild_pane` resets and scrolls to the end when the row count changes, and only remeasures otherwise. On reconnect `Ready`, loaded panes are rebuilt because names may have changed.
  - The main area shows "Loading messages…", an error with **Retry**, "No messages yet.", or gpui-component's `MessageScroller` (virtualized, follows the tail, opens at the bottom, has a jump-to-latest button).
  - Row renderers: a start-of-channel header; system rows as an italic muted "→ text" with the time; message rows with an author and time header, which continuations omit. The body text wraps, and rows show "(edited)" and "[attachment: name]" lines.
- Tests:
  - `tests/fixtures/messages_page.json` has 7 messages with real ULIDs at known times, newest first, plus users and a member nickname. It covers a mention, a grouped follow-up, a nickname, a system `user_joined`, an attachment with an edit, a masquerade and a webhook. `messages_bare.json` is the bare-array shape.
  - `tests/store_history.rs`: 8 tests.
  - Unit tests: 4 for `time` and 1 for the query.

### Divergences
- **Minor: used gpui-component's `MessageScroller`** instead of a hand-rolled GPUI `list()`. It already handles tail-following, prepending with the anchor kept, and jump-to-latest, which takes care of chunk 6's main risk. The plan's chunk 6 risk note is updated.
- **Minor: the author fallback is the plain username**, not `username#discriminator`, to match the sidebar's DM names and the web app. Masquerade and webhook names were added at the top of the order, because bridges and bots rely on them.
- **Minor: fetches are keyed by channel** instead of a "drop stale results" guard. Each response lands in its own channel's history and pane, so it can never replace the channel being viewed. This sits inside the GPUI view, so there's no separate unit test for it. The store-side guarantees (per-channel histories) are tested.
- **Minor, extras:**
  - mention rendering (`<@id>` becomes `@name`, `<#id>` becomes `#name`)
  - same-author grouping
  - the start-of-channel marker shown already when the first page is short (planned for chunk 6)
- **Test fix, not a code fix:** my first ULID test used the wrong expected value for `01ARZ3NDEK…`. The decoder was right. The test now uses the ULID README's `decodeTime("01ARYZ6S41…") = 1469918176385` example, which I checked independently.

### Verification Performed
- `cargo test`: 67 passed, 0 failed, 3 ignored:
  - 25 unit tests
  - 9 `api_login`
  - 7 contract
  - 10 `session_restore`
  - 8 `store_ready`
  - 8 `store_history`
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live: after `Ready`, the app requested `GET /channels/<first Home conversation>/messages?limit=50&sort=Latest&include_users=true` and logged `loaded 0 messages` (the first conversation is empty). No warnings or errors.

### Manual Validation
The user opened channels and a DM in the running app and confirmed "Looks right": the content and order match the web app.

## Chunk 6: Live messages and older-history paging

### Changes Made
- `src/api/events.rs`: `ServerEvent` gains these variants, all with `revolt-models` payloads, decoded through a shared `decode_as` helper that turns unparseable events into `Unknown` and logs them:
  - `Message(Box<Message>)`
  - `MessageUpdate { id, channel, data: Box<PartialMessage>, clear: Vec<FieldsMessage> }`
  - `MessageDelete { id, channel }`
  - `BulkMessageDelete { channel, ids }`

  The session loop forwards them to the UI. 2 new unit tests (a Bulk frame with all four events, and a malformed message).
- `src/store.rs`:
  - `apply_message`: binary-search insert or replace by ULID in a loaded channel. It also caches the embedded user and member, and updates the channel's `last_message_id`, so Home reorders even for unloaded DMs.
  - `apply_update`: merges with the backend's generated `Message::apply_options(PartialMessage)`, then clears `FieldsMessage::Pinned` if listed.
  - `apply_delete(ids)`.
  - `prepend_page`: merges, sorts and dedups, and sets `exhausted` on a short page.
  - Also: `oldest_message_id`, `clear_histories`, and a shared `absorb_page`.
  - `diff_rows(old, new) -> Vec<RowOp>`: a minimal diff over rows keyed by message ID. It produces `Prepend` (keeps the anchor), `Append`, `Remove`, `Insert`, `Remeasure` (same row with new content, or a grouping change) and `Reset`. A surviving old row anchors the diff, so deleting the oldest row doesn't force a reset.
- `src/ui/chat.rs`:
  - Live events are applied to the store, then `sync_pane` turns `diff_rows` into `MessageScrollerState` calls (`prepend`, `append`, `splice`, `remeasure_items`, `reset`). This replaces chunk 5's `rebuild_pane`. A new message keeps the view at the bottom (tail-following), an edit remeasures one row, and an older page prepends with the anchor kept.
  - Paging:
    - Each pane observes its scroller.
    - Each render defers a `maybe_load_older` check, which also covers content too short to scroll.
    - If the view is at the top (`logical_scroll_top().item_ix == 0`) and history isn't exhausted, it fetches `before = oldest`.
    - A "Loading older messages…" pill shows at the top while loading, or the error with **Retry**. A failure stops automatic retries until Retry is clicked.
  - On a **reconnect** `Ready`, all loaded history, panes and in-flight loads are dropped and the current channel reloads. This avoids gaps from events missed while offline.
  - Debug logs record each live event applied.
- `tests/store_live.rs` (new): 12 tests.
  - Store behaviour: live append and dedup, unloaded-channel Home reorder, partial update merge with Pinned clear, single and bulk delete, older-page merge with overlap.
  - `diff_rows` (append, prepend with start marker, edit, middle delete, oldest-row delete, mixed changes, no overlap): every case is replayed on a simulated list, which must reproduce the new rows exactly, with untouched rows unchanged.
- `README.md`: feature list updated, plus the third opt-in check (`live_events`).

### Divergences
- **Minor: reconnect reloads history instead of showing a note** (plan Assumption 10). Reloading the open channel after a reconnect means no gaps, which is better than a visible "reconnected" note over possibly missing messages.
- **Minor: `BulkMessageDelete` handled too.**
- **Minor: the start-of-channel marker** already came from chunk 5. Paging only reaches it when a page comes back short.

### Verification Performed
- `cargo test`: 81 passed, 0 failed, 3 ignored:
  - 27 unit tests
  - 9 `api_login`
  - 7 contract
  - 10 `session_restore`
  - 8 `store_ready`
  - 8 `store_history`
  - 12 `store_live`
- `cargo test -- --ignored`: all 3 opt-in checks passed (live API drift, live websocket handshake, real Keychain round trip with no leftover item).
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live session against the user's account, about 14 minutes (debug log):
  - Many `live: new message` events, including in unloaded channels, which only reorder Home.
  - In an open conversation, the user sent a message, edited it, then deleted it, and all three were applied to the loaded history: `live: new message …ZVH0…`, `live: edit …ZVH0…` and `live: delete …ZVH0…` (02:18:16, 02:18:25, 02:18:49).
  - A **real server-side disconnect** (`Connection reset by peer`) at 02:13:56. The app retried after 1 s, authenticated, received `Ready`, logged `reconnected: reloading message history`, and reloaded the open channel (20 messages).
  - Keepalive: 35 pongs at 20 s intervals, with no warnings or errors for the whole session.
  - Paging in a busy channel: the first page loaded 50, then each scroll to the top fetched `before=<oldest>`, with totals going 100, 150, 200, 250, 300. Each step added exactly 50 (no duplicates), and each cursor was strictly older. Fetching stopped when the user stopped scrolling (nothing more in the following 12 s), so it isn't a runaway chain. The first older page came about 1 s after opening. That's consistent with the user scrolling straight away, but with second-level log timestamps one early prefetch at open can't be ruled out. Either way it would be a single page.

### Manual Validation
- The user confirmed live new, edit and delete work, and that paging works.
- The user didn't page all the way to the start of the busy channel. The end-of-history marker is covered by `older_page_merges_without_duplicates` and shown live for short channels (chunk 5).

## Summary

_Completed: 2026-11-04_

### Chunks Completed
1. Project scaffold and window shell: `5e60bc3`
2. Stoat REST client and login screen with 2FA, on `revolt-models`/`revolt-result`: `d1f0032`
3. Keychain session persistence and logout: `a0a4bad`
4. Events websocket and sidebar (servers with icons, channels, DMs): `98415ec`
5. Channel message history view: `f1bfa2d`
6. Live messages and older-history paging: this commit

### Divergences
- **User-directed:** reuse the Stoat backend's crates. `revolt-models` and `revolt-result` (MIT) supply all wire types and errors, including `Message::apply_options` for edits and `OptionsQueryMessages` for queries. The AGPL `revolt-database` is avoided, and the client has its own small `ServerEvent` enum.
- **Toolchain:** Rust 1.99.1 pinned in `rust-toolchain.toml`, because `gpui-pre` needs `std::hint::cold_path`. The crate is split into a library and a thin binary.
- **Libraries:**
  - `keyring` 4.2 instead of 3.x.
  - The Keychain account is the API URL, so tokens are kept per instance.
  - Log Out has no shortcut (⇧⌘Q belongs to macOS).
  - The local token is cleared before calling the server on logout.
- **Bug fixed in live testing:** the websocket handshake got a 400 because the events URL had no `/` path.
- **Fixes found along the way:**
  - The asset source wasn't registered, so icons were blank.
  - Server icons were added at the user's request, using a `reqwest`-backed GPUI `HttpClient`.
  - Voice-capable channels are listed, since Stoat 0.16 has no voice channel type.
- **Message list:** gpui-component's `MessageScroller`, plus the `diff_rows` scroller diff.
- **Display:** the plain username is the final author fallback, with masquerade and webhook names ahead of it. Mentions are resolved and same-author messages are grouped.
- **Reconnect** reloads history instead of showing a note.
- **Tried and backed out:** an offscreen-snapshot debug feature, because `gpui-kit/test-support` pulls in `gpui-fast` with exact version pins that conflict with the lockfile.

### Definition of Done
- [x] **Build:** `cargo build` succeeds with `gpui-kit = "=0.9.0"`, and `cargo run` opens the app.
- [x] **Tests:** `cargo test && cargo clippy --all-targets -- -D warnings` passes (81 tests, 0 warnings).
- [x] **Login with 2FA:** email/password login works against `api.stoat.chat` (user-confirmed). An MFA response leads to the code prompt, and TOTP and recovery codes both complete login. Both are covered by `wiremock` tests. The user confirmed login works but didn't say separately that they used the 2FA step.
- [x] **Login errors:** invalid credentials show "Incorrect email or password." inline. This is covered by a `wiremock` test, but it wasn't observed live.
- [x] **Session persistence:**
  - The token is saved to the Keychain on login and reused on relaunch (verified live).
  - A 401 at startup clears it; a network failure at startup keeps it. Both are `session_restore` tests.
- [x] **Logout:** Log out calls `POST /auth/session/logout`, deletes the Keychain entry and returns to the login screen. Verified live: the log said `logged out`, the item was gone, and the user confirmed the session was gone in Stoat.
- [x] **Sidebar:** lists every joined server with its text channels grouped by category, plus DMs, groups and Saved Notes, all from `Ready`. User-confirmed against the web app, including server order and icons.
- [x] **Keepalive:** the websocket pings every 20 s. It stayed connected through a 14-minute session, and when the server reset the connection it reconnected with backoff after 1 s.
- [x] **History view:** opening a channel shows its latest 50 messages oldest to newest, with author name, local timestamp and plain text (user-confirmed). System messages and attachments have readable placeholders (covered by tests).
- [x] **Live updates:** `Message`, `MessageUpdate` and `MessageDelete` update loaded channels. The user confirmed this, and the log shows the events applied within a second of arriving.
- [x] **Paging:** scrolling to the top loads older pages of 50 without duplicates (verified live up to 300 messages). The start-of-channel marker at the end of history is covered by tests and shown live for short channels.
- [x] **README:** documents running the app, `STOAT_API_URL`, the expected Keychain prompt during development, and the test commands.

### Remaining Manual Steps
None required. Optional:
- run the plan's Wi-Fi off/on manual test (a real server-side disconnect was already handled)
- page a busy channel all the way to its start, to see the marker after paging
