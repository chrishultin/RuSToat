# Implementation Record: Voice spike (RuSToat milestone 32)

_Plan: docs/plans/voice-spike.md_
_Started: 2026-11-08_

Supervision mode: continuous.

Pre-existing dirty files, kept out of every commit:
- `.gitignore`: a duplicated, uncommitted "Riptide artifacts (cloud-synced)" block (`.humanlayer/tasks/`), added by tooling.
- `.shadow/`: untracked tooling folder.

The plan itself (`docs/plans/voice-spike.md`) was untracked at the start and is committed with chunk 1.

Machine: MacBook Pro, Apple M3 Max (16 cores, 64 GB), macOS (Darwin 25.6.0), Xcode 27.2 (Apple clang 21.0.0), Rust 1.99.1 (pinned), `aarch64-apple-darwin`.

## Chunk 1: Add the SDK and measure what it costs

### Changes Made
- `Cargo.toml`:
  - `livekit = { version = "=0.10.1", features = ["rustls-tls-native-roots"] }`, default features (`native`), with a comment: what it is, pinned, upstream not Zed's fork, the WebRTC download, `-ObjC`.
  - `rustls = { version = "0.23", default-features = false, features = ["aws-lc-rs"] }` (see Divergences). Already in the tree at 0.23.46.
- `Cargo.lock`: the resolved additions.
- `build.rs` (new): on macOS targets only (`CARGO_CFG_TARGET_OS == "macos"`), `cargo:rustc-link-arg-bins=-ObjC`, with a comment on rust-sdks #795 and why only the binary.
- `src/voice.rs` (new): module doc, and `webrtc_ready() -> Result<(Vec<String>, Vec<String>), livekit::AudioError>`, which creates a `PlatformAudio` and returns the recording and playout device names, then drops it.
- `src/lib.rs`: `pub mod voice;`.
- `src/main.rs`:
  - Installs rustls's `aws_lc_rs` provider as the process default, first thing after logging starts.
  - When debug logging is on, runs `webrtc_ready()` once on the tokio runtime's blocking pool and logs the devices (warns if it fails). To be removed in chunk 3.

### Divergences
- **Minor (spike finding): two rustls crypto providers.** With `livekit` added, the app linked and WebRTC started, but the events websocket panicked on the first TLS connection: "Could not automatically determine the process-level CryptoProvider from Rustls crate features." Our `reqwest` 0.13 enables rustls's `aws-lc-rs`, and LiveKit's `livekit-net` (through `tokio-rustls`) and its `reqwest` 0.12 enable `ring`. With both on, rustls won't pick a default, and `tokio-tungstenite`'s `ClientConfig::builder()` panics. Fix: `rustls` as a direct dependency (the version already in the tree) and `rustls::crypto::aws_lc_rs::default_provider().install_default()` at the top of `main`, keeping the provider the app already used. Integration tests don't need it: they talk plain HTTP/WS to wiremock and passed without it.
- **Minor: where the startup smoke call runs.** The plan says `main.rs` calls `webrtc_ready()` "at startup at debug level". It runs on `runtime().spawn_blocking`, so WebRTC starts on a tokio thread as it will in chunk 3, not on the main thread, and only when `log_enabled!(Debug)`.

No later chunk is affected. LiveKit's own TLS (`livekit-net`, `tokio-tungstenite` 0.29) also builds its config with `rustls::ClientConfig::builder()`, the process default. Without this fix, chunk 3's `Room::connect` would have panicked the same way; with it, LiveKit uses `aws-lc-rs` too.

### Verification Performed
- `RUST_LOG=stoat_gpui=debug,livekit=info cargo run` with the change:
  - `PlatformAudio: acquired Platform ADM`, 8 recording and 10 playout devices; `webrtc ready: inputs ["default (Elgato Wave:3)", …], outputs ["default (ODAC-revB USB DAC)", …]`. Index 0 of each list is macOS's default device, named `default (<device>)`.
  - No "unrecognized selector" or other crash at launch.
  - The first run panicked in rustls (above). After the fix, both signed-in accounts reached `events: Ready`.
- `RUST_LOG=stoat_gpui=debug scripts/run-bundled.sh release`: bundled, signed (Apple Development identity), launched, logged `webrtc ready`, and both accounts reached `Ready`.
- `cargo test`: 70 test binaries, 1051 passed, 0 failed, 5 ignored. **The test binaries link without `-ObjC`**, so `-ObjC` stays on the app binary only (`rustc-link-arg-bins`).
- `cargo clippy --all-targets -- -D warnings`: clean.
- `rustfmt --edition 2024 --check build.rs src/voice.rs`: clean. `src/main.rs`: 0 `Diff in` (hand-formatted). `src/lib.rs`: one line added by hand.

#### Build and size figures
Same commands both times (`/tmp/voice-spike/measure.sh`, run from the repo root). "Before" is commit `3bccfcd`, the commit before the SDK. "After" is that commit plus this chunk's changes. Each clean build starts from `cargo clean`, with the cargo registry already warm, so crates aren't downloaded but WebRTC is.

| Measurement | Command | Before | After | Change |
|---|---|---|---|---|
| Clean release build | `cargo clean && time cargo build --release` | 80 s | 99 s | +19 s |
| Clean release build, WebRTC already on disk | `cargo clean && LK_CUSTOM_WEBRTC=<copy> cargo build --release` | – | 92 s | +12 s |
| Release binary | `ls -l target/release/stoat-gpui` | 50,304,576 B (48.0 MiB) | 68,182,192 B (65.0 MiB) | +17.9 MB (+36%) |
| `.app` bundle | `scripts/bundle.sh release && du -sh target/release/RuSToat.app` | 48 MB | 65 MB | +17 MB |
| Incremental release rebuild | `touch src/ui/chat.rs && time cargo build --release` | 19 s | 19 s | ±0 |
| Clean debug build | `cargo clean && time cargo build` | 55 s | 75 s | +20 s |
| Test binaries | `time cargo test --no-run` (after the debug build) | 32 s | 37 s | +5 s |
| `target/` on disk | `du -sh target` (after the debug build and tests) | 8.9 GB | 12 GB | +3.1 GB |

The WebRTC download:
- **Size and time:** `webrtc-mac-arm64-release.zip` from the `webrtc-a545de4` release is 277,130,371 bytes (264 MiB). `curl` fetched it in 4 s on this connection. It extracts to 1.1 GB.
- **Where it goes:** the `scratch` crate's out directory, `target/<profile>/build/scratch-*/out/livekit_webrtc/livekit/mac-arm64-release-webrtc-a545de4/`. Because `scratch` is a build dependency, debug and release each get their own copy (2.2 GB when both exist).
- **`cargo clean` removes it,** so every clean build downloads it again: once for debug and once for release.
- **The workaround:** `LK_CUSTOM_WEBRTC=<dir>` pointing at a kept copy of `mac-arm64-release/` skips the download. It saved about 7 s here; the rest of the +19 s is compiling the `cxx` bridge and the extra crates.

Dependencies (`cargo tree -e normal,build,dev --target aarch64-apple-darwin`, unique name+version):
- **Count:** 594 → 655 packages (+61).
- **Crates in more than one version:** 46 → 55. The new duplicates are `tokio-tungstenite`/`tungstenite` (0.29 for LiveKit, 0.30 ours), `sha1`, `digest`, `block-buffer`, `crypto-common`, `generic-array`, `cpufeatures` and `foldhash`.
- **`reqwest`:** gains a third copy (0.11, 0.12 for LiveKit, 0.13).
- **Single versions:** one `rustls` (0.23.46) and one `prost` (0.14.4).
- **All platforms:** across every platform in the lock file (`cargo metadata`), 969 → 1029 packages.
- The plan's "118 → 135" duplicates came from a different count in a scratch copy; these figures use the same command before and after.

### Manual Validation
- The user ran the bundled release app (`scripts/run-bundled.sh release`): it works as before.
- macOS did **not** ask for the microphone at launch, even though `PlatformAudio` enabled ADM recording during `webrtc_ready()`. Nothing was published, so nothing opened the mic.

## Chunk 2: Ask Stoat for a call

### Changes Made
- `src/api/client.rs`:
  - `RootFeatures.livekit: Option<VoiceFeature>` (`#[serde(default)]`).
  - New `VoiceFeature { enabled, nodes: Vec<VoiceNode> }` and `VoiceNode { name, public_url }`, both with serde defaults. `lat`/`lon` are ignored.
  - `RootConfig::voice_node() -> Option<&str>`: the first node's name when `enabled`, else `None`.
  - `StoatClient::join_call(channel, node) -> CreateVoiceUserResponse`: `POST /channels/{channel}/join_call` with a private `JoinCall { node, force_disconnect: true }` body (not `DataJoinCall`, which would send `"recipients": null`). The doc says the token lasts 10 seconds and must never be logged. The client's own logging only prints the method and URL, and on failure the error, never a success body.
  - `CreateVoiceUserResponse` added to the `revolt_models::v0` import list, which was re-wrapped by hand.
- `src/voice.rs`: `join_error_text(&ApiError) -> String` with the texts of Assumption 8 (minus the LiveKit one, which is chunk 3's). Everything else goes to `profile_edit::error_text(err, "Couldn't join the call. Try again.")`. Three unit tests.
- `src/permissions.rs`: `can_connect`, `can_speak` and `can_listen` (Connect, Speak, Listen on the channel), modelled on `can_view`.
- `src/store.rs`: `ChatStore::can_connect` / `can_speak` / `can_listen` wrappers beside `can_pin`.
- `src/ui/chat.rs`:
  - `voice_node: Option<String>` on `ChatView`, set from `root.voice_node()` when the root config loads, beside `group_size`.
  - A public `voice_node()` getter, so the field isn't dead code before chunk 3 reads it.
- `tests/fixtures/root.json`: the live `features.livekit` section (`hel1`, `wss://livekit.hel1.voice.stoat.chat`), fetched from `api.stoat.chat` today.
- `tests/api_instance.rs`: `the_first_voice_node_is_offered`, and `no_voice_node_when_voice_is_off_or_missing` (disabled, empty node list, no `livekit` key).
- `tests/api_voice.rs` (new): `join_call` sends exactly `{"node":"hel1","force_disconnect":true}` with the session token to `/channels/c1/join_call` and parses `{token, url}`. A 400 `CannotJoinCall` and a 403 `MissingPermission` stay `ApiError::Api`.
- `tests/permissions.rs`:
  - All three permissions are true for an ordinary member with Stoat's default permissions, and `can_connect` is false for a non-member.
  - A channel default-permission override denying Connect, Speak or Listen takes away only that one.

### Divergences
- **Minor:** `ChatView::voice_node()`, a public getter, isn't in the plan. It keeps clippy's dead-code check quiet until chunk 3 reads the field.
- **Minor:** `voice_node` is set before `web_app`, because `root.app` is moved out of `root` on that line.
- **Minor:** `CreateVoiceUserResponse` derives `Deserialize` with the `serde` feature RuSToat already enables, so the plan's fallback private struct wasn't needed.

### Verification Performed
- `cargo test`: 71 test binaries, 1060 passed (9 new), 0 failed, 5 ignored.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift (`Diff in` counts): `src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15 (all unchanged); `src/permissions.rs`, `tests/permissions.rs`, `tests/api_instance.rs`, `src/voice.rs` and `tests/api_voice.rs` 0.

### Manual Validation
None required for this chunk.
