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
