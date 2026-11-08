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

## Chunk 3: Join and leave a voice channel

### Changes Made
- `src/voice.rs` (the tokio side, no GPUI):
  - `VoiceError { Api(ApiError), LiveKit(RoomError), Audio(AudioError) }` with `text()`:
    - Api: `join_error_text`.
    - LiveKit: "Couldn't connect to the voice server. Try again."
    - Audio: "Couldn't start your microphone and speakers."
  - `VoiceEvent::Disconnected` and `VoiceEvents` (a `futures` unbounded receiver).
  - `VoiceCall::join(channel, url, token, publish_mic, subscribe)`:
    - Creates `PlatformAudio`, then `Room::connect` with `auto_subscribe = false`.
    - With `subscribe`, subscribes to the audio publications of everyone already in the room.
    - Spawns `read_events` on `runtime()`.
    - With `publish_mic`, publishes `LocalAudioTrack::create_audio_track("microphone", audio.rtc_source())` with `TrackSource::Microphone`.
    - Once the room exists, dropping the half-built call (a publish error, or the join being cancelled) closes it.
  - `read_events`:
    - Logs participants joining and leaving (info), audio subscribed, unsubscribed and failed (debug/warn), and `Reconnecting` / `Reconnected` (info).
    - Subscribes to newly published **audio** only, when `subscribe` is on.
    - On `Disconnected` (or the event stream ending) sends `VoiceEvent::Disconnected`.
  - `VoiceCall::leave(self)` logs and drops. `Drop` aborts the event task (so leaving isn't reported as a disconnect), then on `runtime()`: `room.close()` (ignoring `AlreadyClosed`), drop the `PlatformAudio`, log "left".
  - Removed `webrtc_ready()`.
- `src/main.rs`: removed chunk 1's startup `webrtc_ready()` call. The rustls provider stays.
- `src/ui/chat.rs`:
  - `ActiveCall { channel, state: CallState, _task }` with `CallState::{Joining, Connected(VoiceCall)}`, kept as `ChatView.voice: Option<ActiveCall>`.
  - `voice_state(channel)` reports whether you're joining or in that channel's call.
  - `join_voice(channel, window, cx)`:
    - Leaves any current call, then reads `can_speak` / `can_listen`.
    - One `spawn_io` runs `join_call(channel, node)` and then, straight away, `VoiceCall::join`.
    - One GPUI task then sets `Connected` (or clears the call and toasts `VoiceError::text()`, with a warn log), and waits on `VoiceEvents`. If those say the call ended, it clears it and toasts "Disconnected from voice."
    - Leaving drops that task, so nothing is toasted.
  - `leave_voice(cx)`.
  - Header: a small button after the search box, shown for `ChannelKind::Text { voice: true }` channels when the channel is usable (not age-gated), `voice_node` is known and `can_connect`. It reads "Join voice" (primary), "Joining…" (primary, loading, disabled) or "Leave voice" (danger).
  - Channel row: the icon is drawn in `theme.primary` (the palette's accent) while `Connected` in that channel.
  - Teardown needs no new code: `voice` is a `ChatView` field, so dropping the view (log out, sign out, closing the window) drops the `VoiceCall`, whose `Drop` closes the room.
- `scripts/bundle.sh`: `NSMicrophoneUsageDescription` = "RuSToat uses the microphone in voice calls."
- `scripts/entitlements.plist`: `com.apple.security.device.audio-input`, and its comment explains it.

### Divergences
- **Minor:** `VoiceCall::join` also takes the channel ID, for logging.
- **Minor:** `VoiceError` has a third case, `Audio`, for `PlatformAudio::new()` failing (no audio devices, say), with its own text. Assumption 8 didn't list one.
- **Minor:** a GPUI task that fails without an answer (cancelled or panicked) toasts the generic "Couldn't join the call. Try again."
- **Finding (testing, not product): no microphone prompt under a tool-launched `cargo run`.** macOS attributes microphone access to the *responsible* app. The `cargo run` started from this session inherits it from `HumanLayer-Local.app`, whose `Info.plist` has no `NSMicrophoneUsageDescription`. TCC therefore denies the mic silently, with no prompt, and the call would send silence. Launching the bundle through LaunchServices (`scripts/bundle.sh debug` then `open target/debug/RuSToat.app`) makes RuSToat responsible for itself. From a normal terminal, `cargo run` attributes the mic to the terminal, as the plan says.
- **Risk #1468 did not show:** join → leave → join worked four times in a row with a fresh `PlatformAudio` each time (it logged "released Platform ADM (ref_count now: 0)" then "acquired" again). Assumption 11's fallback (one `PlatformAudio` for the process) wasn't needed so far; chunk 4 runs five cycles.

### Verification Performed
- `cargo test`: 71 test binaries, 1061 passed (one new `voice::tests::a_failed_connection_or_audio_says_which`), 0 failed, 5 ignored.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `src/ui/chat.rs` 17 (unchanged, after moving the new `use` to the end of the list), `src/voice.rs` 0, `src/main.rs` 0.
- `plutil -lint scripts/entitlements.plist`: OK. The bundled `Info.plist` has the usage string.
- Checked that the token isn't logged: our client logs only the method and URL, and the error on failure. LiveKit sends the token in an `Authorization` header and logs only redacted URLs (`livekit_net::redact_url`).
- `RUST_LOG=stoat_gpui=debug,livekit=info cargo run` (attributed to HumanLayer-Local, so no mic): join → "joined … as <user>", "sending the microphone"; leave → "Disconnected … ClientInitiated", "released Platform ADM", "left". Twice in a row. Connection setup took 2.7 s and 1.8 s.
- The bundled debug app launched with `open`:
  - Join and leave as account A.
  - Then account B joined. The web client user (account A) came in, and the log showed "01KH4Q74… joined", "subscribing to …'s Microphone audio", "subscribed".
  - Then "Leave voice" closed the room cleanly.
  - LiveKit connection setup took 1.6 s and 1.5 s.

### Manual Validation
- The user ran the call against the stoat.chat web client and reported "it's looking good". Two-way audio, the button states and the row icon were checked by eye.
- Not yet confirmed explicitly, and carried into chunk 4's trial: the bundled app's microphone prompt text, and leaving by logging out mid-call (the log shows only button leaves).
