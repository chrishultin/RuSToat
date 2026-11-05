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

The login screen has a **Server** field, set to the official instance (`https://api.stoat.chat`) by default. To use a self-hosted instance, enter its address, e.g. `stoat.example.com`:

- `https://` is added if you leave it out. Type `http://` yourself for a local server.
- The app looks for Stoat's API at that address, then under `/api`, where self-hosted instances usually serve it. Once you log in, the field shows the address it found.
- If the API is somewhere else, such as its own subdomain, enter that address instead.

To change the field's default:

```bash
STOAT_API_URL=https://stoat.example.com/api cargo run
```

Each saved session remembers its own instance, so a token is never sent to another server. While there's a saved account, the app restores that one.

### Multiple accounts

You can be signed in to several accounts at once, on one instance or several. They appear in a column on the far left, as Slack's workspaces do:

- **+** (or **Add Account…** in the Stoat menu) logs in to another account. Each account remembers its own instance.
- Click an account to switch to it. Every account stays connected, so unreads, notifications and the Dock badge cover all of them. Clicking a notification for another account switches to it.
- An account you aren't looking at never marks anything as read.
- **Log Out** (in the Stoat menu, or the button in the window) applies to the account on screen. Right-click an account to log out of it from the switcher.
- If Stoat ends an account's session, the account stays in the switcher, dimmed. Select it to log in again.

## Your session and the Keychain

After you log in, the session token is saved in your login Keychain under the service `chat.stoat.gpui-poc`, with `{API URL}#{user ID}` as the account. The next launch signs you in automatically. A session saved by an older version (whose Keychain account is just the API URL) is picked up as it is.

The list of accounts is kept, without tokens, in `~/Library/Application Support/chat.stoat.rustoat/accounts.json`.

- **Log Out** (in the Stoat menu, or the button in the window) removes the account, deletes its Keychain item and ends the session on Stoat. If Stoat can't be reached, you're still logged out locally; revoke the session from Stoat's Settings → Sessions, where it's listed as "Stoat GPUI POC".
- **Expect a Keychain prompt during development.** macOS ties Keychain access to the app's code signature, and every `cargo build` produces a new unsigned binary. After a rebuild, macOS may ask whether `stoat-gpui` can use the saved item. Choose **Always Allow**. This is expected and won't happen with a signed release build.
- **Each account has its own Keychain item**, so during development you may get one prompt per account after a rebuild.
- If your saved session can't be checked at startup (offline, or a Stoat outage), the app keeps it and offers **Retry**. It only discards a saved session that Stoat itself rejects.

The app also remembers where you were: the open server, and the last channel you viewed in each server and in Home. Switching back to a server returns you to its channel, and the next launch opens where you left off. This is saved in `~/Library/Application Support/chat.stoat.rustoat/places.json`, per instance and account. Deleting the file just starts you at Home.

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
