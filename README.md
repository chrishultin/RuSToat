# RuSToat

A proof-of-concept native desktop client for [Stoat](https://stoat.chat) (formerly Revolt), written in Rust with [GPUI](https://gpui.rs) via [`gpui-kit`](https://gpui-kit.com).

The goal is to find out whether GPUI is a good foundation for a Stoat client. The POC covers logging in (including 2FA), browsing servers, channels and DMs, and reading text chats with live updates. The plan and progress are in [`docs/plans/`](docs/plans/).

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

### Self-hosted instances

The app talks to the official instance, `https://api.stoat.chat`, by default. Point it somewhere else with:

```bash
STOAT_API_URL=https://stoat.example.com/api cargo run
```

Saved sessions are stored per API URL, so switching instances never sends one server's token to another.

## Your session and the Keychain

After you log in, the session token is saved in your login Keychain under the service `chat.stoat.gpui-poc`, with the API URL as the account. The next launch signs you in automatically.

- **Log Out** (in the Stoat menu, or the button in the window) removes the Keychain item and ends the session on Stoat. If Stoat can't be reached, you're still logged out locally; revoke the session from Stoat's Settings → Sessions, where it's listed as "Stoat GPUI POC".
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

The tests use a mock Stoat API (`wiremock`) and an in-memory token store, so they need no network and never touch your Keychain. There are two opt-in checks:

```bash
cargo test --test api_login -- --ignored   # live API: is the server still on the pinned 0.16.x?
cargo test --lib -- --ignored              # real Keychain: writes and deletes a throwaway test item
```
