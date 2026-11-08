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

## Chunk 4: Trial, write-up and decision

### Changes Made
- `docs/plans/voice-spike-implementation.md`: this entry and "Results and decision" below.
- `docs/plans/roadmap.md`: milestone 32's verdict and a link here. Milestone 33 gets the CPU work as its first item.
- No code changes: the trial turned up nothing to fix in `src/voice.rs` or `src/ui/chat.rs`.

### Divergences
- **The trial ran on the release bundle launched with `open`** (`scripts/bundle.sh release`, then `open -n --env RUST_LOG=stoat_gpui=debug,livekit=info --stdout … target/release/RuSToat.app`), not `cargo run`. From this session, `cargo run` can't use the microphone (chunk 3), and the release build is what the CPU criterion is about.
- **CPU was measured three ways.** Two runs of the plan's `top -l 13 -s 5 -pid … -stats cpu` (first sample dropped), plus the process's CPU-time delta over 60 s from `ps -o utime=,time=`, and a 5-second `sample` to see where it goes.
- **Not run (waived by the user, "they seem fine"):** five join/leave cycles in a row, leaving by logging out or by closing the window, joining a second channel while in one, and taking a call over from RuSToat. What the logs do show is under Stability below.
- **Not reported in detail:** latency as it felt, echo on speakers, and what happened when the audio device changed. The user reported "audio stuff seems to be working" and "all looks good", including both themes and the bundle's mic prompt.

### Verification Performed
- `cargo test`: 71 test binaries, 1061 passed, 0 failed, 5 ignored (re-run at the end; no code changed in this chunk).
- `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
- The user and the implementer ran the trial against stoat.chat's web client with a second account on the official `hel1` node; results below.

## Results and decision

### Build
| | Before (`3bccfcd`) | After (chunk 1) | Change |
|---|---|---|---|
| Clean release build | 80 s | 99 s | **+19 s** |
| Clean release build, WebRTC already on disk (`LK_CUSTOM_WEBRTC`) | – | 92 s | +12 s |
| Incremental release rebuild (`touch src/ui/chat.rs`) | 19 s | 19 s | ±0 |
| Clean debug build | 55 s | 75 s | +20 s |
| `cargo test --no-run` | 32 s | 37 s | +5 s |
| `target/` on disk | 8.9 GB | 12 GB | +3.1 GB |

- **The WebRTC download:** `webrtc-mac-arm64-release.zip` (`webrtc-a545de4`) is 277 MB and took 4 s here. It extracts to 1.1 GB.
- **Where it goes:** under `target/<profile>/build/scratch-*/out/livekit_webrtc/`, so debug and release each keep a copy (2.2 GB together).
- **`cargo clean` removes it,** and the next clean build of each profile downloads it again. `LK_CUSTOM_WEBRTC=<kept copy of mac-arm64-release/>` avoids that.
- **Offline:** a build with a cold `target/` and no network fails. Not tried, but it follows from the build script.

### Size
| | Before | After | Change |
|---|---|---|---|
| Release binary | 50,304,576 B | 68,182,192 B | +17.9 MB (+36%) |
| `RuSToat.app` (release) | 48 MB | 65 MB | +17 MB |

### What the crate needs
- **Dependencies:** 594 → 655 crates on `aarch64-apple-darwin` (+61). Crates in more than one version go from 46 to 55: a second `tokio-tungstenite`/`tungstenite` (0.29), a third `reqwest` (0.12), and `sha1`/`digest` and their helpers. There's still one `rustls` (0.23.46) and one `prost` (0.14.4).
- **WebRTC:** a prebuilt static `libwebrtc.a` (the download), a C++20/ObjC++ `cxx` bridge built with Xcode's clang (Apple clang 21, Xcode 27.2: no trouble), and 17 system frameworks, ScreenCaptureKit among them.
- **`-ObjC` on the app binary only** (`build.rs`, `cargo:rustc-link-arg-bins`). The 70 test binaries link and pass without it.
- **A rustls crypto provider, chosen explicitly.** LiveKit brings `ring` next to reqwest's `aws-lc-rs`, and with both rustls panics on the first TLS connection unless the process installs a default. `main` installs `aws-lc-rs`. LiveKit's own TLS uses the same process default.
- **Tokio:** LiveKit runs on our existing 2-worker `runtime()`; nothing else was needed. WebRTC adds its own threads: network, worker, signaling, CaptureWorker and RenderWorker, and two CoreAudio IO threads during a call.
- **Microphone permission:**
  - `NSMicrophoneUsageDescription` in the bundle's `Info.plist`, and `com.apple.security.device.audio-input` for hardened-runtime (release-signed) builds.
  - The bundle asks on first join, naming RuSToat (user confirmed). Launch it with `open` or from Finder so RuSToat is its own responsible process.
  - `cargo run` attributes the mic to whichever app started it. From a terminal that's the terminal; from an app with no usage description (HumanLayer-Local here) the mic is denied silently with no prompt, and the call sends silence.

### Audio
- **Two-way audio:** RuSToat and the web client heard each other. One call ran 9 minutes (17:24:44–17:33:58+, left at 17:37:59) with no reconnect, warning or drop in the log, and the user reported audio working.
- **Joining:** about 3 s from click to "joined" (Stoat's `join_call`, then LiveKit). LiveKit's own "connection setup took" was 1.5–2.7 s over six joins.
- **Subscriptions:** only remote **audio** publications were subscribed (`TR_…` Microphone tracks); no video was decoded.
- **Echo cancellation:** WebRTC's software AEC, NS and AGC were on ("Audio processing configured: AEC=true, AGC=true, NS=true, prefer_hw=false").
- **Latency, echo on speakers, and a device change mid-call:** not reported in detail (see Divergences).

### Stability
- **Join/leave cycles:** six joins over three processes, each with a fresh `PlatformAudio` (the log shows "released Platform ADM (ref_count now: 0)" between them), with no hang or crash:
  - two in a row under `cargo run`;
  - two in a row in the debug bundle;
  - two in the release bundle, one ended by LiveKit and one by Leave.
  - Join → leave → join therefore works at least once per process. **Five in a row in one process wasn't run** (waived). #1468 (a reconnect hang after the last peer connection drops) didn't show, and Assumption 11's fallback wasn't needed.
- **Leaving:** "Leave voice" closes the room in the same second ("Disconnected … ClientInitiated", then "left"). The window stayed responsive while joining and leaving.
- **Taken over by the web client** (found by accident, manual test 4 in reverse):
  - RuSToat was in the call as account A when the web client joined the same channel as account A.
  - The web client wasn't refused with "already connected", as the plan expected. Instead LiveKit removed RuSToat's participant: `Disconnected … ParticipantRemoved`, the toast "Disconnected from voice.", the button reset, and the audio module was released.
  - Two clients with the same identity can't share a room, and the newest wins.
- **Not run (waived):** leaving by logging out or closing the window, switching channels mid-call, and RuSToat taking a call over from the web client. Logging out and closing the window rely on dropping `ChatView`, which drops the `VoiceCall`; that's the same `Drop` path the Leave button uses.

### CPU
Release bundle, Apple M3 Max. `top`'s %CPU is per core: 100% is one core, and the machine has 16.

| State | `top` mean (12 × 5 s) | Max | Other measure |
|---|---|---|---|
| Idle, signed in, outside a call | **0.67%** | 0.8% | |
| Silent call, mic published (first 60 s; includes a 9 s gap between calls) | **7.98%** | 10.1% | |
| Silent call, mic published, the web client user in the room and quiet | **8.43%** | 9.1% | `ps` CPU time over 60 s: **7.05%** (5.5% user, 1.6% system) |

Where it goes (`sample`, 5 s, in the call):
- **Our threads:** the main thread is parked in `nextEventMatchingMask`, so GPUI isn't redrawing, and both `stoat-io` threads are idle.
- **WebRTC:** all the busy time is in its audio pipeline:
  - `CaptureWorkerThread`, the largest: AEC3's delay estimator (`MatchedFilterCore_NEON`, `ApplyFilter_Neon`), noise suppression, and Opus/SILK encoding (`silk_NSQ_del_dec_neon`).
  - `RenderWorkerThread`: NetEq, Opus decode and comfort noise, and mixing.
  - The CoreAudio IO threads and WebRTC's network thread, a little.

The silent call costs about 7–8% of one core, roughly 0.5% of the whole machine. It's WebRTC's software audio processing working every 10 ms, and none of it is RuSToat's code.

### Rough edges
1. **rustls provider clash** (above). Fixed with one line in `main`; any future crate that pulls in `ring` is covered by the same line.
2. **Microphone attribution under `cargo run`** (above). Test the mic through the bundle.
3. **The WebRTC download is per profile and wiped by `cargo clean`.** 277 MB per clean build per profile, and builds need the network.
4. **Same-account calls:** the newest client always wins in LiveKit. RuSToat's `force_disconnect` matches that from our side. When the web client takes the call over, RuSToat just shows "Disconnected from voice."; milestone 33 could say "You joined this call somewhere else." from `DisconnectReason::ParticipantRemoved`.
5. **`PlatformAudio` known issues:**
   - #1468 (reconnect hang) wasn't seen in six joins.
   - #1560 (publishing opens the mic) is how it's meant to work here.
   - #1638 (`set_*_device` fails while audio runs) wasn't exercised; milestone 34's device picker will hit it.
6. **No mic prompt at launch:** creating `PlatformAudio` with recording enabled (chunk 1's smoke check) didn't prompt. The prompt comes when a track is published.

### Verdict against Decision 4
| Criterion | Result | Verdict |
|---|---|---|
| Two-way audio with a web client user stays clear for 5 minutes | 9 minutes, no drop, the user reports it working | **Pass** |
| Five join/leave/join cycles without a hang or crash | Six joins over three processes, at most two in a row in one, no hang or crash; five in a row not run (waived) | **Not shown.** No failure seen, but the criterion as written wasn't run. |
| A clean release build takes no more than 5 minutes longer | +19 s (+12 s with WebRTC kept) | **Pass** |
| CPU stays under 5% in a silent call | 7.05–8.43% across three measurements, all WebRTC's audio processing | **Fail** |

### Decision: go, with changes
The SDK builds and links cleanly, joins Stoat's LiveKit, carries clear two-way audio with the web client, leaves cleanly, and costs little to build. What fails is a silent call's CPU: about 7–8% of a core, against a 5% target. That is WebRTC's own audio processing, not our integration. Milestones 33–37 go ahead, on these conditions:

1. **First, in milestone 33: bring a silent call under 5%, or agree on a new target.**
   - Measure what each part of WebRTC's audio processing costs: `PlatformAudio::set_echo_cancellation` / `set_noise_suppression` / `set_auto_gain_control` off, one at a time.
   - Check whether macOS voice-processing I/O can do the echo cancellation in hardware.
   - As a reference, measure the web client in a browser on this Mac, which runs the same WebRTC processing.
   - The plan's rejected `cpal` + `NativeAudioSource` route isn't obviously cheaper, because it still needs WebRTC's processing for echo cancellation. Try it only if the measurements show `PlatformAudio` itself is the overhead.
2. **Run the five-cycle check properly in milestone 33,** before building on join and leave. Also run the waived checks there: leaving by logging out and by closing the window, switching channels, and RuSToat taking over from the web client.
3. **Keep the rustls provider line in `main`,** and don't test the microphone through a tool-launched `cargo run`.

## Summary

_Completed: 2026-11-08_

### Chunks Completed
1. Add the SDK and measure what it costs (`5e58c75`)
2. Ask Stoat for a call (`d292d80`)
3. Join and leave a voice channel (`293346d`)
4. Trial, write-up and decision (this commit)

### Divergences
- Chunk 1: rustls had two crypto providers (LiveKit's `ring` beside reqwest's `aws-lc-rs`), so `main` installs `aws-lc-rs` and `rustls` became a direct dependency. The smoke check ran on the blocking pool, under debug logging only.
- Chunk 2: a public `ChatView::voice_node()` getter; `voice_node` is set before `web_app` (a move); `CreateVoiceUserResponse` was usable as is.
- Chunk 3: `VoiceCall::join` also takes the channel ID; a third `VoiceError::Audio` case with its own text; the generic text for a failed task. Finding: no mic prompt under a tool-launched `cargo run` (the responsible app has no usage description).
- Chunk 4: the trial ran on the release bundle launched with `open`; CPU was measured three ways. The user waived five join/leave cycles in a row, leaving by logging out or closing the window, switching channels, and RuSToat taking a call over. Latency, echo and the device change weren't reported in detail. Found: the web client joining as the same account removes RuSToat from the call.

### Definition of Done
- [x] `livekit` 0.10.1 is in `Cargo.toml`, `build.rs` passes `-ObjC` to the app binary on macOS only, and RuSToat builds, launches and passes `cargo test` (1061 passed) and `cargo clippy --all-targets -- -D warnings`.
- [x] `RootConfig` reads `features.livekit`. An instance without voice, or with it disabled, shows no Join button. `voice_node()` is tested for both; the button is gated on it. No instance without voice was tried by hand.
- [~] `join_call` sends exactly `{"node":…,"force_disconnect":true}` to `/channels/{id}/join_call` (tested), and each Stoat voice error has its own message (tested). *Joining from RuSToat ends the same account's call on another client:* not run (waived). The reverse was seen: the web client joining as the same account ends RuSToat's call.
- [~] In a voice channel where you have Connect, "Join voice" connects with the default microphone (with Speak) and plays others' audio (with Listen), and "Leave voice" disconnects: met. *Joining another channel leaves the first:* written (`join_voice` calls `leave_voice` first), but not run in the trial (waived).
- [~] Logging out, signing out or closing the window while in a call disconnects it: by construction (dropping `ChatView` drops the `VoiceCall`, whose `Drop` closes the room), but not run in the trial (waived).
- [x] The bundle's `Info.plist` has `NSMicrophoneUsageDescription`, and `scripts/entitlements.plist` has `com.apple.security.device.audio-input`.
- [~] This record has before/after build times, binary and `.app` sizes, the WebRTC download, what the crate needs, audio, CPU in a silent call and the rough edges, with a verdict per criterion of Decision 4 and an overall **go, with changes**. Five join/leave cycles in a row weren't run; six joins over three processes are recorded instead.
- [x] `docs/plans/roadmap.md` records the decision for milestone 32 and links here.
- [x] No touched file's rustfmt drift grew (`src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15; new and clean files 0).

### Remaining Manual Steps
- The waived trial checks, carried into milestone 33 by the roadmap:
  - five join/leave cycles in a row;
  - leaving by logging out and by closing the window;
  - switching channels mid-call;
  - RuSToat taking a call over from the web client.
- The milestone 33 CPU investigation (Decision, item 1).
