# Implementation Record: RuSToat for iPhone, milestone 1

_Plan: docs/plans/ios-app.md_
_Started: 2026-11-13_

Mode: continuous, with every manual check batched to the end (user's choice at pre-flight). XcodeGen may be installed with Homebrew.

## Chunk 1: Build the library without GPUI, for iOS

### Changes Made
- `Cargo.toml`:
  - Added `[workspace] members = ["."]`.
  - Added `[features] default = ["desktop"]` with `desktop = ["dep:gpui-kit", "dep:livekit", "dep:keyring"]`, and made those three dependencies optional.
  - Added an explicit `[[bin]] stoat-gpui` with `required-features = ["desktop"]`.
  - Moved `tokio-tungstenite`'s TLS feature into target-specific sections: `rustls-tls-native-roots` for `cfg(not(target_os = "ios"))` and `rustls-tls-webpki-roots` for `cfg(target_os = "ios")`.
- `src/lib.rs`: put `#[cfg(feature = "desktop")]` on `assets`, `banners`, `dock`, `http`, `login_item`, `shortcuts`, `sound`, `themes`, `ui`, `voice` and `window_hide`, and added a doc paragraph about the feature.
- `src/rail.rs`: gated the `gpui_kit::{Rgba, rgb}` import, `parse_colour` and its test behind `desktop`.
- `src/session.rs`: gated `KeychainStore`, its impls, `MigratingStore::keychain()` and the ignored Keychain test behind `desktop`. `TokenStore`, `MigratingStore`, `MemoryStore` and the restore logic are still shared.
- `rust-toolchain.toml`: added `targets = ["aarch64-apple-ios", "aarch64-apple-ios-sim"]`.
- `README.md`: added a paragraph under "Tests" on the `desktop` feature, with the no-default-features check commands.
- `Cargo.lock`: webpki-roots entries for the iOS target.

### Divergences
- **Workspace members:** the workspace lists only `"."` for now. `ios/stoat-ffi` doesn't exist until chunk 2, and Cargo rejects missing members. Chunk 2's file list in the plan now says to add it.
- **Smaller than expected:** `prefs.rs` and `places.rs` (flagged as risks) needed no changes. The `render_icon` example needs no `required-features`, since it only uses `resvg`. `main.rs` and `build.rs` were left as they were.
- **macOS-only `objc2-*` dependencies** stay unconditional. They're `cfg(target_os = "macos")` already, so they never reach iOS, and `emoji.rs` (shared) uses `objc2-core-text` on macOS.

### Verification Performed
- `cargo check --lib --no-default-features --target aarch64-apple-ios`: OK.
- `cargo check --lib --no-default-features --target aarch64-apple-ios-sim`: OK.
- `cargo clippy --lib --no-default-features -- -D warnings` (host) and `… --target aarch64-apple-ios`: clean.
- `cargo tree --no-default-features --target aarch64-apple-ios -p stoat-gpui`: no `gpui-kit`, `livekit`, `keyring` or `rustls-native-certs`; `webpki-roots` present.
- `cargo test`: 1128 passed, 0 failed, 5 ignored, across 67 test binaries (~2 min 16 s).
- `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
Deferred to the end of the milestone: `cargo run` still launches the desktop app and connects.

## Chunk 2: The FFI crate, the XCFramework, and an app that launches

### Changes Made
- `Cargo.toml`: workspace members are now `[".", "ios/stoat-ffi"]`. `Cargo.lock` gained UniFFI 0.32.2 and its dependencies.
- `ios/stoat-ffi/Cargo.toml`: crate `stoat-ffi` 0.1.0 with `crate-type = ["staticlib", "lib"]`. It depends on `stoat-gpui` with `default-features = false`, and on `uniffi = "=0.32.2"` and `thiserror`. A `bindgen` feature (`uniffi/cli`) gates the `uniffi-bindgen` binary (`required-features`), so clap and `uniffi_bindgen` stay out of the app's library.
- `ios/stoat-ffi/uniffi.toml`: Swift module `StoatCore`, FFI module `StoatCoreFFI`, generated modulemap.
- `ios/stoat-ffi/src/lib.rs`:
  - `setup_scaffolding!()`.
  - `CoreError::Failed { message }`, a flat user-facing error, with a `CoreError::failed(impl Display)` constructor.
  - `core_version()` (the crate's version) and `normalize_instance(input)`.
  - Two unit tests: the version matches `Cargo.toml`, and normalisation gives desktop's text.
- `ios/stoat-ffi/src/bin/uniffi-bindgen.rs`: `uniffi::uniffi_bindgen_main()`.
- `scripts/ios/build-core.sh`:
  - Always re-executes itself under `env -i`, so Xcode's SDK and toolchain variables don't leak into aws-lc's C build, and Xcode and terminal runs share Cargo fingerprints.
  - Builds `--release` for `device`, `simulator` or both (chosen from Xcode's `PLATFORM_NAME`), with `IPHONEOS_DEPLOYMENT_TARGET=26.0`.
  - Only when a slice's `.a` is newer than the XCFramework: generates Swift bindings in library mode from the `.a`, assembles headers plus `module.modulemap`, runs `xcodebuild -create-xcframework` with every built slice into `ios/build/StoatCore.xcframework`, and copies `StoatCore.swift` into `ios/RuSToat/Generated/`. A no-op run takes about 0.4 s.
- `scripts/ios/test.sh` (the plan's optional wrapper):
  - Runs the no-default-features clippy and iOS check, `cargo test`/`clippy -p stoat-ffi`, `build-core.sh` and `xcodegen generate`.
  - Picks a simulator by UDID (first iPhone, or `IOS_SIMULATOR=<name>`), boots it with `simctl bootstatus -b`, then runs `xcodebuild … -destination id=<udid> test`.
- `ios/project.yml`:
  - `RuSToat` app: iOS 26, iPhone only, bundle ID `com.chrishultin.rustoat.ios`, Swift 6.0, generated Info.plist with `CFBundleDisplayName` "RuSToat" and a launch screen. It links the XCFramework (not embedded), with `OTHER_LDFLAGS` `-lc++ -framework Security -framework SystemConfiguration`. A "Build Rust core" pre-build script runs `build-core.sh` on every build (`ENABLE_USER_SCRIPT_SANDBOXING = NO`).
  - `RuSToatTests` unit-test bundle hosted in the app, and a `RuSToat` scheme.
- `ios/RuSToat/RuSToatApp.swift`, `ios/RuSToat/RootView.swift`: a `TabView` with Home, Servers and You tabs (`Tab` API), `.tabBarMinimizeBehavior(.onScrollDown)`, and placeholder lists that show `coreVersion()`.
- `ios/RuSToatTests/CoreBridgeTests.swift` (Swift Testing): `normalizeInstance` succeeds, a blank address throws `CoreError.Failed` with desktop's text, and `coreVersion() == "0.1.0"`.
- `.gitignore`: added `/ios/build/`, `/ios/RuSToat.xcodeproj/` and `/ios/RuSToat/Generated/`.
- `README.md`: a new "Building for iPhone" section, and `scripts/ios/test.sh` under Tests.

### Divergences
- **Clean environment for the build script.** The plan only said to source `~/.cargo/env`. Under Xcode, aws-lc-sys's host build (for the bindgen binary) failed with `stdlib.h not found` because Xcode's environment leaked in. Re-executing under `env -i` fixed that, and also stopped Xcode and terminal builds from invalidating each other's Cargo caches.
- **Simulator boot race.** Running `xcodebuild test` against a cold simulator sometimes failed with "Application failed preflight checks (Busy)". There are also two simulators named "iPhone 17". `test.sh` therefore picks a simulator by UDID and boots it first.
- **Swift 6 language mode worked as is** with UniFFI 0.32's output, so the Swift 5 fallback wasn't needed.
- **`coreVersion()` returns `stoat-ffi`'s version.** The Rust test checks it against `ios/stoat-ffi/Cargo.toml`, and the Swift test asserts "0.1.0".
- **Info.plist keys** come from `INFOPLIST_KEY_*` build settings, not XcodeGen's `info:` block, so no generated plist lands in the source tree. Portrait-only and the team are left for chunk 7, as planned.
- **`.tabBarMinimizeBehavior(.onScrollDown)`** was added in this chunk, as the manual check implied.

### Verification Performed
- `scripts/ios/build-core.sh`: first run about 1.5 min for both slices; then "up to date" in 0.4 s, from both a terminal and Xcode.
- `(cd ios && xcodegen generate)`: OK.
- `xcodebuild … test` via `scripts/ios/test.sh`, from cold-booted iPhone 17 Pro and iPhone 17 simulators: 3/3 Swift tests passed (`CoreBridgeTests`), exit 0.
- `cargo test -p stoat-ffi`: 2 passed. `cargo clippy -p stoat-ffi --all-targets -- -D warnings`: clean, with and without `--features bindgen`.
- Desktop: `cargo test`: 1128 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- Simulator Debug app size: 30 MB, under the ~50 MB threshold for LTO, so no FFI profile change.
- Screenshot: the app launches on the iPhone 17 Pro simulator showing "Core 0.1.0" and the floating glass tab bar.

### Manual Validation
Deferred to the end of the milestone: watch the tab bar shrink while scrolling in the simulator.

## Chunk 3: Log in, 2FA, and staying logged in

### Changes Made
- `src/api/client.rs`: `StoatClient` has a `session_name` field (default `SESSION_FRIENDLY_NAME`) and a `with_session_name()` builder, and both login calls send it as `friendly_name`. Desktop behaviour is unchanged.
- `ios/stoat-ffi/src/session.rs` (new):
  - `TokenVault` callback interface (`load`/`save`/`clear`, throwing `CoreError`), bridged to `session::TokenStore` by `VaultStore`.
  - Records and enums: `Account { id, instance, user_id, name }`; `LoginOutcome { LoggedIn, NeedsMfa { instance, ticket, allows_totp, allows_recovery }, Disabled }`; `MfaAnswer { Totp, Recovery }`; `RestoreOutcome { SignedIn, NeedsLogin, Offline { account, message } }`.
  - The `Core` object, holding the token store, the account file and the current session (account plus token):
    - `Core::new(vault, data_dir)` keeps `accounts.json` in `data_dir`, and `Core::with_stores` is for tests.
    - `account()`, `saved_account()`.
    - `login(server, email, password)` runs `resolve_instance`, then `login` with session name "RuSToat (iPhone)".
    - `login_mfa(instance, ticket, answer)` strips spaces from TOTP codes.
    - On success, the account's name is fetched with `fetch_self`; the account is added or updated, `set_session` and `set_active` are recorded, and the token is saved under `{instance}#{user_id}`.
    - `restore()` wraps `session::restore`.
    - `log_out()` clears the token, ends the Stoat session and removes the account from the list.
    - A Rust-only `client()` returns the signed-in client with its token, for chunk 4.
  - `From<ApiError> for CoreError`, using `ApiError`'s own user-facing `Display` text, as desktop does.
- `ios/stoat-ffi/src/lib.rs`:
  - `mod session`.
  - `From<UnexpectedUniFFICallbackError> for CoreError`.
  - `init_logging()`: env_logger to stderr (shown in Xcode's console), default `stoat_gpui=info,stoat_ffi=info`.
- `ios/stoat-ffi/Cargo.toml`: added `anyhow`, `log` and `env_logger`, and dev-dependencies `wiremock`, `tempfile`, `tokio` and `serde_json`.
- `ios/stoat-ffi/tests/login.rs` (new, 11 tests): plain login (token, account list, active and session ID), MFA then TOTP with spaces, wrong password text, disabled account, blank fields refused, restore signs in, restore with a rejected token clears the vault, restore offline (a closed port) keeps the token, nothing saved, log out forgets everything, and the account file round-trip through `FileAccounts`. Fixtures come from `../../tests/fixtures/`.
- `scripts/ios/build-core.sh`: bindings are generated from a slice built in this run (`targets[0]`), not `slices[0]`. Before, a simulator-only build generated them from a stale device library.
- Swift:
  - `Session/KeychainVault.swift`: `TokenVault` over `SecItem*`, using `kSecAttrAccessibleAfterFirstUnlock` and service `com.chrishultin.rustoat.ios`. Saving again updates the existing item.
  - `AppModel.swift`: an `@MainActor @Observable` model with phases `restoring`, `offline(account, message)`, `signedOut` and `signedIn(account)`.
  - `Login/LoginView.swift`: a gradient background with a `.glassEffect` card, Server/Email/Password fields, and a 2FA step that switches between authenticator and recovery codes. It uses `.buttonStyle(.glassProminent)` and `.glass`.
  - `RootView.swift`: switches on the phase. The offline view (`ContentUnavailableView`) offers Retry and "Log in again". The You tab has a temporary Log Out until chunk 7.
  - `RuSToatApp.swift`: calls `initLogging()`, uses `AppModel.live()` (Application Support), and restores on launch.
- Swift tests:
  - `KeychainVaultTests`: round-trip under service `com.chrishultin.rustoat.ios.tests`.
  - `LiveInstanceTests`: opt-in with `TEST_RUNNER_RUSTOAT_LIVE=1`. A bogus login at api.stoat.chat must fail with "Incorrect email or password."
- `README.md`: a line on the live test.

### Divergences
- **No separate `ios/stoat-ffi/src/accounts.rs`.** The account-file path is `Core::new`'s `data_dir` argument, so the account handling lives in `session.rs`.
- **Session naming needed a small core change** (`StoatClient::with_session_name`). The friendly name was a constant.
- **Rejected sessions keep their account entry,** as desktop does. Only the token is cleared, and logging in again updates the same entry.
- **Bindgen bug fixed in `build-core.sh`** (described above).
- **An extra opt-in live Swift test.** It proved HTTPS from the iOS simulator works with reqwest's rustls platform verifier, and that error text comes through correctly.
- **A temporary Log Out button** in the You tab, so logging out can be checked before chunk 7 builds `YouView`.
- **Placeholders use `Text(verbatim:)`.** SwiftUI rendered "you@example.com" as a Markdown link.

### Verification Performed
- `cargo test -p stoat-ffi`: 2 unit tests and 11 login tests passed. `cargo clippy -p stoat-ffi --all-targets -- -D warnings`: clean.
- `scripts/ios/test.sh`: exit 0. `CoreBridgeTests` (3) and `KeychainVaultTests` (1) passed; `LiveInstanceTests` was skipped.
- `TEST_RUNNER_RUSTOAT_LIVE=1 xcodebuild … test`: 5/5, including the live login at api.stoat.chat (1.7 s).
- Desktop: `cargo test`: 1128 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- Screenshot of the login screen on the simulator: a glass card over the gradient.

### Manual Validation
Deferred to the end: log in with your real account and 2FA, relaunch and land signed in, then log out and check the session is gone from Stoat's Settings → Sessions.

## Chunk 4: Connect, and browse servers, channels and DMs

### Changes Made
- `src/live.rs` (new, in core, GPUI-free): `Changed { ready, lists, timelines }` with `merge`/`is_empty`, and `apply_event(&mut ChatStore, ServerEvent, now_ms) -> Changed`.
  - It covers every `ServerEvent` desktop's `on_connection_event` applies to the store: Ready, Message and its update/delete/append/bulk-delete/reactions, ChannelAck (own only), channel create/update/delete, group join/leave (leaving yourself removes the channel), emoji, member join/leave/update (your own leave removes the server), UserUpdate, server update/role/ranks/create/delete, UserRelationship, UserSettingsUpdate, slowmodes and voice states.
  - Typing and the other non-store events report nothing.
- `src/lib.rs`: `pub mod live`.
- `src/ui/chat.rs`: a comment on `on_connection_event` pointing at `live::apply_event`, and the reverse in `live.rs`, as the drift mitigation.
- `tests/live_dispatch.rs` (new, 14 tests): Ready fills lists, a message in an unviewed channel marks it unread, mentions increment, loaded timelines report changes for message/update/delete, others' acks are ignored, your own ack clears unread, ChannelDelete (and repeating it is a no-op), ServerUpdate renames, ServerDelete, leaving a server, UserUpdate renames a DM, leaving a group elsewhere, typing, and `Changed::merge`.
- `ios/stoat-ffi/src/controller.rs` (new):
  - `ControllerListener` callback interface (`changed(Change)`, `status(ConnectionStatus)`).
  - Records `Change`, `ServerItem`, `ChannelItem`, `ChannelSectionItem` and `Summary`; enums `ChannelKindItem` and `ConnectionStatus` (Connecting, Connected, Disconnected, SessionRejected, Paused).
  - `AccountController`:
    - Construction: `new(core, listener)`, or `for_client` in tests.
    - `connect()` spawns `events::connect` plus a drain task, and on first connect fetches `GET /` for `MediaUrls`. `disconnect()` drops both and reports `Paused`.
    - Snapshots: `servers`, `server`, `server_channels`, `home_channels`, `channel` and `summary`, with server icon URLs from `store.media()`.
    - The drain applies events under the store mutex (never held across an await or a Swift call) and coalesces bursts into one `changed` per 50 ms.
    - After each Ready it fetches the notification, ordering and folder settings, as desktop does. A second Ready (a reconnect) calls `clear_histories()`.
- `ios/stoat-ffi/src/session.rs`: `Core::forget_session()` for a rejected session, which clears the token without contacting Stoat.
- `ios/stoat-ffi/tests/support/mod.rs` (new): a fake Stoat. A local tokio-tungstenite events server sends Authenticated, then the fixture Ready, then pushed frames, and can drop every connection. A wiremock API's `GET /` points at that server. Also a `Recorder` listener and a `connected()` helper.
- `ios/stoat-ffi/tests/controller.rs` (new, 7 tests): connecting reports status and lists servers, channels and Home; server icons use the root's file server; a live message marks a channel unread; 20 messages give at most 2 change calls; disconnect/connect; a dropped connection reconnects; Logout reports SessionRejected.
- `ios/stoat-ffi/examples/fake_stoat.rs` (new): serves the fixtures on localhost for trying the app without an account, and posts a message in #random every 20 s.
- `ios/stoat-ffi/Cargo.toml`: added `futures` and `tokio` (time); dev-dependencies `tokio-tungstenite` and tokio `net`.
- Swift:
  - `Model/AccountModel.swift`: an `@Observable` model holding `status`, `servers`, `home`, `summary`, `listsVersion`, `timelineVersions` and `readyCount`. A listener bridge hops to the main actor.
  - `Home/HomeView.swift`, `Servers/ServerListView.swift`, `Servers/ServerChannelsView.swift` (sections by category).
  - `Shared/AvatarView.swift` (`AsyncImage` with an initials fallback, plus `UnreadBadge`), `Shared/ChannelRow.swift` (with `ChannelRoute`), `Shared/ConnectionBanner.swift` (a glass capsule).
  - `Chat/ChatView.swift`: a placeholder, replaced in chunk 5.
  - `AppModel.swift`:
    - Owns the `AccountModel`; a rejected session forgets the token and returns to login with a notice.
    - `setActive` connects or disconnects.
    - DEBUG-only `devLogin()` from `RUSTOAT_DEV_LOGIN`.
  - `RootView.swift`: tabs with mention badges and the banner overlay.
  - `RuSToatApp.swift`: the `scenePhase` `.active` phase connects and `.background` disconnects (`.inactive` keeps the connection).
  - `LoginView.swift`: shows a `notice`.
- `README.md`: a note on `fake_stoat` and the dev login.

### Divergences
- **`Changed` is a struct, not an enum,** so one event can report a list change and a timeline change together (a new message in a loaded channel does both).
- **The controller also fetches settings and `GET /`,** as desktop's `ChatView` does. Without the settings, mutes and server order would be missing; without `GET /`, icons would be missing.
- **`Core::forget_session` was added,** so the app can handle `SessionRejected`.
- **Tab badges and a `summary()` snapshot were added.** Small, but not in the plan.
- **The fake Stoat example and the DEBUG dev login were added** so the app can be checked in the simulator without your credentials. Neither ships in a Release build.
- **Settings ordering:** settings fetched after `Ready` can land after the first list change. They send their own change, so the lists correct themselves within moments.

### Verification Performed
- `cargo test --test live_dispatch`: 14 passed.
- `cargo test -p stoat-ffi`: 2 + 7 (controller) + 11 (login) passed. The controller tests ran 5 more times, all passing (~1.2 s each).
- `cargo clippy -p stoat-ffi --all-targets -- -D warnings`: clean.
- Desktop: `cargo test`: 1142 passed (1128 + 14), 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- `scripts/ios/test.sh`: exit 0, Swift build with no warnings.
- Simulator, against `fake_stoat` with the dev login: logged in, connected and received Ready (log: "2 servers, 11 channels…"). Home showed Saved Notes, bob, Weekend plans, Ada L. and Unknown user, with unread dots, and the glass tab bar.

### Manual Validation
Deferred to the end: compare servers and DMs with desktop, a live DM from desktop showing an unread dot, and background then return to see the reconnect banner.

## Chunk 5: Reading a channel

### Changes Made
- `ios/stoat-ffi/src/timeline.rs` (new):
  - `TimelineItem` (StartOfChannel, NewMessages, Message, Blocked) with `id()`, matching Swift's `Identifiable`.
  - `MessageItem`, carrying `key`, `render_key`, `continuation`, `mine`, `jumbo`, `edited`, `system`, `pending`, replies, attachments and embeds.
  - `PendingItem`, `ReplyItem`/`ReplyStateItem`, and compact `AttachmentItem` and `EmbedItem`.
  - Conversions from the store's `TimelineRow`/`MessageRow`/`AttachmentView`/`EmbedView`.
- `ios/stoat-ffi/src/controller.rs`:
  - `HISTORY_PAGE = 50`.
  - Shared state: `viewing` (the channel on screen), `dividers` (read position when opened), `reply_fetches`.
  - `Shared::load_latest` (`fetch_messages` → `set_history`), `Shared::load_older` (`before` the oldest → `prepend_page`; returns whether there's more), and `Shared::fetch_replies` (`fetch_messages_around(…, 1)` → `add_reply_sources`, or `reply_source_unavailable`; up to 5 at a time, as desktop).
  - On a reconnect's `Ready` (after `clear_histories`), the viewed channel is reloaded.
  - Exported: `open_channel` (sets viewing, records the divider, loads if not loaded), `load_older`, `close_channel`, `timeline` (with `with_unread_divider`), and `mark_read`.
  - `mark_read` only runs for the viewed channel, while connected and at the present. It acks locally with `apply_ack`, then sends `PUT …/ack/…`.
- `ios/stoat-ffi/tests/reading.rs` (new, 8 tests): the first page loads oldest-first (opening again doesn't refetch); older pages prepend until StartOfChannel; keys stay unique and stable across a prepend; a live edit changes `render_key` and `edited`, and a delete removes the row; `mark_read` acks only the viewed, connected channel, exactly once (`expect(1)`); a reconnect reloads the open channel; the divider sits after the read position and stays until reopened; reply originals are fetched.
- Swift:
  - `Chat/ChatView.swift`:
    - A `ScrollView`/`LazyVStack` with `.defaultScrollAnchor(.bottom, for: .initialOffset)` and `for: .sizeChanges`, which keeps the bottom fixed when older rows are inserted above.
    - A top sentinel loads older pages, with Retry on error.
    - `onScrollGeometryChange` tracks being at the bottom, and `markRead` is called only then, with the app active.
    - A reload on `timelineVersions`/`readyCount` changes, `closeChannel` on disappear, and `stoat:` links handled as a no-op.
    - `TimelineRowView` for StartOfChannel, the red "New messages" divider, and blocked runs. `TimelineItem: Identifiable` uses the message key.
  - `Chat/MessageRowView.swift`: avatar and header on group starts, replies, text (`.id(renderKey)`), "(edited)", centred system messages, a `.sending` row dimmed and a failed row's reason in red. `AttachmentCard` (thumbnail or file card, opening in Safari) and `EmbedCard`.
  - `Chat/MessageText.swift`: `AttributedString(markdown:)` with `.inlineOnlyPreservingWhitespace` and a partial-parse fallback.
  - `Chat/ReplyPreviewView.swift`.
  - `Home/HomeView.swift`: a navigation path, plus a DEBUG-only `RUSTOAT_DEV_OPEN` to open a channel at launch.
- `ios/RuSToatTests/MessageTextTests.swift` (5 tests): bold and italics, code, links (http and `stoat:`), line breaks, and the fallback.
- `README.md`: a note on `RUSTOAT_DEV_OPEN`.

### Divergences
- **Scroll position on prepend** uses `.defaultScrollAnchor(.bottom, for: .sizeChanges)`, not `scrollPosition(id:)` pinning. With bottom anchoring, rows inserted above don't move what's on screen, and new messages keep the list at the bottom. If a jump shows on a real device, the fallback is `scrollPosition(id:)`.
- **Reply originals are fetched**, as on desktop. Otherwise replies would say "Loading…" forever.
- **Reloading after a reconnect happens in Rust,** not Swift, so it's covered by a Rust test.
- **The divider is fixed when the channel opens** and kept until it closes, as desktop does.
- **Reactions are left out of `MessageItem`** (iOS milestone 2).
- **A DEBUG-only `RUSTOAT_DEV_OPEN`** was added for simulator screenshots.

### Verification Performed
- `cargo test -p stoat-ffi --test reading`: 8 passed, three runs in a row. The full `stoat-ffi` suite: 2 + 7 + 11 + 8.
- `cargo clippy -p stoat-ffi --all-targets -- -D warnings`: clean. `TimelineItem` has the store's `large_enum_variant` allowance.
- Swift: 10 tests in 4 suites passed, with no warnings after fixes: the older-error text is now shown, and `MessageText.attributed` is `nonisolated`.
- Desktop: `cargo test`: 1142 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- Simulator against `fake_stoat`, opening #general: start-of-channel header, grouped messages, `@bob`/`#random` mentions as links, a continuation without a header, the centred "Carol joined" system message, "(edited)", an image placeholder, and masquerade and webhook names.

### Manual Validation
Deferred to the end: scroll back several pages in a busy channel with no jumps or duplicates; edit and delete from desktop; check the unread dot clears on desktop after reading on the phone.

## Chunk 6: Sending messages

### Changes Made
- `src/api/error.rs`: added `ApiError::is_duplicate_nonce()` and `ApiError::send_failure_text()`, moved from private functions in `src/ui/chat.rs` (`is_duplicate`, `send_error_text`) so both apps share them. `chat.rs` now calls the methods. Desktop behaviour is unchanged.
- `src/permissions.rs`: added `can_send(store, user, channel)` (SendMessage). `src/store.rs`: added `ChatStore::can_send(channel)`. `tests/permissions.rs`: added `sending_messages_follows_its_permission` (server channel with SendMessage denied, owner, friend DM, saved notes, blocked DM, a view-only group).
- `ios/stoat-ffi/src/controller.rs`:
  - `Shared::post`, as desktop's `post_pending` without attachments. Success applies the message (merging with the pending row by nonce) and acks it, as desktop's `ack_sent`. `DuplicateNonce` discards the pending message. Other errors call `fail_pending` with `send_failure_text`; `answered` is false only for `Network`.
  - Exported: `can_send`, `send(channel, text, reply_to)` (adds the pending message, clears the divider, spawns the post, returns the key), `retry`, `discard`, and `typing(channel, empty)` (desktop's `OutgoingTyping` throttle over the connection's `begin_typing`/`end_typing`, with an idle timer).
  - `Running` keeps the `Connection` for sending typing frames.
- `ios/stoat-ffi/tests/sending.rs` (new, 7 tests):
  - A send is pending immediately, keyed by its first idempotency key. Then the live echo arrives before a delayed POST answer, leaving exactly one row with the same key and nothing pending.
  - A 500 fails the row with its reason, and retry sends with a fresh key (Stoat answered, so the old key is spent).
  - A dropped connection (a raw TCP API that closes without answering), then retry, sends the same key again.
  - Replies are sent and shown in the pending row; discard; empty text is refused; `can_send`.
- `ios/stoat-ffi/tests/support/mod.rs`: `Events::sender()` for pushing frames from mock responders.
- `ios/stoat-ffi/examples/fake_stoat.rs`: answers sends (with the nonce) and echoes them on the socket, and accepts acks.
- Swift:
  - `Chat/Composer.swift`: a `GlassEffectContainer` holding a vertical `TextField` (1–6 lines, `.glassEffect(.regular.interactive())`), a circular `.glassProminent` send button, and a glass "Replying to …" chip. `CantSendBar` shows instead of the composer where sending isn't allowed.
  - `Chat/ChatView.swift`:
    - `.safeAreaBar(edge: .bottom)` with the composer, and `.scrollPosition` so a send scrolls to the bottom.
    - Drafts per channel in `AccountModel.drafts` (in memory), typing reported on each change, and the placeholder "Message #name", "Message Name" or "Save a note".
    - `.toolbar(.hidden, for: .tabBar)` in conversations.
  - `Chat/MessageRowView.swift`: `MessageActions`; a context menu with Reply, Copy Text, and Retry/Discard on failed rows; swipe right to reply; a failed row's red "… Tap to retry." button. Pending rows stay dimmed, as in chunk 5.

### Divergences
- **Retry after an answered failure uses a fresh idempotency key.** The plan said retry re-POSTs with the same key. That's only safe when Stoat may not have seen the request (a network failure), and the tests cover both cases. This is `ChatStore::retry_pending`'s existing behaviour, shared with desktop.
- **Send-error text and the duplicate check moved onto `ApiError`,** a small desktop refactor, so iOS shows the same text without copying it.
- **`can_send` is new in core.** Desktop had no such check; it relied on Stoat refusing the send.
- **Slowmode cooldowns aren't started or shown on iOS yet.** A send during slowmode fails with Stoat's message and can be retried.
- **The tab bar is hidden in conversations.**

### Verification Performed
- `cargo test -p stoat-ffi --test sending`: 7 passed, three runs in a row. The full `stoat-ffi` suite: 2 + 7 + 11 + 8 + 7.
- `cargo test --test permissions`: 50 passed, including the new test.
- Desktop: `cargo test`: 1143 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- `scripts/ios/test.sh`: exit 0, Swift 10/10, no warnings.
- Simulator against `fake_stoat`, opening Ada's DM: the floating glass composer with "Message Ada L." and the send button, over the message list.

### Manual Validation
Deferred to the end: send messages and replies from the phone and see them on desktop; in Airplane mode, send, see the row fail, then retry once back online.

## Chunk 7: On your phone

### Changes Made
- `examples/render_icon.rs`: a `--ios <icon.svg> <AppIcon.png>` mode that renders the tile alone at 1024 px, square and edge to edge (`ios_tile` sets `rx` to 0 and the viewBox to the tile). It fails loudly if the SVG's tile or viewBox changes.
- `scripts/ios/make-icon.sh` (new): regenerates `ios/RuSToat/Assets.xcassets/AppIcon.appiconset/AppIcon.png`, which is committed with its `Contents.json` (a single 1024 universal iOS icon).
- `ios/stoat-ffi/src/controller.rs`: a `MeItem` record and `AccountController::me()` (name, `username#1234`, avatar URL, presence, status text). `tests/controller.rs`: `me_is_the_signed_in_user`.
- `ios/project.yml`: `DEVELOPMENT_TEAM: 87ATGCDMGT`, `CODE_SIGN_STYLE: Automatic`, portrait only (`INFOPLIST_KEY_UISupportedInterfaceOrientations`), `ASSETCATALOG_COMPILER_APPICON_NAME: AppIcon`. The launch screen came in chunk 2.
- Swift:
  - `You/YouView.swift` (new): avatar, name, username and status, the server address and connection state, Log Out with a confirmation dialog, and the app and core versions.
  - `Model/AccountModel.swift`: adds `me`.
  - `RootView.swift`: the tab is remembered with `@SceneStorage("tab")`, and the You tab uses `YouView`.
  - `Home/HomeView.swift`: `@SceneStorage("homeChannel")` reopens the last conversation after the first Ready. The DEBUG `RUSTOAT_DEV_OPEN` overrides it.
  - `Servers/ServerListView.swift`: a typed `ServersRoute` path (server, then channel), remembered as `serverPlace` (`serverID/channelID`) and restored after the first Ready. `ServerChannelsView` links push `ServersRoute.channel`.
- `README.md`: a "Running on your iPhone" section (the Xcode account, Developer Mode, Run, trusting the certificate, the command line, the 7-day free-team note, the icon, the session name).
- `docs/plans/roadmap.md`: a pointer to this plan's iOS roadmap.

### Divergences
- **The signed device build couldn't run here.** `xcodebuild … -destination 'generic/platform=iOS' -allowProvisioningUpdates build` failed with "No Account for Team 87ATGCDMGT". Xcode needs your Apple ID under Settings → Accounts, which only you can add. With `CODE_SIGNING_ALLOWED=NO` the device build compiles and links: `BUILD SUCCEEDED`, a 35 MB Debug app, and both XCFramework slices carry the current symbols.
- **The icon needed a new `--ios` render mode.** The macOS SVG draws Apple's rounded tile with transparent margins, but iOS wants a full-bleed square. The PNG keeps an alpha channel (fully opaque); that's fine for development installs, but App Store validation would reject it.
- **Restoring places uses `@SceneStorage`, not `places.rs`.** It's per device, and enough for one account.

### Verification Performed
- `scripts/ios/make-icon.sh`: a 1024×1024 full-bleed render, checked visually.
- `cargo test -p stoat-ffi`: 2 + 8 (controller) + 11 + 8 + 7 passed. Clippy clean.
- Device build without signing: succeeded, with the "Build Rust core" phase building the `aarch64-apple-ios` slice. Signed build: blocked on your Xcode account (above).
- `scripts/ios/test.sh`: exit 0, Swift 10/10, no warnings.
- Desktop: `cargo test`: 1143 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
Pending. You batched every manual check to the end; they're listed under Remaining Manual Steps below.

## Summary

_Completed: 2026-11-13 (code). Manual checks and the signed device install are pending._

### Chunks Completed
1. The library builds without GPUI for iOS (`desktop` feature, workspace, iOS TLS roots).
2. The `stoat-ffi` UniFFI crate, `build-core.sh` → `StoatCore.xcframework`, the XcodeGen project, and a glass tab-bar shell with bridge tests.
3. Log in, 2FA, restore and log out, with tokens in the iOS Keychain.
4. `src/live.rs` shared event dispatch, `AccountController`, and the Home/Servers/channel lists with a live connection banner.
5. Reading channels: paging, reply originals, the divider, mark-read while on screen, and a reload after reconnecting.
6. Sending: pending, confirmed or failed rows, retry and discard, replies, typing, `can_send`, and the glass composer.
7. The icon, the You tab, signing and portrait settings, remembering the tab and channel, and the README.

### Divergences
- Chunk 1: the workspace started as `["."]`, and `ios/stoat-ffi` was added in chunk 2.
- Chunk 2: `build-core.sh` re-executes itself under a clean environment; `test.sh` picks the simulator by UDID and boots it first.
- Chunk 3: no separate `accounts.rs`; `StoatClient::with_session_name` was added; an opt-in live HTTPS test; a bindgen slice bug was fixed.
- Chunk 4: `Changed` is a struct; the controller also fetches settings and `GET /`; `Core::forget_session`; tab badges; the `fake_stoat` example and DEBUG dev login.
- Chunk 5: bottom scroll anchoring instead of `scrollPosition(id:)` pinning; reply originals are fetched; the reload after reconnecting is done in Rust.
- Chunk 6: retry uses a fresh key after an answered failure (as desktop); send-error text and the duplicate check moved onto `ApiError`; `can_send` added to core; no slowmode countdown yet; the tab bar is hidden in chats.
- Chunk 7: the signed device build needs your Xcode account; an `--ios` icon mode; `@SceneStorage` for places.

### Definition of Done
- [x] `cargo test && cargo clippy --all-targets -- -D warnings` passes with default features (1143 tests). Desktop behaving as before: **pending your `cargo run` check**.
- [x] `cargo check --lib --no-default-features --target aarch64-apple-ios` succeeds, and `gpui-kit`, `livekit` and `keyring` are absent from that `cargo tree`.
- [x] `scripts/ios/build-core.sh` produces `ios/build/StoatCore.xcframework` with device and simulator slices, and `xcodebuild … test` passes on a simulator.
- [ ] Logging in with email, password and TOTP works against `api.stoat.chat`: **pending you**. Covered by wiremock tests and the live bogus-login HTTPS test. The token goes only to the Keychain under `{API URL}#{user ID}`; `accounts.json` holds no token.
- [ ] Relaunching restores the session; a rejected token returns to login; an unreachable server shows Retry: covered by Rust tests, **pending you on the real instance**.
- [ ] Home and Servers lists with live unread state: covered by tests and the simulator against `fake_stoat`, **pending you on real data**.
- [ ] A channel shows its history, pages back to the start, applies live edits and deletes, and acks on screen: covered by tests, **pending you**.
- [x] Sent messages show as pending, then sent, or failed with retry, and never twice (tests). The Airplane-mode check is **pending you**.
- [x] The tab bar, toolbars, composer and banner use system Liquid Glass (`TabView`, `.glassEffect`, `GlassEffectContainer`, `.buttonStyle(.glass/.glassProminent)`), with no custom blur. Light and dark mode are **pending you**.
- [ ] The app installs and runs on your iPhone from Xcode: **pending your Xcode account**. The unsigned device build succeeds.
- [x] The README documents building for the simulator and installing on a device.

### Remaining Manual Steps
1. Add your Apple ID in Xcode → Settings → Accounts, then run on your iPhone (README "Running on your iPhone").
2. `cargo run`: the desktop app still launches and connects.
3. Log in with 2FA on the phone, relaunch and land signed in, then later log out and check Stoat's Settings → Sessions.
4. Compare servers and DMs with desktop. Send yourself a DM from desktop and watch the unread dot appear live.
5. Read a long channel back several pages: no jumps or duplicates. Edit and delete from desktop and watch the phone update.
6. Read on the phone, then check the unread dot clears on desktop.
7. Send messages and replies. In Airplane mode, send, see the row fail, turn Airplane mode off, tap retry, and check it sends once.
8. Background the app for 5+ minutes and return: it reconnects, and the open channel reloads.
9. Check light and dark mode, and that the tab bar shrinks while scrolling.
10. Use it for a day on cellular, locking and unlocking and switching apps.

## Merging main, and signing for the device

_2026-11-13, after the summary above._

`main` had moved 47 commits ahead (milestones 22–31: webhooks, roles, channel settings, custom emoji, reports, audio and video). Merging it into this branch needed:
- **Conflicts:** `src/lib.rs` (new modules next to the `desktop` gates) and `tests/permissions.rs` (import lists from both sides).
- **`src/roles.rs` was GPUI-free except for colours.** `ColourChoice::of` now uses a new `rail::hex_colour` (lower-case `#rrggbb` from `#rgb`/`#rrggbb`), sharing `rail::hex_value` with `parse_colour`. `hex_of`, `rgba_hex` and the `gpui_kit` import are behind `desktop`. A new unit test covers `hex_colour`.
- **`src/live.rs`** handles `main`'s new events as desktop does: `EmojiUpdate` (`rename_emoji`), and `WebhookCreate`/`Update`/`Delete` (`add_webhook`, `apply_webhook_update`, `remove_webhook`).
- **`src/media_player.rs`:** `END_SLACK` and `at_end` are allowed as dead code off macOS. Only the macOS player uses them, and the iOS build's `-D warnings` caught it.
- **`ios/stoat-ffi/src/timeline.rs`:** `main`'s new `AttachmentView::Audio` and `Video` show as file cards that open in Safari. Inline media is iOS milestone 3.

Signing: the plan's team ID was wrong. `87ATGCDMGT` is in the certificate's name, but its `OU`, and the team Xcode lists (a paid individual team, not a free one), is **`G55L4BQ44C`**. `ios/project.yml`, the README and the plan's assumption 2 now say so.

Verification after the merge:
- `cargo test`: 1499 passed, 0 failed, 5 ignored (91 binaries). `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo clippy --lib --no-default-features -- -D warnings`, on the host and for `aarch64-apple-ios`: clean. No `gpui-kit`, `livekit` or `keyring` for iOS.
- `scripts/ios/test.sh`: exit 0.
- `xcodebuild … -destination 'generic/platform=iOS' -allowProvisioningUpdates build`: **BUILD SUCCEEDED**, signed by "Apple Development: Christopher Hultin (87ATGCDMGT)", TeamIdentifier G55L4BQ44C. The Definition of Done's "installs from Xcode" now waits only on you pressing Run with the phone selected.
