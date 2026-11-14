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

### Code signing

`cargo run` and the app bundle are both signed as `com.chrishultin.rustoat` with your **Apple Development** certificate, picked automatically from your keychain (`scripts/sign.sh`; `cargo run` goes through `scripts/cargo-runner.sh`, set in `.cargo/config.toml`). A stable signature is what lets the Keychain remember your "Always Allow" across rebuilds. Test binaries aren't signed.

- **Choose another identity** with `RUSTOAT_SIGN_IDENTITY="Apple Development: …"` (any name or hash from `security find-identity -v -p codesigning`).
- **Without an Apple Development certificate,** the build is signed ad hoc with a warning. It still runs, but the Keychain asks again after every rebuild.
- **The first signing may ask for your private key** ("codesign wants to access key…"). Choose **Always Allow**.

For more logging:

```bash
RUST_LOG=stoat_gpui=debug cargo run
```

### Notifications: running as an app bundle

New messages notify according to your Stoat notification settings. The Dock icon shows a badge for unread DMs and mentions. Closing the window disconnects every account and clears the badge, but RuSToat keeps running: click its Dock icon to open the window again and reconnect. A plain `cargo run` shows notices as in-app toasts only, because macOS only lets an app post notification banners when it runs from an app bundle. To get banners, run it bundled:

```bash
scripts/run-bundled.sh
```

This builds `target/debug/RuSToat.app` (identifier `com.chrishultin.rustoat`, signed as above) and runs it with its logs in your terminal. Pass `release` to bundle a release build. `scripts/bundle.sh` only builds the bundle and prints its path.

- **The first bundled run asks for permission to send notifications.** Choose **Allow**. If you don't, RuSToat logs `banners: turned off…` and falls back to in-app toasts. You can change the setting later in System Settings → Notifications → RuSToat.
- **Banners only appear while RuSToat is in the background.** While its window is in front, you get toasts instead. Clicking a banner brings RuSToat forward on that channel, and reading a channel, here or in another Stoat client, removes its banner from Notification Center.
- **The bundle and `cargo run` share one code identity,** so a Keychain "Always Allow" given to one covers the other.
- **The bundle has RuSToat's own icon,** a cyborg take on Stoat's stoat (`assets/icon/rustoat.svg`). After editing the SVG, run `scripts/make-icon.sh` to regenerate `assets/icon/RuSToat.icns`. If the Dock keeps showing an old icon, run `killall Dock`. The icon derives from Stoat's logo: fine for personal use, but check Stoat's brand terms before distributing it.

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

After you log in, the session token is saved in your login Keychain under the service `com.chrishultin.rustoat`, with `{API URL}#{user ID}` as the account. The next launch signs you in automatically. A session saved by an older version (whose Keychain account is just the API URL) is picked up as it is.

The list of accounts is kept, without tokens, in `~/Library/Application Support/com.chrishultin.rustoat/accounts.json`.

- **Log Out** (in the Stoat menu, or the button in the window) removes the account, deletes its Keychain item and ends the session on Stoat. If Stoat can't be reached, you're still logged out locally; revoke the session from Stoat's Settings → Sessions, where it's listed as "RuSToat" (sessions from before the rename keep the name "Stoat GPUI POC").
- **One Keychain prompt per account, then none.** macOS ties Keychain access to the app's code signature. The first run asks whether RuSToat can use each account's item; choose **Always Allow**. Because builds are signed with a stable identity (see [Code signing](#code-signing)), rebuilds don't ask again.
- If your saved session can't be checked at startup (offline, or a Stoat outage), the app keeps it and offers **Retry**. It only discards a saved session that Stoat itself rejects.

The app also remembers where you were: the open server, and the last channel you viewed in each server and in Home. Switching back to a server returns you to its channel, and the next launch opens where you left off. This is saved in `~/Library/Application Support/com.chrishultin.rustoat/places.json`, per instance and account. Deleting the file just starts you at Home.

### Moving over from the old names

RuSToat used to run as `chat.stoat.rustoat` with the Keychain service `chat.stoat.gpui-poc`. The first launch under the new identifier moves everything over:

- **Your saved session** is copied to the new Keychain service and the old item is deleted. macOS asks once more for the old item, since an older, differently signed build created it. Choose **Allow**.
- **The Application Support folder** `chat.stoat.rustoat` is renamed to `com.chrishultin.rustoat`, unless the new one already exists.
- **macOS asks for notification permission again,** once, for the new bundle identifier. The old "RuSToat" entry in System Settings → Notifications can be removed.

## Building for iPhone

`ios/` holds a SwiftUI app for iPhone (iOS 26 or later) on the same Rust core: `ios/stoat-ffi` exposes the library, built without its `desktop` feature, to Swift through [UniFFI](https://mozilla.github.io/uniffi-rs/). The plan is in [`docs/plans/ios-app.md`](docs/plans/ios-app.md).

Requirements: Xcode 26 or later with an iOS simulator, rustup (it adds the iOS targets from `rust-toolchain.toml`), and [XcodeGen](https://github.com/yonaskolb/XcodeGen):

```bash
brew install xcodegen
(cd ios && xcodegen generate)      # ios/project.yml → ios/RuSToat.xcodeproj
open ios/RuSToat.xcodeproj
```

Then pick an iPhone simulator (or your iPhone) and press Run. The first build compiles the Rust core in release mode, which takes a few minutes. `scripts/ios/build-core.sh` does the same from a terminal (`ios/build/StoatCore.xcframework` plus the Swift bindings), for both the device and the simulator.

- **Xcode rebuilds the core itself** before each build (a script phase in `project.yml`), for only the slice it needs. Cargo's change tracking keeps this to under a second when no Rust changed. The app reads the core from the XCFramework's slices through search paths rather than as a linked framework, because Xcode copies linked frameworks before any script runs, so a change to the core's API would need a second build.
- **`RuSToat.xcodeproj`, `ios/build/` and `ios/RuSToat/Generated/` aren't committed.** After pulling changes to `project.yml` or adding Swift files, run `xcodegen generate` again.
- **Trying it without a real account:** `cargo run -p stoat-ffi --example fake_stoat` serves the test fixtures (REST and the events websocket) on localhost and prints the address. Log in there with any email and password, or have a Debug build log in at launch: `SIMCTL_CHILD_RUSTOAT_DEV_LOGIN=http://127.0.0.1:PORT xcrun simctl launch booted com.chrishultin.rustoat.ios`. Add `SIMCTL_CHILD_RUSTOAT_DEV_OPEN=ch-general` to open a channel straight away.
- **Bindings always match the library.** They're generated from the built library by a `uniffi-bindgen` binary in `stoat-ffi` itself, from the same pinned UniFFI version.

### Running on your iPhone

The app installs from Xcode onto your own iPhone (iOS 26 or later), signed by Xcode for your team (`G55L4BQ44C`, set in `ios/project.yml`; it's the `OU` of your Apple Development certificate, not the ID in its name). There's no TestFlight.

1. **Sign in to Xcode with your Apple ID:** Xcode → Settings → Accounts → **+**. Your team should appear under it. Signing only works once it does.
2. **Turn on Developer Mode on the phone:** connect it by cable (or have it on the same Wi-Fi after pairing once), then on the phone go to Settings → Privacy & Security → Developer Mode. Turn it on and restart when asked. The option only appears after the phone has been connected to Xcode once.
3. **Build and run:** in `ios/RuSToat.xcodeproj`, pick your iPhone as the run destination and press Run. The first device build compiles the core for `aarch64-apple-ios`, which takes a few minutes.
4. **Trust the developer the first time:** if the phone says "Untrusted Developer", go to Settings → General → VPN & Device Management, choose your Apple Development certificate, and trust it.

From the command line, `xcodebuild -project ios/RuSToat.xcodeproj -scheme RuSToat -destination 'generic/platform=iOS' -allowProvisioningUpdates build` builds and signs the same thing, once Xcode has your account.

- **If your team were a free personal team,** builds would expire after 7 days. Yours is a paid individual team, so they last as long as the provisioning profile (a year).
- **The app icon** is `assets/icon/rustoat.svg`, squared off for iOS. Regenerate it after editing the SVG with `scripts/ios/make-icon.sh`.
- **iOS sessions are named "RuSToat (iPhone)"** in Stoat's Settings → Sessions, and the token is kept in the iPhone's Keychain.

## Releasing

```bash
scripts/release.sh
```

This builds a release `RuSToat.app`, signs it with the hardened runtime and a secure timestamp, and packs it with a link to `/Applications` into `dist/RuSToat-<version>.dmg` (compressed, signed and checked with `hdiutil verify`). Each build's `CFBundleVersion` is `<version>.<commit count>`. The build is for this Mac's architecture only (Apple silicon here).

- **Signing:** the script uses a **Developer ID Application** certificate if your keychain has one, else **Apple Development**. `RUSTOAT_SIGN_IDENTITY` overrides the choice.
- **Without Developer ID** (the current state), the DMG runs on this Mac and others registered to your team, but Gatekeeper rejects it elsewhere (`spctl` says `rejected`). That's fine for trying out installs locally.
- **Notarization** runs only with a Developer ID certificate and a stored `notarytool` profile. To set it up:
  1. The team's Account Holder creates a "Developer ID Application" certificate at developer.apple.com → Certificates, and installs it in your keychain.
  2. Store notary credentials once (an app-specific password from appleid.apple.com): `xcrun notarytool store-credentials rustoat-notary --apple-id <you@example.com> --team-id <team ID>`.
  3. Release with `RUSTOAT_NOTARY_PROFILE=rustoat-notary scripts/release.sh`. The script submits the DMG, waits, and staples the ticket.
- **`scripts/release.sh --dry-run`** builds nothing. It prints the identity, the DMG path and every command it would run, including the notarization steps, or says why it would skip them.
- **Entitlements:** release builds use `scripts/entitlements.plist`, which is deliberately empty. The network, the Keychain and notifications need no hardened-runtime exceptions.

## Built on Stoat's own crates

API types come straight from the Stoat backend's published crates instead of being re-declared:

- [`revolt-models`](https://crates.io/crates/revolt-models) (MIT): users, servers, channels, messages, login and 2FA payloads
- [`revolt-result`](https://crates.io/crates/revolt-result) (MIT): API error types

They're pinned to the live API's version (`=0.16.2`), so bump all `revolt-*` crates together. The AGPL-licensed `revolt-database` crate, which holds the server's event enum, is deliberately not used. The client defines its own small event enum and uses `revolt-models` types for every payload.

## Tests

```bash
cargo test && cargo clippy --all-targets -- -D warnings
```

The library's GPUI views, voice and macOS integrations sit behind the `desktop` feature, which is on by default. To check that the rest still builds without them, as the iOS app needs:

```bash
cargo clippy --lib --no-default-features -- -D warnings
cargo check --lib --no-default-features --target aarch64-apple-ios
```

`scripts/ios/test.sh` runs those, the `stoat-ffi` tests and the iPhone app's Swift tests on a simulator (the first available iPhone, or `IOS_SIMULATOR="iPhone 17"`).

`TEST_RUNNER_RUSTOAT_LIVE=1 xcodebuild … test` also runs `LiveInstanceTests`, which checks HTTPS from iOS with a deliberately wrong login at `api.stoat.chat`.

The tests use a mock Stoat API (`wiremock`) and an in-memory token store, so they need no network and never touch your Keychain. There are three opt-in checks:

```bash
cargo test --test api_login -- --ignored    # live API: is the server still on the pinned 0.16.x?
cargo test --test live_events -- --ignored  # live events service: does the websocket handshake succeed?
cargo test --lib -- --ignored               # real Keychain: writes and deletes a throwaway test item
```
