# Plan: Stoat Chat GPUI Client (POC)

## Summary
Build a proof-of-concept native desktop client for Stoat Chat (the service formerly called Revolt) in Rust with GPUI. The POC covers logging in, including TOTP 2FA, and reading text chats in servers, DMs and groups. It shows live new messages and can page back through older history. The point is to find out whether GPUI is a workable foundation for a full Stoat client.

## Approach
Use **`gpui-kit = "=0.9.0"`**. It is one crate that pins a matching GPUI build (`gpui-pre 0.3.11`, a snapshot of Zed) and re-exports `gpui-component`. That library supplies the text `Input` (with password masking), buttons, virtual lists, scrolling and theming that a chat client needs. Raw GPUI has no text input widget, and the `gpui 0.2.2` crate on crates.io is about a year behind Zed. I rejected two alternatives:
- Raw `gpui 0.2.2`: we would have to write our own text input, starting from Zed's `input.rs` example.
- A git dependency on Zed: unpinned, slow to build, and its API changes under us.

Networking runs on a **dedicated Tokio runtime on a background thread**. That runtime handles `reqwest` for REST and `tokio-tungstenite` for the events websocket. GPUI has its own executor, so results come back to the UI through executor-agnostic `futures::channel` oneshot and mpsc channels. GPUI tasks (`cx.spawn`) await those channels and update entities.

**Reuse the Stoat backend's own Rust crates for every wire type.** Don't hand-write serde models. The backend (`stoatchat/stoatchat`) publishes its core crates to crates.io under the old `revolt-*` names, at the same version the live API reports (0.16.2). Checked on 2026-11-04:
- **`revolt-models = { version = "=0.16.2", default-features = false, features = ["serde", "partials"] }`** (MIT). Its `v0` module holds:
  - login: `DataLogin`, `ResponseLogin`, `MFAResponse`, `MFAMethod`
  - entities: `User`, `Server`, `Category`, `Channel` (tagged on `channel_type`), `Member`, `Message`, `SystemMessage`
  - paging: `BulkMessageResponse`, `OptionsQueryMessages`, `MessageSort`
  - partial updates: `PartialMessage`, `FieldsMessage`, `AppendMessage`

  It also has helpers such as `impl From<SystemMessage> for String`, which gives system messages readable text. Turning off the default features drops the `rocket` server integration. A scratch build confirmed it compiles as a client dependency and parses real `ResponseLogin`, `Channel` and `PartialMessage` JSON.
- **`revolt-result = { version = "=0.16.2", default-features = false, features = ["serde"] }`** (MIT). Error bodies deserialize into `revolt_result::Error`, whose `ErrorType` is tagged on `type` (`InvalidCredentials`, …), so we don't redefine error codes.
- **`revolt-permissions`** (MIT) is already a transitive dependency. Use it only if `Ready` turns out to include channels you can't view.
- **Not reused: `revolt-database`.** It's AGPL-3.0 and pulls in MongoDB, Redis, RabbitMQ and SMTP clients. Unfortunately it's also where the websocket event enum (`EventV1`) lives. So the client defines its own small `ServerEvent` enum, tagged on `type` with the variant names from the events protocol, covering only the events we use. Every payload inside it is a `revolt-models` type. Write it from the protocol docs and field names, not by copying `EventV1`'s source, so the client stays clear of the AGPL.
- **Rejected:** community wrappers on crates.io (`stoat-rs`, `mutiny-rs`, `kaho`). They're unofficial and may lag behind the API. The backend's models are official and match the API version exactly.

The Stoat REST API has **no endpoint that lists joined servers**. That data arrives only in the websocket `Ready` event, so the websocket is required for the sidebar, not optional. The same connection then delivers `Message*` events for live updates.

Key API facts, checked on 2026-11-04 against `https://api.stoat.chat/openapi.json` (API version 0.16.2):
- `GET https://api.stoat.chat/` returns config. Its `ws` field is the events URL, currently `wss://events.stoat.chat`.
- `POST /auth/session/login` takes `{email, password, friendly_name}` or `{mfa_ticket, mfa_response: {totp_code} | {recovery_code}, friendly_name}`. The response is tagged on `result`:
  - `"Success"`: `{_id, user_id, token, name, last_seen}`
  - `"MFA"`: `{ticket, allowed_methods: ["Password"|"Recovery"|"Totp"]}`
  - `"Disabled"`: `{user_id}`
- Authenticated requests send the header `x-session-token: <token>`.
- `GET /users/@me` validates a saved token. `POST /auth/session/logout` ends the session.
- `GET /channels/{id}/messages?limit=50&include_users=true[&before=<msg id>]` returns `{messages, users, members?}` (newest first by default).
- Websocket: `wss://events.stoat.chat?version=1&format=json&token=<token>`. The server sends `Authenticated`, then `Ready` (`users, servers, channels, members, emojis…`). The client must send `{"type":"Ping","data":<n>}` every 10–30 s. The events this POC needs are `Message`, `MessageUpdate` and `MessageDelete`, plus `Error` (bad token) and `Logout`.
- IDs are ULIDs, so a message's timestamp comes from its `_id`.

## Assumptions
1. The client points at the official instance, `https://api.stoat.chat`, because that is where your account is. The `STOAT_API_URL` environment variable overrides it for self-hosted instances. The events URL always comes from the API root's `ws` field and is never hardcoded.
2. Messages render as **plain text**, with no markdown, emoji images, embeds or avatars. *(Amended in chunk 4: server icons in the rail **are** shown. GPUI's `img()` loads them from the file server through a `reqwest`-backed `HttpClient`, so user avatars are a small step if wanted later.)* Attachments show as `[attachment: filename]`. Rich rendering is a separate milestone.
3. The POC is **read-only**: no sending, editing, reactions or typing indicators. You chose live updates and older history, not sending.
4. Live updates apply to channels whose history is already loaded. Unread badges and mention counts are out of scope.
5. A message's author name resolves in this order: server member nickname, then the user's `display_name`, then `username#discriminator`, then the raw ID. Users missing from the `Ready` cache are filled in from `include_users=true` on each history fetch.
6. `TextChannel`, `DirectMessage`, `Group` and `SavedMessages` channels are listed. *(Amended in chunk 4: Stoat 0.16 has no separate voice channel type. Voice-capable channels are `TextChannel`s with `voice: Some(..)` and have text chat too, so they're listed with a speaker icon instead of being hidden.)* `revolt-models`' `Channel` enum has no catch-all variant, and we can't add one to a dependency. So `Ready`'s lists are first parsed as `Vec<serde_json::Value>`, then each item is converted to its model type on its own. An item that fails (for example a new `channel_type`) is logged and skipped, without failing the whole `Ready`.
11. `revolt-*` crates are pinned with `=0.16.2` to match the live API version. If the server moves ahead and parsing breaks, the fix is to bump all `revolt-*` crates together.
7. The target is macOS (your machine, with Xcode and Metal available). Nothing should deliberately block other platforms, but only macOS is tested.
8. The session token is stored in the macOS Keychain with the `keyring` crate (v3, `apple-native` feature), as service `chat.stoat.gpui-poc` and account `session`. You chose Keychain persistence.
9. Login sends `friendly_name: "Stoat GPUI POC"`, so the session is easy to find and revoke in Stoat's Sessions settings.
10. Reconnect handling is basic: after a drop, retry with exponential backoff (1 s → 30 s cap) and re-apply the `Ready` that follows. Messages missed during the gap are not backfilled, and the POC says so with a visible "reconnected" note in the open channel.

## Open Questions
- *(Deferred)* Do you want rich rendering (markdown, avatars, embeds) in a follow-up milestone, or is plain text enough to judge GPUI?
- *(Deferred)* Should the project move to a permanent folder such as `~/Git/Personal/`? This does not block implementation.

## Plan

### Chunk 1: Project scaffold and window shell
**Goal:** A Cargo binary that opens a themed GPUI window with a placeholder view and has a working Tokio bridge. This proves the toolchain and `gpui-kit` build on this machine before any app code depends on them.
**Files likely touched:** `Cargo.toml`, `rust-toolchain.toml` (pins Rust 1.99.1, because `gpui-pre` needs `std::hint::cold_path`), `src/lib.rs` (library crate, so `tests/` can import it), `src/main.rs` (thin binary), `src/runtime.rs` (Tokio runtime and `spawn_io` helper), `src/ui/mod.rs`, `src/ui/app_root.rs`, `.gitignore`
**Depends on:** None
**Risks:**
- The first GPUI build is slow (several minutes) and compiles Metal shaders through `xcrun metal`. If that fails, the Xcode setup is the problem, not our code.
- The `gpui-kit` docs may lag the 0.9.0 API. Pin `=0.9.0` and read the crate source in `~/.cargo/registry/src` when the docs disagree.
- If the Tokio bridge pattern is wrong, every later chunk inherits the bug, so test it here.
**How to verify:**
- `cargo build` succeeds.
- `cargo test` passes a unit test showing `spawn_io` runs a Tokio future (for example `tokio::time::sleep` plus a return value) and delivers its result through a oneshot that a plain `futures::executor::block_on` can await.
- `cargo run` opens a window titled "Stoat" with placeholder text.
**Manual check:** The window opens, resizes and closes cleanly with Cmd+Q.
**Shippable when:** The app builds, opens a window and quits without errors, and the runtime bridge test passes.

### Chunk 2: Stoat REST client and login screen (with 2FA)
**Goal:** Log in with email and password, completing a TOTP or recovery-code step when the API asks for one, then show "Logged in as <username>". This is the first real user-facing flow.
**Files likely touched:**
- `Cargo.toml`: add `revolt-models`, `revolt-result`, `reqwest` (rustls, json), `serde`, `serde_json`; dev-deps `wiremock`, `tokio`
- `src/api/mod.rs`: re-exports `revolt_models::v0` as `models`
- `src/api/error.rs`: `ApiError` enum with `Api(revolt_result::Error)`, `Unauthorized`, `Network`, `Decode`
- `src/api/client.rs`: `StoatClient` with `root_config()`, `login()`, `login_mfa()`, `fetch_self()`, all using `revolt-models` request/response types
- `src/ui/login.rs`, `src/ui/app_root.rs`
- `tests/api_login.rs`
**Depends on:** Chunk 1
**Risks:**
- The API root config (`GET /`) may not have a matching type in `revolt-models`. If not, define a two-field struct (`ws`, `features.autumn.url`) instead of pulling in `revolt-config`'s server config types.
- `revolt_result::ErrorType` covers every backend error code. Map the few we expect (`InvalidCredentials`, `InvalidToken`, `DisabledAccount`, `UnverifiedAccount`) to readable text, and show the rest via `ErrorType`'s `IntoStaticStr` name.
- `revolt-models` depends unconditionally on `revolt-config`, which brings in `sentry` and `config` (about 270 crates in total). Its only use inside `revolt-models` is in `PushNotification::from`, which we never call. Don't call `revolt_config::config()`; it tries to load a server `Revolt.toml`.
- `gpui-component` `Input` focus and Enter-to-submit wiring may take some trial and error.
- The API may rate-limit repeated failed logins, so don't hammer it while testing.
**How to verify:**
- `cargo test` runs contract tests. They parse JSON fixtures shaped like the live API (in `tests/fixtures/`) into the `revolt-models` types: all three `ResponseLogin` variants, plus an error body parsed into `revolt_result::Error`. These catch drift between the pinned crate and the server.
- `cargo test --test api_login` runs `wiremock` integration tests:
  - success returns the token
  - MFA returns a ticket, and a second call with `totp_code` returns the token
  - a 401 `InvalidCredentials` maps to an error
- `cargo clippy --all-targets -- -D warnings` is clean.
**Manual check:** You type your own email and password, plus a 2FA code if prompted, and see "Logged in as <you>". A wrong password shows an inline error. (Claude can't enter real credentials; you type them.)
**Shippable when:** A real account can log in end to end through the UI, including the 2FA path, and the automated tests pass.

### Chunk 3: Keychain session persistence and logout
**Goal:** Remember the session token so the next launch skips the login screen, and add a Log out action. Without this, every test run means typing credentials and a 2FA code again.
**Files likely touched:** `src/session.rs` (`load_token`, `save_token`, `clear_token` over `keyring`), `src/ui/app_root.rs` (startup state: `Checking → LoggedOut | LoggedIn`), `src/api/client.rs` (`logout()`), `src/main.rs` (menu item and keybinding for Log out)
**Depends on:** Chunk 2
**Risks:**
- macOS may show a Keychain "allow access" prompt after each rebuild, because the unsigned binary's code signature changes. This is expected in development. Note it in the README instead of working around it.
- At startup, tell a revoked token (401 on `/users/@me`: clear it, show login) apart from a network failure (keep the token, show a retry state). Don't wipe valid tokens when you're offline.
**How to verify:**
- `cargo test` covers the startup state decision with a fake `TokenStore` trait (token present + 200 → LoggedIn, token present + 401 → cleared + LoggedOut, network error → Offline/retry), so the tests never touch the real Keychain.
- Log in, quit and relaunch with `cargo run`: no login screen.
**Manual check:** Log out returns to the login screen. In the Stoat web app's Sessions settings, the "Stoat GPUI POC" session is gone afterwards.
**Shippable when:** Relaunching keeps you logged in, logging out ends the server session and clears the Keychain entry, and the tests pass.

### Chunk 4: Events websocket and sidebar (servers, channels, DMs)
**Goal:** Connect to the events websocket, parse `Ready`, and show a server rail, the selected server's text channels (ordered by category) and a Home entry listing DMs, groups and Saved Notes. After this chunk you can browse everything you're in.
**Files likely touched:**
- `src/api/events.rs`:
  - our own `ServerEvent` enum, tagged on `type`, with `Authenticated`, `Ready`, `Pong`, `Error`, `Logout` and an `#[serde(other)] Unknown` catch-all. `Ready` holds lenient lists that become `revolt-models` `User`, `Server`, `Channel` and `Member`.
  - the outgoing `Ping`
  - the connection task, with ping every 20 s, reconnect backoff, and sending parsed events into an mpsc channel
- `Cargo.toml`: add `tokio-tungstenite` (rustls)
- `src/store.rs`: an `Entity<ChatStore>` holding `users`, `servers`, `channels` and `members` maps, plus a `selected` state
- `src/ui/sidebar.rs`, `src/ui/app_root.rs`
- `tests/fixtures/ready.json`: a trimmed, anonymized sample
**Depends on:** Chunk 3 (the startup path supplies the token). Chunk 2 alone is enough if Chunk 3 slips.
**Risks:**
- `Ready` is large. The `revolt-models` types are strict, with no catch-all `Channel` variant, so a single new field shape could fail a whole list. Parse each list item on its own (Assumption 6) so one bad entry can't take down the sidebar. Log how many items were skipped.
- Our `ServerEvent` tags must match the server's exactly. Check the variant names against a real event capture (`RUST_LOG=stoat_gpui::api::events=trace` logs the raw frames) and add a fixture for each event we handle.
- DM display names need the other participant's user from `Ready.users`: `DirectMessage.recipients` minus self.
- Channel order comes from `server.categories[].channels`, with uncategorized channels from `server.channels` first. This is easy to get subtly wrong, so unit test it.
- Websocket keepalive: missing pings cause silent drops after about 30 to 60 s.
**How to verify:**
- `cargo test` covers:
  - parsing `tests/fixtures/ready.json` into the store
  - channel ordering with categories
  - DM naming
  - the `Ping` serialization format
  - an unknown `channel_type` parsing as `Other`
- `RUST_LOG=stoat_gpui=debug cargo run` logs `Authenticated`, `Ready (N servers, M channels)` and a `Pong` at least every 30 s.
**Manual check:** Every server you're in appears in the rail. Clicking one shows its text channels under the right category headers. Home shows your DMs with the other person's name.
**Shippable when:** The sidebar matches what the Stoat web app shows for your account, stays connected for more than 5 minutes idle, and the tests pass.

### Chunk 5: Channel message history view
**Goal:** Selecting any channel or DM loads its most recent 50 messages and shows them oldest to newest, with author name, timestamp and plain-text content. This is the core "view text chats" requirement.
**Files likely touched:**
- `src/api/client.rs`: `fetch_messages(channel, before: Option<Id>, limit)`. It returns `revolt-models`' `BulkMessageResponse` (already `#[serde(untagged)]` over both shapes). The query string is built from `OptionsQueryMessages` and `MessageSort`.
- `src/store.rs`: per-channel `MessageList` holding a sorted `Vec<Message>`, an `oldest_loaded` marker and an `exhausted` flag
- `src/ui/channel_view.rs`: header with the channel name, then the message list
- `src/ulid_time.rs`: ULID to local timestamp
**Depends on:** Chunk 4
**Risks:**
- `BulkMessageResponse` comes from `revolt-models` and has two shapes. With `include_users=true` it should always be the object form, but handle the bare-array form too (no user info).
- Messages come back newest-first, so reverse them for display.
- Long messages need text wrapping inside the list items, and variable-height rows can make scroll position jumpy.
- System messages (`system` set, `content` null) need a one-line description, not a blank row. Start from the backend's own `String::from(SystemMessage)` and swap user IDs for names where the store has them.
- Switching channels fast must not let a slow earlier fetch overwrite the current channel. Tag each fetch with its channel ID and drop stale results.
**How to verify:**
- `cargo test` covers:
  - `BulkMessageResponse` in both shapes
  - newest-first to display-order conversion
  - author resolution precedence (nickname > display_name > username > ID)
  - ULID to timestamp against a known ID
  - system-message text
  - the stale-fetch guard
**Manual check:** Open three different channels and a DM. Content and order match the web app, long messages wrap, and the view opens scrolled to the bottom.
**Shippable when:** Any text channel or DM shows its latest 50 messages correctly, and the tests pass.

### Chunk 6: Live messages and older-history paging
**Goal:** New, edited and deleted messages show up in loaded channels in real time. Scrolling to the top of a channel loads the previous 50, which completes the reading experience.
**Files likely touched:**
- `src/api/events.rs`: add the `Message(Message)`, `MessageUpdate { id, channel, data: PartialMessage, clear: Vec<FieldsMessage> }` and `MessageDelete { id, channel }` variants to `ServerEvent`, with all payload types from `revolt-models`
- `src/store.rs`: `apply_event`; `prepend_page` with ID-based de-duplication and an `exhausted` flag when a page has fewer than `limit` messages
- `src/ui/channel_view.rs`: stick to the bottom when new messages arrive and the user is already at the bottom, a top-of-list trigger for loading older messages, a loading indicator, and keeping the scroll anchor while prepending
- `src/api/events.rs`: forwarding these events
**Depends on:** Chunk 5
**Risks:**
- Keeping the scroll position while prepending variable-height rows was the hardest UI problem in this plan. *(Updated after chunk 5: the message list is `gpui-component`'s `MessageScroller`. Its `MessageScrollerState::prepend(count)` keeps the scroll anchor, `append` with tail-following handles sticking to the bottom, and `logical_scroll_top().item_ix == 0` signals that the user reached the top, which triggers the next page. Use those rather than hand-rolled offsets. The fallback is still a "Load older messages" button at the top.)*
- The start-of-channel marker already exists (chunk 5 shows it when a page comes back short); chunk 6 only has to add it when paging reaches the end.
- A live message arriving while an older page is in flight must not be lost or duplicated. Merge by ID and keep the list sorted by ULID.
- `MessageUpdate` carries a `PartialMessage` plus a `clear` list. Merge only the fields present (`content`, `edited`) and remove the fields named in `clear`. Check whether `revolt-models` already provides an `apply_options`-style merge for `Message` before writing one.
- Live messages for channels that haven't been loaded are ignored. They load fresh when opened, per Assumption 4.
**How to verify:**
- `cargo test` covers:
  - applying `Message` to a loaded channel and ignoring it for an unloaded one
  - `MessageUpdate` partial merge
  - `MessageDelete` removal
  - merging a prepended page with overlapping IDs (no duplicates, sorted)
  - `exhausted` set when a page has fewer than `limit` messages
**Manual check:**
- From the Stoat web app or phone, send, edit and delete a message in a channel open in the POC. Each change appears within about 1 s.
- Scroll up in a busy channel. Older messages load without the view jumping, and the beginning of the channel eventually shows a "start of channel" marker.
**Shippable when:** Live create/edit/delete and older-history paging work in a real busy channel, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`. These are built-in cargo subcommands, so no script is needed. Run them after every chunk.

Optional drift check against the live API: `cargo test --test api_login -- --ignored`.

**Gotcha:** in modules that `use gpui_kit::*`, test submodules must import items explicitly instead of using `use super::*`. Otherwise GPUI's `test` macro shadows `#[test]`.

### Unit Tests
- Contract tests: JSON fixtures in `tests/fixtures/`, shaped like the live API, parse into the pinned `revolt-models` / `revolt-result` types. This guards against drift between the crate and the server. Cover:
  - `ResponseLogin` (Success / MFA / Disabled)
  - `revolt_result::Error` bodies
  - `Ready`
  - `BulkMessageResponse` (both shapes)
  - `MessageUpdate` partial data
- Our own `ServerEvent` enum: each handled `type` tag parses, an unrecognized tag becomes `Unknown`, and a `Ready` list item with an unknown `channel_type` is skipped without failing the rest.
- `ChatStore` logic:
  - channel ordering by category
  - DM naming
  - author name precedence
  - message merge, de-dupe and sort
  - live event application
  - the `exhausted` flag
  - the stale-fetch guard
- Utilities: ULID to timestamp, and the `Ping` payload shape.
- Startup state decision with a fake `TokenStore` (200 / 401 / network error).
- The Tokio bridge (`spawn_io`) delivering results to a non-Tokio executor.

### Integration Tests
Use `wiremock` against `StoatClient`:
- Happy path: login success, then `fetch_self`, then `fetch_messages`.
- MFA path: login returns MFA, then a TOTP call returns Success.
- Failures:
  - 401 `InvalidCredentials` → readable error
  - 401 on `/users/@me` → `Unauthorized`
  - connection refused → `Network` error, distinct from `Unauthorized`
- Optional, if time allows: a local `tokio-tungstenite` server test that sends `Authenticated` and `Ready` and checks that the events task emits them and sends a `Ping`.

### Manual Tests
1. A fresh launch with no Keychain entry shows the login screen. A wrong password shows an inline error.
2. Log in with 2FA, entering a TOTP code. You land in the app.
3. Quit and relaunch. You're still logged in, with no login screen.
4. Browse every server and the Home/DM list. Names and channel order match the web app.
5. Open a busy channel. You see the latest messages, scrolled to the bottom.
6. Send, edit and delete a message from another client. Each change shows up live.
7. Scroll to the top repeatedly until "start of channel" appears.
8. Leave the app idle for 10 minutes, then send a message from another client. It still arrives (keepalive works).
9. Turn Wi-Fi off and on. The app reconnects and shows a "reconnected" note.
10. Log out. The login screen returns, and the session is gone from Stoat's Sessions settings.

## Definition of Done
- [ ] `cargo build` succeeds on macOS with `gpui-kit = "=0.9.0"`, and `cargo run` opens the app.
- [ ] `cargo test && cargo clippy --all-targets -- -D warnings` passes with no failures or warnings.
- [ ] Email/password login works against `api.stoat.chat`. An MFA response leads to a code prompt, and both TOTP and recovery codes complete the login.
- [ ] Invalid credentials show a readable inline error, never a panic or a silent no-op.
- [ ] The session token is saved to the Keychain on login and reused on relaunch. A 401 at startup clears it and shows login. A network failure at startup does not clear it.
- [ ] Log out calls `POST /auth/session/logout`, deletes the Keychain entry and returns to the login screen.
- [ ] The sidebar lists every joined server with its text channels grouped by category, plus DMs, groups and Saved Notes under Home, all from the websocket `Ready` event.
- [ ] The websocket sends `Ping` every 20 s and stays connected for more than 10 minutes idle. It reconnects with backoff after a drop.
- [ ] Opening a channel shows its latest 50 messages oldest-to-newest, with the resolved author name, a local timestamp and plain-text content. System messages and attachments have readable placeholders.
- [ ] Live `Message`, `MessageUpdate` and `MessageDelete` events update loaded channels within about 1 s.
- [ ] Scrolling to the top loads older pages of 50 without duplicates and shows a start-of-channel marker when history runs out.
- [ ] A `README.md` documents how to run the app, the `STOAT_API_URL` override and the expected Keychain prompt during development.
