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
