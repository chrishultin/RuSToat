# RuSToat

A proof-of-concept native desktop client for [Stoat](https://stoat.chat) (formerly Revolt), written in Rust with [GPUI](https://gpui.rs) via [`gpui-kit`](https://gpui-kit.com).

The goal is to find out whether GPUI is a good foundation for a Stoat client. The POC covers:

- logging in, including 2FA
- browsing servers (with their icons), channels and DMs
- reading text chats: live new, edited and deleted messages, and older history that loads as you scroll up

The plan and progress are in [`docs/plans/`](docs/plans/).

## Running

Requirements: macOS with Xcode (GPUI compiles Metal shaders with `xcrun metal`) and [rustup](https://rustup.rs). The toolchain is pinned in `rust-toolchain.toml`; rustup installs it automatically.

```bash
cargo run
```

The first build compiles GPUI and takes several minutes.

For more logging:

```bash
RUST_LOG=stoat_gpui=debug cargo run
```

### Notifications: running as an app bundle

New messages notify according to your Stoat notification settings. The Dock icon shows a badge for unread DMs and mentions. A plain `cargo run` shows notices as in-app toasts only, because macOS only lets an app post notification banners when it runs from an app bundle. To get banners, run it bundled:

```bash
scripts/run-bundled.sh
```

This builds `target/debug/RuSToat.app` (identifier `chat.stoat.rustoat`, ad-hoc signed) and runs it with its logs in your terminal. Pass `release` to bundle a release build. `scripts/bundle.sh` only builds the bundle and prints its path.

- **The first bundled run asks for permission to send notifications.** Choose **Allow**. If you don't, RuSToat logs `banners: turned off…` and falls back to in-app toasts. You can change the setting later in System Settings → Notifications → RuSToat.
- **Banners only appear while RuSToat is in the background.** While its window is in front, you get toasts instead. Clicking a banner brings RuSToat forward on that channel, and reading a channel, here or in another Stoat client, removes its banner from Notification Center.
- **Expect a Keychain prompt on the first bundled run too.** The bundle has a different code identity from the `cargo run` binary, so it gets the same prompt.

### Self-hosted instances

The app talks to the official instance, `https://api.stoat.chat`, by default. Point it somewhere else with:

```bash
STOAT_API_URL=https://stoat.example.com/api cargo run
```

Each saved session remembers its own instance, so a token is never sent to another server. While there's a saved account, the app restores that one; `STOAT_API_URL` only picks the instance you log in to.

## Your session and the Keychain

After you log in, the session token is saved in your login Keychain under the service `chat.stoat.gpui-poc`, with `{API URL}#{user ID}` as the account. The next launch signs you in automatically. A session saved by an older version (whose Keychain account is just the API URL) is picked up as it is.

The list of accounts is kept, without tokens, in `~/Library/Application Support/chat.stoat.rustoat/accounts.json`.

- **Log Out** (in the Stoat menu, or the button in the window) removes the account, deletes its Keychain item and ends the session on Stoat. If Stoat can't be reached, you're still logged out locally; revoke the session from Stoat's Settings → Sessions, where it's listed as "Stoat GPUI POC".
- **Expect a Keychain prompt during development.** macOS ties Keychain access to the app's code signature, and every `cargo build` produces a new unsigned binary. After a rebuild, macOS may ask whether `stoat-gpui` can use the saved item. Choose **Always Allow**. This is expected and won't happen with a signed release build.
- If your saved session can't be checked at startup (offline, or a Stoat outage), the app keeps it and offers **Retry**. It only discards a saved session that Stoat itself rejects.

## Built on Stoat's own crates

API types come straight from the Stoat backend's published crates instead of being re-declared:

- [`revolt-models`](https://crates.io/crates/revolt-models) (MIT): users, servers, channels, messages, login and 2FA payloads
- [`revolt-result`](https://crates.io/crates/revolt-result) (MIT): API error types

They're pinned to the live API's version (`=0.16.2`), so bump all `revolt-*` crates together. The AGPL-licensed `revolt-database` crate, which holds the server's event enum, is deliberately not used. The client defines its own small event enum and uses `revolt-models` types for every payload.

## Tests

```bash
cargo test && cargo clippy --all-targets -- -D warnings
```

The tests use a mock Stoat API (`wiremock`) and an in-memory token store, so they need no network and never touch your Keychain. There are three opt-in checks:

```bash
cargo test --test api_login -- --ignored    # live API: is the server still on the pinned 0.16.x?
cargo test --test live_events -- --ignored  # live events service: does the websocket handshake succeed?
cargo test --lib -- --ignored               # real Keychain: writes and deletes a throwaway test item
```
